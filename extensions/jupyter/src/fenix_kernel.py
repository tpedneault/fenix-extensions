"""Fenix's Jupyter extension's kernel: runs a notebook's cells for it.

Fenix starts it with the notebook's Python and talks to it in JSON lines:
what to do on stdin ({"op": "run", "id": 1, "code": "..."}, "interrupt",
"restart", "input", "shutdown"), what happened on stdout ({"ev": "ready"},
"output", "clear", "input", "done", "died", "log").

With jupyter_client and a kernel for the notebook (ipykernel's python3),
cells run on a real Jupyter kernel: magics, display(), widgets' text,
everything IPython does. Without, they run here, in one namespace, as
Python runs them: the last expression shown, print()'s output, input()
asked of you, matplotlib's figures drawn as pictures, `!cmd` and `%pip`.
It needs nothing but Python 3.8 or later.
"""

import ast
import base64
import builtins
import io
import json
import linecache
import os
import platform
import queue
import subprocess
import sys
import threading
import time
import traceback

PROTO = None
_send_lock = threading.Lock()
inputs = queue.Queue()
commands = queue.Queue()


def send(**msg):
    line = json.dumps(msg, ensure_ascii=False, default=repr)
    with _send_lock:
        PROTO.write(line + "\n")
        PROTO.flush()


def log(text):
    send(ev="log", text=str(text))


def wait_input(id, prompt, password=False):
    """A line of yours, asked for in Fenix."""
    # One given after an interrupt, for an ask that's gone, isn't this one's.
    while not inputs.empty():
        inputs.get_nowait()
    send(ev="input", id=id, prompt=prompt, password=bool(password))
    while True:
        try:
            return inputs.get(timeout=0.1)
        except queue.Empty:
            continue


# ---------------------------------------------------------------- Jupyter


class Jupyter:
    """A Jupyter kernel, through jupyter_client."""

    mode = "jupyter"

    def __init__(self, name):
        from jupyter_client.kernelspec import KernelSpecManager
        from jupyter_client.manager import KernelManager

        specs = KernelSpecManager().find_kernel_specs()
        pick = name if name in specs else ("python3" if "python3" in specs else None)
        if pick is None:
            raise RuntimeError("no Jupyter kernel for %s (there's %s)" % (name or "python3", ", ".join(sorted(specs)) or "none"))
        self.km = KernelManager(kernel_name=pick)
        self.note = "" if pick == (name or pick) else "the notebook's kernel %s isn't here: %s runs it" % (name, pick)
        self.start()

    def start(self):
        self.km.start_kernel()
        self.kc = self.km.client()
        self.kc.start_channels()
        self.kc.wait_for_ready(timeout=120)
        self.label = self.km.kernel_spec.display_name if self.km.kernel_spec else self.km.kernel_name

    def run(self, id, code):
        msg_id = self.kc.execute(code, allow_stdin=True, store_history=True)
        count, status, idle, reply = None, "ok", False, False
        checked = time.monotonic()
        while not (idle and reply):
            busy = False
            try:
                m = self.kc.stdin_channel.get_msg(timeout=0)
                if m["msg_type"] == "input_request":
                    c = m["content"]
                    self.kc.input(wait_input(id, c.get("prompt", ""), c.get("password", False)))
                    busy = True
            except queue.Empty:
                pass
            if not reply:
                try:
                    r = self.kc.get_shell_msg(timeout=0)
                    if r["parent_header"].get("msg_id") == msg_id:
                        reply = True
                        content = r["content"]
                        count = content.get("execution_count", count)
                        if content.get("status") == "error":
                            status = "error"
                        elif content.get("status") == "aborted":
                            status = "aborted"
                    busy = True
                except queue.Empty:
                    pass
            try:
                m = self.kc.get_iopub_msg(timeout=0 if busy else 0.05)
            except queue.Empty:
                m = None
            if m is not None and m["parent_header"].get("msg_id") == msg_id:
                self.iopub(id, m)
                kind, c = m["msg_type"], m["content"]
                if kind == "status" and c.get("execution_state") == "idle":
                    idle = True
                elif kind == "execute_input":
                    count = c.get("execution_count", count)
                elif kind == "error":
                    status = "error"
            if time.monotonic() - checked > 0.5:
                checked = time.monotonic()
                if not self.km.is_alive():
                    send(ev="done", id=id, count=count, status="aborted")
                    raise Died("the kernel died")
        send(ev="done", id=id, count=count, status=status)

    def iopub(self, id, m):
        kind, c = m["msg_type"], m["content"]
        if kind == "stream":
            send(ev="output", id=id, output={"output_type": "stream", "name": c.get("name", "stdout"), "text": c.get("text", "")})
        elif kind in ("execute_result", "display_data", "update_display_data"):
            out = {"output_type": "display_data" if kind == "update_display_data" else kind, "data": c.get("data", {}), "metadata": c.get("metadata", {})}
            if kind == "execute_result":
                out["execution_count"] = c.get("execution_count")
            send(ev="output", id=id, output=out)
        elif kind == "error":
            send(ev="output", id=id, output={"output_type": "error", "ename": c.get("ename", ""), "evalue": c.get("evalue", ""), "traceback": c.get("traceback", [])})
        elif kind == "clear_output":
            send(ev="clear", id=id, wait=bool(c.get("wait", False)))

    def interrupt(self):
        self.km.interrupt_kernel()

    def restart(self):
        self.kc.stop_channels()
        self.km.restart_kernel(now=True)
        self.kc = self.km.client()
        self.kc.start_channels()
        self.kc.wait_for_ready(timeout=120)

    def shutdown(self):
        try:
            self.kc.stop_channels()
            self.km.shutdown_kernel(now=True)
        except Exception:
            pass


