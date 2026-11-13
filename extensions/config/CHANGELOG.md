# Changelog

## 1.1.0

- For Fenix 1.5 (extension API 1.5): how installing Taplo, Marksman,
  shfmt, ShellCheck or hadolint ended is a notification rather than a
  line in the message line, since the download can take a while and you
  may be elsewhere -- "Installed Taplo 0.10.0", and what it's now used as;
  or "Couldn't install Taplo 0.10.0", saying whether its download,
  unpacking it or setting it up failed and why, with *Try again*. The
  newest for a tool takes the last one's place, and a failure's is taken
  away when you try again. Starting an install is still said in the
  message line. The language servers and Prettier, installed by `npm`
  as a task, are as they were. The README gives standard keys beside
  Vim's.

## 1.0.0

- The first release, for Fenix 1.0: yAML, JSON, TOML, Dockerfiles, shell scripts and Markdown: their language servers, SchemaStore's schemas for GitHub Actions, Compose, package.json, tsconfig and the rest, Taplo, shfmt and Prettier formatting, ShellCheck and hadolint, the project's checks with their fixes, and snippets.
