def fenix_cargo(kind, file, out):
    # What a launch file debugs, built by cargo for the file you're in and
    # linked at out, the program the launch file names: kind 'bin' (the
    # file's binary, else its crate's), 'example' (the file's), or 'test'
    # (the file's test target, or its crate's tests in the file's module).
    # lldb-dap runs this before it makes its target (initCommands), as
    # CodeLLDB's cargo does. lldb's own Rust formatters are loaded too,
    # as rust-lldb loads them. No backticks: lldb would run what's in them.
    import json, os, shutil, subprocess
    def norm(p):
        return os.path.normcase(os.path.realpath(p))
    def say(text):
        print('fenix: ' + text)
    if os.path.lexists(out):
        os.remove(out)
    if not file:
        return say('open a Rust file of the crate to debug')
    cargo = os.environ.get('CARGO') or shutil.which('cargo') or os.path.expanduser('~/.cargo/bin/cargo')
    cwd = os.path.dirname(file)
    run = subprocess.run([cargo, 'metadata', '--format-version', '1', '--no-deps'], cwd=cwd, capture_output=True, text=True)
    if run.returncode != 0:
        return say('cargo metadata failed: ' + run.stderr.strip())
    here = norm(file)
    packages = json.loads(run.stdout)['packages']
    def holds(p):
        return here.startswith(norm(os.path.dirname(p['manifest_path'])) + os.sep)
    mine = sorted([p for p in packages if holds(p)], key=lambda p: -len(p['manifest_path']))
    pkg = (mine or packages)[0]
    targets = pkg['targets']
    def of(k):
        return [t for t in targets if k in t['kind']]
    own = [t for t in targets if norm(t['src_path']) == here]
    target, flag, only = None, None, None
    if kind == 'bin':
        named = [t for t in of('bin') if t['name'] in (pkg.get('default_run'), pkg['name'])]
        target = ([t for t in own if 'bin' in t['kind']] or named or of('bin') or [None])[0]
        flag = '--bin'
    elif kind == 'example':
        examples = of('example')
        target = ([t for t in own if 'example' in t['kind']] or (examples if len(examples) == 1 else []) or [None])[0]
        flag = '--example'
    else:
        tests = [t for t in own if t['kind'][0] in ('test', 'bench', 'bin')]
        if tests:
            target = tests[0]
        else:
            # A module: its crate's tests, those in it only.
            crates = of('lib') or of('proc-macro') or of('bin')
            target = crates[0] if crates else None
            if target:
                rel = os.path.relpath(here, norm(os.path.dirname(target['src_path']))).replace(os.sep, '/')
                parts = rel[:-3].split('/') if rel.endswith('.rs') else []
                if parts and parts[-1] == 'mod':
                    parts.pop()
                if parts and '..' not in parts and parts != [os.path.basename(target['src_path'])[:-3]]:
                    only = '::'.join(parts) + '::'
        k = target['kind'][0] if target else 'test'
        flag = '--lib' if k in ('lib', 'rlib', 'proc-macro') else '--' + k
    if target is None:
        return say('nothing in ' + pkg['name'] + ' to debug as ' + ('an example' if kind == 'example' else 'a ' + kind) + ' -- open its file')
    args = [cargo, 'test', '--no-run'] if kind == 'test' else [cargo, 'build']
    args += ['--message-format=json-render-diagnostics', '-p', pkg['name']]
    args += [flag] if flag == '--lib' else [flag, target['name']]
    if target.get('required-features'):
        args += ['--features', ','.join(target['required-features'])]
    say(' '.join(['cargo'] + args[1:]))
    run = subprocess.run(args, cwd=cwd, capture_output=True, text=True)
    if run.returncode != 0:
        print(run.stderr.rstrip())
        return say('the build failed')
    exe = None
    for line in run.stdout.splitlines():
        try:
            m = json.loads(line)
        except ValueError:
            continue
        if m.get('reason') == 'compiler-artifact' and m.get('executable') and m['target']['name'] == target['name'] and m['profile']['test'] == (kind == 'test'):
            exe = m['executable']
    if exe is None:
        return say('cargo built no program for ' + target['name'])
    folder = os.path.dirname(out)
    os.makedirs(folder, exist_ok=True)
    with open(os.path.join(folder, '.gitignore'), 'w') as f:
        f.write('*\n')
    try:
        os.link(exe, out)
    except OSError:
        shutil.copy2(exe, out)
    say(exe)
    try:
        import lldb
        debugger = lldb.debugger
    except (ImportError, AttributeError):
        return only
    if only:
        say('the tests in ' + only[:-2])
        debugger.HandleCommand('settings set target.run-args ' + only)
    rustc = os.path.join(os.path.dirname(cargo), 'rustc')
    run = subprocess.run([rustc if os.path.exists(rustc) or os.path.exists(rustc + '.exe') else 'rustc', '--print', 'sysroot'], cwd=cwd, capture_output=True, text=True)
    etc = os.path.join(run.stdout.strip(), 'lib', 'rustlib', 'etc')
    if run.returncode == 0 and os.path.isfile(os.path.join(etc, 'lldb_commands')):
        debugger.HandleCommand('command script import "' + os.path.join(etc, 'lldb_lookup.py') + '"')
        debugger.HandleCommand('command source -s 1 "' + os.path.join(etc, 'lldb_commands') + '"')
    return only
