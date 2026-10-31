//! The build as the linter run on save: `dotnet build` of the saved
//! file's project, its warnings and errors (MSBuild's `path(line,col):
//! warning CS0168: ...`) shown as the file's problems. Fenix doesn't ask a
//! server for its diagnostics (csharp-ls gives them only when asked), so
//! this is where a C# file's problems come from.
//!
//! A project file's own (`App.csproj : warning NU1903: ...`, a vulnerable
//! package) go on its first line.

use fenix_extension_api::{Command, Problem, Severity};
use fenix_lang_kit::lint::severity;
use fenix_lang_kit::{compiler_lines, Linter, Output};

use crate::project::Solution;

/// The build's command for project `project`: quiet, only its problems,
/// each once, without the terminal logger's redrawing.
pub fn build(dotnet: &Command, project: &str) -> Command {
    let mut cmd = dotnet.clone().args(["build", project, "-nologo", "-v:q", "-clp:NoSummary"]);
    cmd.env.push(("MSBUILDTERMINALLOGGER".into(), "off".into()));
    cmd.env.push(("DOTNET_NOLOGO".into(), "1".into()));
    cmd
}

/// What the build said about `file`: MSBuild's lines, each once (a
/// project built for two frameworks says each twice), the project it was
/// for left off the end.
pub fn parse(out: &Output, file: &str) -> Result<Vec<Problem>, String> {
    let text = out.text();
    let mut problems: Vec<Problem> = Vec::new();
    if file.rsplit_once('.').is_some_and(|(_, x)| crate::project::PROJECT_EXTENSIONS.iter().any(|p| p.eq_ignore_ascii_case(x))) {
        problems = project_lines(&text, file);
    } else {
        for mut p in compiler_lines(&text, file, Severity::Warning) {
            if let Some(at) = p.message.rfind(" [") {
                if p.message.ends_with(']') {
                    p.message.truncate(at);
                }
            }
            if !problems.iter().any(|q| q.start == p.start && q.code == p.code && q.message == p.message) {
                problems.push(p);
            }
        }
    }
    if problems.is_empty() && !out.ok() && !text.contains(": error ") {
        // It failed without a word about any file: the SDK's own trouble.
        return Err(out.why());
    }
    Ok(problems)
}

/// `path.csproj : warning NU1903: message` lines about project `file`.
fn project_lines(text: &str, file: &str) -> Vec<Problem> {
    let file = file.replace('\\', "/");
    let mut out: Vec<Problem> = Vec::new();
    for line in text.lines() {
        let Some((path, rest)) = line.split_once(" : ") else { continue };
        if !path.replace('\\', "/").ends_with(&file) {
            continue;
        }
        let Some((head, message)) = rest.split_once(": ") else { continue };
        let mut words = head.split_whitespace();
        let Some(sev) = words.next().and_then(severity) else { continue };
        let mut p = Problem::new(sev, (0, 0), (0, 1), message.trim().to_string());
        p.code = words.next().map(str::to_string);
        if !out.iter().any(|q| q.code == p.code && q.message == p.message) {
            out.push(p);
        }
    }
    out
}

/// The linter: a saved C# file (or project file) built with its project.
/// A file in no project of the solution isn't built.
pub fn linter(dotnet: &Command, solution: &Solution) -> Linter {
    let dotnet = dotnet.clone();
    let projects: Vec<(String, String)> = solution.projects.iter().map(|p| (p.path.clone(), p.dir().to_string())).collect();
    let of = move |file: &str| -> Option<String> {
        projects.iter().filter(|(path, dir)| path == file || dir.is_empty() || file.starts_with(&format!("{dir}/"))).max_by_key(|(_, d)| d.len()).map(|(p, _)| p.clone())
    };
    Linter::new(
        "dotnet build",
        &["cs", "csproj", "fs", "fsproj", "vb", "vbproj"],
        move |file| match of(file) {
            Some(project) => build(&dotnet, &project),
            // Nothing to build it with: a build of the folder, which
            // says so.
            None => build(&dotnet, "."),
        },
        parse,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::line;

    const WARN: &str = include_str!("samples/build-warn.txt");
    const ERROR: &str = include_str!("samples/build-error.txt");

    fn out(code: i32, text: &str) -> Output {
        Output { code: Some(code), stdout: text.into(), stderr: String::new() }
    }

    #[test]
    fn the_builds_warnings_are_the_files_problems() {
        let p = parse(&out(0, WARN), "src/Orbit.Core/Class1.cs").unwrap();
        let rows: Vec<_> = p.iter().map(|p| (p.start, p.severity, p.code.as_deref(), p.message.as_str())).collect();
        assert_eq!(
            rows,
            [
                ((6, 12), Severity::Warning, Some("CS0168"), "The variable 'unused' is declared but never used"),
                ((11, 45), Severity::Warning, Some("CS8602"), "Dereference of a possibly null reference."),
            ]
        );
        assert_eq!(parse(&out(0, WARN), "src/Orbit.App/Program.cs").unwrap(), []);
        let e = parse(&out(1, ERROR), "src/Orbit.App/Program.cs").unwrap();
        assert_eq!((e[0].start, e[0].severity, e[0].code.as_deref(), e[0].message.as_str()), ((3, 34), Severity::Error, Some("CS1003"), "Syntax error, ',' expected"));
    }

    #[test]
    fn a_project_files_own_go_on_its_first_line_once() {
        let p = parse(&out(0, WARN), "src/Orbit.App/Orbit.App.csproj").unwrap();
        assert_eq!(p.len(), 1, "said twice, shown once");
        assert_eq!((p[0].start, p[0].code.as_deref()), ((0, 0), Some("NU1903")));
        assert_eq!(p[0].message, "Package 'Newtonsoft.Json' 12.0.1 has a known high severity vulnerability, https://github.com/advisories/GHSA-5crp-9r3c-p9vr");
        // A build that failed and says nothing of a file.
        let sdk = out(145, "A compatible .NET SDK was not found.\n\nRequested SDK version: 9.0.100");
        assert_eq!(parse(&sdk, "Program.cs").unwrap_err(), "Requested SDK version: 9.0.100");
    }

    #[test]
    fn a_saved_file_builds_its_project() {
        let dir = crate::project::tests::orbit("lint");
        let s = Solution::read(&dir.to_string_lossy(), None);
        let l = linter(&Command::new("dotnet"), &s);
        assert_eq!(line(&(l.command)("src/Orbit.Api/Controllers/Weather.cs")), "dotnet build src/Orbit.Api/Orbit.Api.csproj -nologo -v:q -clp:NoSummary");
        assert_eq!(line(&(l.command)("src/Orbit.App/Orbit.App.csproj")), "dotnet build src/Orbit.App/Orbit.App.csproj -nologo -v:q -clp:NoSummary");
        assert_eq!((l.command)("x.cs").env[0], ("MSBUILDTERMINALLOGGER".to_string(), "off".to_string()));
        assert!(l.takes("Program.cs") && l.takes("App.csproj") && !l.takes("appsettings.json"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
