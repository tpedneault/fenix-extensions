# Discord

Discord Rich Presence for Fenix: your Discord profile shows the file
you're in, its language's picture, the project, what you're debugging
or the task that's running, and how long you've been at it.

```text
Fenix
Editing main.rs
In fenix-extensions
00:42 elapsed
```

It talks to Discord's desktop app on this machine, by its local socket
-- `discord-ipc-0` to `-9`: a named pipe on Windows, a Unix socket in
`$XDG_RUNTIME_DIR`, `$TMPDIR` or `/tmp` elsewhere, and on Linux in
Discord's Flatpak and Snap folders too. Started before Discord, it
looks for it every few seconds, then every minute; when Discord quits
it looks again. The browser's Discord has no socket, so it can't be
reached.

## Your Discord application

What a profile shows comes from a Discord application: its name is what
you're "playing", and its pictures are its art assets. Fenix has none
registered, so you make one, once:

1. At [discord.com/developers/applications](https://discord.com/developers/applications),
   **New Application**, named `Fenix` (or what you'd like it to say).
2. Copy its **Application ID** (General Information) into
   `discord.client_id` (`SPC h s`, or `discord.client_id = "..."` in
   your settings).
3. Under **Rich Presence → Art Assets**, upload the pictures, named by
   the keys below (512 x 512 or larger, PNG or JPEG). One you don't
   upload is left blank; Discord can take a few minutes to show a new
   one.

Until `discord.client_id` is set it does nothing but say so.

| Key | What |
| --- | --- |
| `fenix` | Fenix's own picture: the small one, and the large one before you've opened a file (Fenix's `docs/brand/fenix-icon-256.png`, scaled up) |
| `idle` | The small one while you're idle |
| `debug` | The small one while you're debugging |
| `task` | The small one while a task runs |
| `text` | A file of a language not below |
| `rust` `python` `javascript` `typescript` `c` `cpp` `csharp` `go` `java` `kotlin` `swift` `ruby` `php` `lua` `zig` `haskell` `ocaml` `elixir` `erlang` `clojure` `scala` `dart` `r` `julia` `shell` `powershell` `tcl` | The file's language, the large one |
| `html` `css` `sass` `vue` `svelte` `json` `yaml` `toml` `xml` `markdown` `latex` `sql` `nix` `arduino` `mermaid` `cmake` `docker` `make` `csv` | The same |

A language is known by the file's extension (`.rs`, `.hpp`), or its
name for those that have none (`Dockerfile`, `Makefile`,
`CMakeLists.txt`).

## What it shows

- **Editing main.rs**, the language's picture (`rust`) and name, with
  Fenix's picture small beside it.
- **In fenix-extensions**: the project's folder's name. A file in no
  project shows none.
- **Elapsed**: since you came to the project (`discord.elapsed =
  "project"`), to the file (`"file"`), or not shown (`"none"`).
- **Debugging app.py**, the `debug` picture small (its text `Paused
  (breakpoint)` while it's stopped), the time since the session started:
  while a debug session runs, by its program or its launch's name. An
  adapter's child sessions (js-debug's) show as the one you started.
- **Running cargo build**, the `task` picture small, the time since it
  started: while a task runs in the task pane, when you aren't
  debugging.
- **Idle**, Fenix's `idle` picture small and no time, after
  `discord.idle` minutes without a key (5; 0 never). A key anywhere
  brings it back.

On a page, a terminal or the explorer, the file you were last in stays.

## Privacy

| Setting | What it does |
| --- | --- |
| `discord.hide_files` | "Editing Rust", not the file's name; "Debugging Rust", not the program's; "Running a task", not its name |
| `discord.hide_projects` | No project's name |
| `discord.private` | Projects nothing is shown for while you're in them: names or folders, `*` for any characters (`work-*`, `/home/me/clients/*`) |

`Discord: toggle` (`discord.toggle`) clears it and disconnects for the
rest of this session; run again, it shows it again. `Discord: look for
Discord now` (`discord.reconnect`) doesn't wait for the next try, and
says how it went.

## Settings

| Setting | What it does |
| --- | --- |
| `discord.client_id` | Your Discord application's ID (empty: nothing shown) |
| `discord.idle` | Minutes without a key before you're idle (5) |
| `discord.elapsed` | `project`, `file` or `none` |
| `discord.hide_files` | Hide file names (off) |
| `discord.hide_projects` | Hide project names (off) |
| `discord.private` | Projects that show nothing |

## What it may do

- Talk to Discord's local socket (`ipc`): `discord-ipc-*`, and
  `app/com.discordapp.Discord/discord-ipc-*` and
  `snap.discord/discord-ipc-*` for Flatpak's and Snap's. Nothing else on
  the machine, and nothing on the network.
- Know the file you're in and its project (`editor = "read"`); it reads
  no file's text.
- Be told what has the focus, that you're typing, that a key was
  pressed, that a task started and ended, and that a debug session
  started, stopped, ran on and ended (`events = ["focus", "typing",
  "active", "tasks", "debug"]`).

What it sends Discord is only the activity above, as Discord's
`SET_ACTIVITY`, at most once every 4 seconds, with Fenix's process id
(`pid`): Discord clears the activity once Fenix is gone.
