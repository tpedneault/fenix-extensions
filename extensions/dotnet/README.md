# C# and .NET

C# and .NET in Fenix: csharp-ls as the language server, debugging with
netcoredbg, the build's warnings and errors on save where there's no
csharp-ls, CSharpier as the
formatter, the solution's tasks, the project doctor's checks with their
fixes, outdated and vulnerable NuGet packages, snippets, and `dotnet
new`'s projects in the new-project wizard. Built on lang-kit
(`crates/lang-kit`), as each language pack is.

It reads the solution at the project's top (`.sln` or `.slnx`) and its
projects; without one, the project files three folders down. Each
project's SDK, output type, frameworks, assembly name, packages and
where its build goes come from its file and the `Directory.Build.props`
above it (`OutputPath`, `BaseOutputPath`, `UseArtifactsOutput`,
`RuntimeIdentifier`, when they're plain paths).

## The language server

`csharp-ls`, given the solution (`--solution`), when it's installed
(`~/.dotnet/tools` is looked in). Roslyn's own server isn't used: it
isn't on nuget.org, talks over a named pipe, and loads a solution only
when its client asks in a message of its own. The doctor's fix row
installs csharp-ls with `dotnet tool update -g`, at the newest version
the SDK can run: 0.16.0 on .NET 8, 0.20.0 on .NET 9, the latest on 10.

csharp-ls gives its problems only when asked for them (LSP's pull
diagnostics), which Fenix does as a file opens and changes.

## Problems on save

Where csharp-ls isn't found, or with `dotnet.lint_on_save` on, when a
C# file (or a project file) is saved, its project is built
(`dotnet build <project> -v:q -clp:NoSummary`) and MSBuild's warnings
and errors about that file are shown with the server's: `CS0168`,
`CS8602`, an analyzer's. A project file gets its own on its first line
(`NU1903`, a vulnerable package). `dotnet.lint_on_save` off turns it
off even then.

## Formatting

`SPC c F` -- and saving, with `editor.format_on_save` -- runs CSharpier
on the buffer (`csharpier format --write-stdout --stdin-path ${file}`):
the project's own when its tools manifest has it (`dotnet csharpier`),
else the one installed. With neither, csharp-ls formats. `dotnet
format` can't take a buffer on stdin -- it changes the files -- so it's
the `format` task instead.

## Debugging

netcoredbg (`netcoredbg --interpreter=vscode`), from the tools folder or
`PATH`; the fix row downloads Samsung's release for this machine
(Linux x64 and arm64, Intel macOS, Windows x64) into the tools folder.

`SPC u u`, in a project with no launch configuration of its own, offers
each program of the solution, its project built first (`build
<project>`, or `build` for a project alone):

| | |
|---|---|
| .NET: *project* | `dotnet` with its Debug build's dll, in its own folder, in Development with its launch profile's variables and URL, as `dotnet run` would |
| .NET: *project*, stopped at its start | the same, `stopAtEntry` |
| .NET: attach to a process | the process picked as the session starts (`${pickProcess}`) |
| .NET: the tests dotnet test holds | once *.NET: debug the tests* has them |

The dll's path comes from the project file and `Directory.Build.props`;
the doctor says whether the program you were in last is built, with a
row that builds it.

*.NET: debug the tests* (`SPC m t`) runs `dotnet test` with
`VSTEST_HOST_DEBUG=1` -- the test project you're in, else the
solution's -- which builds and then holds the test host for the
debugger; its process is then a launch of its own, which attaches to it.
Its output opens beside when it ends. A test project that's a program
(xunit.v3, TUnit, the MSTest runner) is debugged as a project.

## Tasks

`SPC p t`: `build`, `test`, `restore`, `clean` and `format` for the
solution (`tool restore` with a tools manifest), then `build <project>`
for each, `run <project>` for each program, `watch <project>` for a web
project and `test <project>` for each test project. A task of the same
name in `.fenix/tools.json` wins. Saving the solution, a project file,
`Directory.Build.props`, `global.json` or the tools manifest makes them
again.

## The project doctor

- **.NET SDK**: `dotnet --version` in the project, which `global.json`
  steers: the SDK it asks for, or, without one, one at least as new as
  the newest framework a project targets. Fix: the download page.
- **Language server**: csharp-ls. Fix: `dotnet tool update -g csharp-ls`.
- **Debugger**: netcoredbg. Fix: its release, into the tools folder.
- **CSharpier**, unless the project has its own. Fix: `dotnet tool
  update -g csharpier`.
- **Debugging**: the project to debug, built or not. Fix: build it.

## Packages

`SPC m p` (or *.NET: packages*): lang-kit's page, each package once for
each project that has it, its project as its group; those another
package brings are listed too.

| | |
|---|---|
| list | `dotnet list <solution> package --include-transitive --format json` |
| outdated | the same, `--outdated` |
| vulnerable | the same, `--vulnerable`: NuGet's severity, the GitHub advisory |
| `u` | `dotnet add <project> package <name> --version <latest>`; a transitive one is asked for, which is NuGet's way to move one up |
| `U` | the same, when there's one to do: `dotnet add package` takes one |
| `a` | `dotnet add <project> package <name>` (`name@1.2.3` for a version), to the project you're in |
| `d` | `dotnet remove <project> package <name>` |

A project not restored yet says so (`dotnet restore`, the `restore`
task).

## Snippets

`cls`, `rec`, `recs` (record struct), `iface`, `enum`, `prop`, `propi`
(init), `propr` (required), `propf` (a backing field), `ctor`, `am` (an
async method), `aw` (`ConfigureAwait(false)`), `linq` (a query), `ws`
(`Where`/`Select`), `gb` (`GroupBy`, counted), `foreach`, `usingv`, `try`,
`ex` (an exception type), `cw`, `ns` (file-scoped); xUnit's `fact`,
`afact`, `theory`, `xclass`; NUnit's `test`, `tcase`, `fixture`; `mapget`
(a minimal API endpoint).

## Project templates

Each runs `dotnet new`, with the SDK's framework or the one you pick:

- **Console app**, **Class library**, **Web API**, **Worker service**:
  a solution with the project in `src/` and, if you like, xUnit tests in
  `tests/` that reference it; a `.gitignore` and an `.editorconfig`.
- **xUnit tests**: a test project on its own.

## Files

`*.cake` is read as C#.

## Keys

After `SPC m` in a .NET project: `p` packages, `d` check again, `t`
debug the tests.