class Died(Exception):
    pass


# ---------------------------------------------------------------- built in


class Stream(io.TextIOBase):
    """A cell's stdout or stderr: sent a few times a second, not a
    message a print()."""

    def __init__(self, name, owner):
        self.name = name
        self.owner = owner
        self.parts = []
        self.lock = threading.Lock()

    def writable(self):
        return True

    def write(self, s):
        if not isinstance(s, str):
            s = str(s)
        with self.lock:
            self.parts.append(s)
        return len(s)

    def flush(self):
        with self.lock:
            text, self.parts = "".join(self.parts), []
        if text and self.owner.cell is not None:
            send(ev="output", id=self.owner.cell, output={"output_type": "stream", "name": self.name, "text": text})

    @property
    def encoding(self):
        return "utf-8"

    def isatty(self):
        return False


class Stdin(io.TextIOBase):
    """sys.stdin while a cell runs: each line asked of you."""

    def __init__(self, owner):
        self.owner = owner

    def readable(self):
        return True

    def readline(self, size=-1):
        return self.owner.ask("") + "\n"

    def read(self, size=-1):
        return self.readline()


class Builtin:
    """Cells run here, as Python runs them, in one namespace."""

    mode = "builtin"

    def __init__(self, why):
        self.note = "%s: cells run on Python itself, without IPython's magics (ipykernel brings them)" % why
        self.label = "Python %s" % platform.python_version()
        self.cell = None
        self.busy = False
        self.count = 0
        os.environ["MPLBACKEND"] = "module://fenix_mpl"
        self.stdout = Stream("stdout", self)
        self.stderr = Stream("stderr", self)
        sys.stdout, sys.stderr, sys.stdin = self.stdout, self.stderr, Stdin(self)
        builtins.input = self.input
        builtins.display = self.display
        self.ns = {"__name__": "__main__", "__builtins__": builtins, "display": self.display}
        self.ns["__fenix_magic__"] = self.magic
        self.ns["__fenix_shell__"] = self.shell
        threading.Thread(target=self.flusher, daemon=True).start()
        global fenix_show
        fenix_show = self.figures

    def flusher(self):
        while True:
            time.sleep(0.2)
            self.stdout.flush()
            self.stderr.flush()

    def ask(self, prompt):
        self.stdout.flush()
        self.stderr.flush()
        return wait_input(self.cell, prompt)

    def input(self, prompt=""):
        return self.ask(str(prompt))

    def display(self, *objs, **kw):
        for obj in objs:
            self.emit("display_data", obj)

    def emit(self, kind, obj):
        self.stdout.flush()
        self.stderr.flush()
        data = {"text/plain": repr(obj)}
        for meth, mime, binary in (("_repr_png_", "image/png", True), ("_repr_jpeg_", "image/jpeg", True), ("_repr_html_", "text/html", False), ("_repr_markdown_", "text/markdown", False)):
            f = getattr(obj, meth, None)
            if f is None or isinstance(obj, type):
                continue
            try:
                v = f()
            except Exception:
                continue
            if v is None:
                continue
            if isinstance(v, tuple):
                v = v[0]
            data[mime] = base64.b64encode(v).decode("ascii") if binary and isinstance(v, bytes) else v
        out = {"output_type": kind, "data": data, "metadata": {}}
        if kind == "execute_result":
            out["execution_count"] = self.count
        send(ev="output", id=self.cell, output=out)

    def figures(self):
        """matplotlib's open figures, as pictures, then closed."""
        plt = sys.modules.get("matplotlib.pyplot")
        if plt is None:
            return
        for num in plt.get_fignums():
            fig = plt.figure(num)
            buf = io.BytesIO()
            try:
                fig.savefig(buf, format="png", bbox_inches="tight")
            except Exception as e:
                self.stderr.write("the figure didn't draw: %s\n" % e)
                continue
            w, h = fig.get_size_inches() * fig.dpi
            self.stdout.flush()
            send(ev="output", id=self.cell, output={"output_type": "display_data", "data": {"image/png": base64.b64encode(buf.getvalue()).decode("ascii"), "text/plain": "<Figure size %dx%d with %d Axes>" % (w, h, len(fig.axes))}, "metadata": {}})
        plt.close("all")

    def magic(self, line):
        """A `%` line: %pip and %matplotlib; others say what they need."""
        name, _, rest = line.partition(" ")
        if name == "pip":
            return self.shell("%s -m pip %s" % (subprocess.list2cmdline([sys.executable]), rest))
        if name in ("matplotlib", "config", "load_ext", "autoreload"):
            return None
        if name == "cd":
            os.chdir(os.path.expanduser(rest.strip() or "~"))
            return None
        if name == "pwd":
            return os.getcwd()
        if name == "time":
            t = time.perf_counter()
            v = eval(rest, self.ns)
            self.stdout.write("Wall time: %.3f s\n" % (time.perf_counter() - t))
            return v
        self.stderr.write("%%%s needs IPython: install ipykernel into this Python (the notebook page's i key)\n" % name)
        return None

    def shell(self, cmd):
        """A `!` line: run, its output the cell's."""
        self.stdout.flush()
        p = subprocess.Popen(cmd, shell=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL, text=True, errors="replace")
        for line in p.stdout:
            self.stdout.write(line)
        p.wait()
        return None

    def transform(self, code):
        """IPython's `!cmd` and `%magic` lines as calls; a `%%` cell's
        first line said to be IPython's."""
        lines = code.split("\n")
        if lines and lines[0].lstrip().startswith("%%"):
            self.stderr.write("%s needs IPython: the rest runs as Python\n" % lines[0].split()[0])
            lines[0] = ""
        out = []
        for line in lines:
            s = line.lstrip()
            indent = line[: len(line) - len(s)]
            if s.startswith("!"):
                out.append("%s__fenix_shell__(%r)" % (indent, s[1:]))
            elif s.startswith("%") and not s.startswith("%%"):
                out.append("%s__fenix_magic__(%r)" % (indent, s[1:]))
            else:
                out.append(line)
        return "\n".join(out)

    def run(self, id, code):
        self.cell = id
        self.count += 1
        name = "<cell In[%d]>" % self.count
        status = "ok"
        try:
            src = self.transform(code)
            linecache.cache[name] = (len(src), None, src.splitlines(True), name)
            tree = ast.parse(src, filename=name, mode="exec")
            last = None
            if tree.body and isinstance(tree.body[-1], ast.Expr):
                last = ast.Expression(tree.body.pop().value)
            self.busy = True
            try:
                exec(compile(tree, name, "exec"), self.ns)
                if last is not None:
                    value = eval(compile(last, name, "eval"), self.ns)
                    if value is not None:
                        self.ns["_"] = value
                        self.emit("execute_result", value)
            finally:
                self.busy = False
            self.figures()
        except BaseException as e:
            self.busy = False
            if isinstance(e, SystemExit) and not e.code:
                pass
            else:
                status = "error"
                self.error(e)
        finally:
            self.stdout.flush()
            self.stderr.flush()
            send(ev="done", id=id, count=self.count, status=status)
            self.cell = None

    def error(self, e):
        tb = e.__traceback__
        # Its own frames left out: from the cell's down.
        while tb is not None and tb.tb_frame.f_code.co_filename == __file__:
            tb = tb.tb_next
        lines = traceback.format_exception(type(e), e, tb)
        self.stdout.flush()
        self.stderr.flush()
        send(ev="output", id=self.cell, output={"output_type": "error", "ename": type(e).__name__, "evalue": str(e), "traceback": [l.rstrip("\n") for l in lines]})

    def interrupt(self):
        if self.busy:
            import _thread

            _thread.interrupt_main()

    def restart(self):
        self.ns.clear()
        self.ns.update({"__name__": "__main__", "__builtins__": builtins, "display": self.display, "__fenix_magic__": self.magic, "__fenix_shell__": self.shell})
        self.count = 0
        plt = sys.modules.get("matplotlib.pyplot")
        if plt is not None:
            plt.close("all")

    def shutdown(self):
        pass


