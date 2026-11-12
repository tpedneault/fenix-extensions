# Log viewer

Log files in a pane: an entry a row, its line number, its level and its
text, coloured by level -- errors red, warnings yellow, debug and trace
dim. A stack trace, a traceback, a message over several lines is folded
into the entry that starts it; Enter opens it, a JSON line
pretty-printed.

```text
app.log  212.4 MiB · 2,394,391 entries · 2,793,055 lines · indexing 42%
e 1,203 error   w 15,233 warn   i 1,904,112 info   d 12,000 debug   t 0 trace   o 0 other
3,017 of 2,394,391 shown · /timeout/ -healthcheck user=bob · ?db 12 found
 ▂▃▃▂▂█▃▂▂▂▃▃▂▂▃▂▂▂▂▂▃▂▂▂▅▃▂▂▂▂▃▂▂▂▂▃▃▂▂▂
 2024-03-01 00:00:00          6 min a column           2024-03-01 16:37:39
────────────────────────────────────────────────────────────────────────────
   1 I  2024-03-01 12:00:01,100 [main] INFO  com.acme.App - Starting on port 8080
   2 E▸ 2024-03-01 12:00:02,250 [http-1] ERROR com.acme.web.Handler - Request failed +6 lines
   9 W  2024-03-01 12:00:03,000 [pool-2] WARN  com.acme.db.Pool - 9 of 10 connections in use
row 2 of 3,017 · line 2-8 · error · 2024-03-01 12:00:02.250
```

`*.log`, a rotated `*.log.1` to `*.log.99`, and `*.out` open in it however
you open them -- the explorer, the file picker, `:e`. So does a
journalctl export: `journalctl -o export > boot.export`, or `-o json`
saved as `.json` (only one that starts as journald's does; any other
JSON opens as text). Any file at all opens as a log with **Logs: open this
file as a log** (`logs.open`) from the command palette.

## Formats

Each line's level and time are read from how it's written:

| Format | For example |
| --- | --- |
| JSON lines | `{"time":"…","level":"warn",…}`, pino's and bunyan's `"level":40`, Serilog's `"@l"`, `"severity"`, ECS's `"log.level"`, journald's `PRIORITY` and `__REALTIME_TIMESTAMP` |
| logfmt | `time=… level=error msg="…"`, `lvl=`, `ts=` |
| Bracketed | `[WARN] …`, `[error]`, `<debug>`, Apache's `[core:error]` |
| Rust | env_logger's `[2024-… INFO  my_crate] …`, tracing's `2024-…Z  WARN my_crate: …`, a panic |
| Java | log4j's and logback's `2024-… [main] ERROR com.acme.App - …`, Spring's, a stack trace with `Caused by:` and `... 12 more` |
| Python | logging's `2024-… - app - WARNING - …`, `WARNING:root:…`, a traceback |
| syslog | `<34>1 2024-…` (RFC 5424), `<14>Mar  1 12:00:03 …` and journalctl's `Mar  1 12:00:03 host unit[42]: …` |
| journalctl | `-o json`, `-o export` (a row is its `SYSLOG_IDENTIFIER[_PID]: MESSAGE`), `-o short`, `-o short-iso` |
| Others | logcat, glog, .NET's `info:` and `fail:`, logrus's `ERRO[0005]`, nginx, Apache's access logs (5xx an error, 4xx a warning) |

The levels are fatal (critical, emerg, alert, panic), error (severe,
fail), warn, info (notice), debug, trace (verbose, finer) and other, for a
line that says none. A time is read from the line's start (ISO 8601 with
or without its zone, syslog's, ctime's, logcat's, a time of day alone),
from the brackets near its start (`[10/Oct/2000:13:55:36 -0700]`), or from
a JSON or logfmt field (text, or a number since 1970 in seconds,
milliseconds, microseconds or nanoseconds).

