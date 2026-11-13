# Changelog

## 1.2.0

- For Fenix 1.5 (extension API 1.5): how installing netcoredbg ended is
  a notification rather than a line in the message line, since the
  download can take a while and you may be elsewhere -- "Installed
  netcoredbg", and what it's now used as; or "Couldn't install
  netcoredbg", saying whether its download, unpacking it or setting it up
  failed and why, with *Try again*. A failure's is taken away when you
  try again. Starting the install is still said in the message line.
  csharp-ls and CSharpier, installed by `dotnet tool`, are as they were.
  What it says when `dotnet test` holds its test host for the debugger
  names the keys that start a debug session as the scheme in use has them
  (`C-k u u` with standard keys, or what you or Fenix bound), where it
  said `SPC u u` whichever you use. The README gives standard keys beside
  Vim's.

## 1.1.0

- `dotnet.packages` can be run from outside Fenix, over the control channel
  (`fenix/runCommand`): its page opened by a script or another tool, as
  the screenshots' scripts do.

## 1.0.0

- The first release, for Fenix 1.0: c# and .NET: csharp-ls and its problems, debugging with netcoredbg -- each program built first, a process picked to attach to --, CSharpier, the solution's tasks, the project's checks with their fixes, outdated and vulnerable NuGet packages, snippets, and dotnet new's projects.
