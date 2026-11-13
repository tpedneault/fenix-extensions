# Changelog

## 1.2.0

- For Fenix 1.5 (extension API 1.5): what it says names the keys of the
  key scheme you use, and yours where you rebound them -- an env file's
  warnings ("`C-k p e m` adds them", "`C-k p e i` adds it to .gitignore")
  with standard keys, `SPC p e m` with Vim keys, and the task picker's
  keys once tasks run with a profile -- and says them again when you
  switch. On Fenix 1.0 to 1.4, 1.1.0 stays the one you have. The README
  gives standard keys beside Vim's.

## 1.1.0

- `dotenv.page` can be run from outside Fenix, over the control channel
  (`fenix/runCommand`): its page opened by a script or another tool, as
  the screenshots' scripts do.

## 1.0.0

- The first release, for Fenix 1.0: env files (.env): values hidden but on the cursor's line, every env file side by side, profiles switched with a backup kept or given to tasks, a warning when Git would commit one, keys checked against .env.example, and variable names completed after process.env., os.environ[ and the like.