fenix_show = None


# ---------------------------------------------------------------- the loop


def reader(kernel_ref):
    for line in sys.__stdin__:
        try:
            msg = json.loads(line)
        except ValueError:
            continue
        op = msg.get("op")
        if op == "interrupt":
            k = kernel_ref[0]
            if k is not None:
                try:
                    k.interrupt()
                except Exception as e:
                    log("interrupt: %s" % e)
        elif op == "input":
            inputs.put(msg.get("text", ""))
        else:
            commands.put(msg)
    commands.put({"op": "shutdown"})


def start(name, force_builtin):
    if not force_builtin:
        try:
            k = Jupyter(name)
            return k
        except ImportError as e:
            why = "%s isn't installed" % (getattr(e, "name", None) or "jupyter_client")
        except Exception as e:
            why = str(e) or type(e).__name__
    else:
        why = "chosen"
    return Builtin(why)


def main():
    global PROTO
    args = sys.argv[1:]
    opts = {"--cwd": None, "--kernel": None}
    force_builtin = False
    i = 0
    while i < len(args):
        if args[i] in opts and i + 1 < len(args):
            opts[args[i]] = args[i + 1]
            i += 2
        else:
            force_builtin |= args[i] == "--builtin"
            i += 1
    # Its stdout is for Fenix: whatever else writes to it goes to stderr.
    PROTO = os.fdopen(os.dup(1), "w", encoding="utf-8", newline="\n")
    os.dup2(2, 1)
    sys.stdout = sys.stderr
    if opts["--cwd"]:
        try:
            os.chdir(opts["--cwd"])
        except OSError as e:
            log("%s: %s" % (opts["--cwd"], e))
    here = os.path.dirname(os.path.abspath(__file__))
    # As Jupyter has it: the notebook's folder first, so its modules import.
    sys.path[:] = [""] + [p for p in sys.path if os.path.abspath(p or ".") != here] + [here]
    ref = [None]
    threading.Thread(target=reader, args=(ref,), daemon=True).start()
    kernel = start(opts["--kernel"], force_builtin)
    ref[0] = kernel
    send(ev="ready", label=kernel.label, mode=kernel.mode, note=kernel.note, python=sys.executable)
    while True:
        try:
            msg = commands.get(timeout=0.5)
        except queue.Empty:
            continue
        except KeyboardInterrupt:
            continue
        op = msg.get("op")
        try:
            if op == "run":
                kernel.run(msg.get("id"), msg.get("code", ""))
            elif op == "restart":
                kernel.restart()
                send(ev="ready", label=kernel.label, mode=kernel.mode, note=kernel.note, python=sys.executable)
            elif op == "shutdown":
                kernel.shutdown()
                return
        except KeyboardInterrupt:
            # Between cells: nothing to stop.
            pass
        except Died as e:
            kernel.shutdown()
            send(ev="died", why=str(e))
            return
        except Exception:
            log(traceback.format_exc())
            if op == "run":
                send(ev="done", id=msg.get("id"), count=None, status="error")


if __name__ == "__main__":
    main()
