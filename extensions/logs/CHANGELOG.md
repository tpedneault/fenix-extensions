# Changelog

## 1.0.0

- The first release, for Fenix 1.0: log files read in a pane -- each entry's level and time from the formats logs are written in (JSON lines, logfmt, bracketed levels, env_logger and tracing, log4j and logback, Python's logging, syslog, journalctl's text, JSON and export, logcat, glog, .NET, logrus, Apache and nginx, and patterns of your own), stack traces folded into the entry they follow and opened with Enter, a JSON line pretty-printed, filtered by level, text or regular expression, patterns in and out, a field's value and a time range, searched, counted over time in a row of bars, followed as they grow (truncated and rotated logs read again), and read a block at a time, indexed and filtered in steps, so a log of gigabytes opens at once.