An entry is a line with a time or a level at its start, and the lines
after it that have neither. A log where few lines have one (under one in
five) goes by indentation instead: only an indented line, `Caused by:` or
`... 12 more` goes with the one before. Either way an entry takes at most
2,000 lines or 1 MiB; past that the next line starts another. A panic and
`Exception in thread` start an entry of their own. An entry whose first
line says no level takes the first its other lines say (a traceback's).

`logs.patterns` adds formats of your own: a regular expression and the
level a line matching it has, checked first.

## Keys

| Key | What it does |
| --- | --- |
| `j` `k`, Down Up | An entry down, up; through an open entry taller than the pane first |
| `J` `K`, PageDown PageUp, Ctrl+F Ctrl+B | A screen down, up |
| Ctrl+D Ctrl+U | Half a screen down, up |
| `g` `G`, Home End | The first and last entry shown |
| Enter, Tab | Open the entry, or fold it: all its lines, a JSON line pretty-printed |
| `C` | Fold every entry |
| `e` `w` `i` `d` `t` `o` | Show or hide errors (and fatal), warnings, info, debug, trace, other |
| `a` | Show every level |
| `/` | Filter: only entries with a text (in lower case, any case; with a capital, its own), or `re:` a regular expression |
| `+` | Only entries with one of these patterns (each `+` adds one) |
| `-` | No entry with any of these patterns |
| `=` | A field of a JSON or logfmt line: `user=bob`, `status!=200`, `msg~timeout`, `user.id=7`, `path=/api*` |
| `@` | A time range: `12:00..12:30`, `2024-03-01 12:00..`, `..2024-03-02`, `-15m` (the log's last 15 minutes: `s`, `m`, `h`, `d`) |
| `?` | Search: what it finds is marked, and counted |
| `n` `N` | The next entry the search found, the one before -- round the end; with no search, the next error, the one before |
| `]` `[` | The next error (or fatal) shown, the one before |
| `x` | Clear the filters and the search (the levels stay) |
| `F` | Follow the log as it grows, as `tail -f` does: the cursor stays at the end while it's there |
| `y` | Copy the entry's first line |
| `Y` | Copy the whole entry; a JSON line pretty-printed |
| `r` | The file's text, at the entry's line (`SPC f o` comes back) |
| `h` `l`, Left Right | Scroll across; `0` back |
| `R` | Read the file again from its start |
| `q`, Esc | Close it |

An empty answer to `/`, `?`, `@` clears it; to `+`, `-` or `=`, all of
them. Every filter goes with the levels shown: an entry is shown when its
level is, its text has the `/` text, one of the `+` patterns and none of
the `-` ones, its fields are so, and its time is in the range (one with
no time stays). The counts on the second line are the whole log's; the
third says how many are shown.

With the mouse: a click picks a row, a double click opens it, a click on
the bars goes to the first entry at that time, and the wheel scrolls.

The **bars** count the entries over the log's time, a column each few
minutes (as the pane's width and the log's span make it): red where the
errors are -- every one while they're rare, else where there are more
than usual -- yellow for warnings. With a filter that reads the entries
(any but the levels), they count what's shown.

## Big logs

A log in the project is read where it is, a block at a time: it opens at
once, its first screen shown while the rest is indexed, a slice of the
file each few milliseconds, the page answering and drawn again as it
goes. What's kept is small -- where each 64 KiB of entries starts, and
each entry's level in half a byte -- and an entry's text is read again
when it's shown, so a log of gigabytes takes a few MiB (a 200 MB log,
2.4 million entries: about 3.5 MiB). A filter is worked out the same way,
a slice at a time, a bit for each entry. A line longer than 64 KiB is
read only that far for its level, the filters and the search.

One outside the project is read whole by Fenix, up to the 64 MiB it gives
an extension; open a bigger one from its project.

**Following** (`F`, or `logs.follow`) looks at the file's length every
second (`logs.poll`) and reads what was added. A log that got shorter, or
whose start is another's -- truncated, or rotated and started again under
its name -- is read again from its start. One from outside the project
is read whole again each time.

## Settings

| Setting | What it does |
| --- | --- |
| `logs.levels` | The levels a log opens with shown: `error, warn` (error takes fatal with it), `info`, `debug`, `trace`, `other`. All of them when empty |
| `logs.follow` | Follow a log from when it opens. Off unless set |
| `logs.patterns` | Formats of your own: a regular expression (`^!!`, `\| CRIT \|`) and its level (fatal, error, warn, info, debug, trace, other) |
| `logs.poll` | How often a followed log is looked at, in milliseconds: 200 to 60,000. 1,000 unless set |

## For coding agents

Two tools for your coding agents (Fenix 1.1), in the `logs` toolset,
both class `read`: they read a log and change nothing.

| Tool | What it answers |
| --- | --- |
| `logs_query` | A log's entries, as JSON, the viewer's way: each one's level, time, byte offset, its lines (a stack trace with the entry it follows) and its text. Filtered by `level` (this level or worse), `time` (`12:00..12:30`, `-15m`, as `@` takes it), `text` and `exclude` (as `/` and `-`: lower case any case, `re:` a regular expression) and `fields` (as `=`: `user=bob`, `status!=200`) |
| `logs_summary` | A log at a glance: its size, the formats its lines are written in (from its first thousand entries), its entries by level, its first and last times, and, in up to `buckets` spans of time (24; at most 100), how many entries, warnings and errors came |

**Which file.** `path` is a file's path from the project's root
(`logs/app.log`); left out, the one log of the project open in the viewer.
Only the project's own files are read: an absolute path, a `..`, a link
anywhere on the path (to a file or a folder) and a folder are refused,
and with no project open there's nothing to read. A log you opened from
outside the project isn't reached.

**How much.** `logs_query` gives the newest entries first (`order:
"oldest"` the other way), at most `limit` of them (50; at most 500) and
`max_bytes` of their text (16 KiB; at most 256 KiB), an entry's text cut
at 4 KiB (`"cut": true`). When there's more, `truncated` says what
stopped it -- `limit`, `bytes`, or `scan` -- and `next_cursor`, given back
as `cursor` with the same order, reads on from there. Oldest first, each
entry has its `line`, from 1; newest first, the file is read back from its
end and lines aren't counted.

**Big logs.** A call reads at most 128 MiB of the file: what's past that
is `truncated: "scan"`, and the cursor goes on from it. It's read in steps
of a few hundred KiB on the viewer's timer, as the page indexes a log, so
the viewer answers meanwhile; a small log is answered at once. The summary
of a log over 128 MiB is of its newest 128 MiB, and says so. A log
truncated or rotated between two calls makes their cursor point at
another file's bytes: start again without one.

**Logs hold secrets.** Tokens in URLs, passwords in a connection string,
an `Authorization` header logged by mistake, people's addresses: what a
tool answers goes to your agent, and to whoever runs the model behind it.
The `logs` toolset isn't offered to agents until you turn it on; point
them at logs you'd show them, and Fenix's agent rules can make them ask
first, or refuse, by toolset, tool or class.

## What it may do

- Read the project's files (`filesystem = "project-read"`): a log in the
  project, where it is, as far as each step needs -- and, for your
  coding agents' tools, the log they name in it.
- Read where you are (`editor = "read"`): which file `logs.open` shows,
  and where the project is.

## Building

```text
cargo xtask pack extensions/logs dist
cargo test --manifest-path extensions/logs/Cargo.toml
cargo run --release --manifest-path extensions/logs/Cargo.toml --example big -- /tmp/logs 200
```

`cargo test` checks each format's levels and times against samples
(`src/samples`), entries and stack traces, the index built in steps and
grown, truncated and rotated, the filters, the search, the bars -- and
the agents' tools: the paths they refuse (`..`, absolute ones, links),
the entries they find against the viewer's, their filters, limits, byte
cap and cursors, newest first and oldest, a log read back a window at a
time, the most a call reads, and the summary's counts and formats. The
`big` example writes a big log and times indexing and filtering it.
