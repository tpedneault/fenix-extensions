# Changelog

## 1.1.0

For Fenix 1.1 (extension API 1.2): tools for your coding agents, which
read a log in the project as the viewer does. Fenix 1.0 keeps Log viewer
1.0.0.

- `logs_query` (read): a log's entries as JSON -- level, time, offset,
  lines and text, a stack trace with its entry -- filtered by level (this
  one or worse), time range, text or regular expression, a text left
  out, and fields, newest first or oldest, at most 500 entries and 256
  KiB of their text, with a cursor to read on.
- `logs_summary` (read): its size, the formats its lines are in, its
  entries by level, its first and last times, and counts over time.
- Only a file in the project, named from its root: no `..`, no absolute
  path, no link on the way. A big log is read in steps, at most 128 MiB
  a call. Logs can hold secrets: the README says what to think of.

## 1.0.0

- The first release, for Fenix 1.0: log files read in a pane -- each entry's level and time from the formats logs are written in (JSON lines, logfmt, bracketed levels, env_logger and tracing, log4j and logback, Python's logging, syslog, journalctl's text, JSON and export, logcat, glog, .NET, logrus, Apache and nginx, and patterns of your own), stack traces folded into the entry they follow and opened with Enter, a JSON line pretty-printed, filtered by level, text or regular expression, patterns in and out, a field's value and a time range, searched, counted over time in a row of bars, followed as they grow (truncated and rotated logs read again), and read a block at a time, indexed and filtered in steps, so a log of gigabytes opens at once.
