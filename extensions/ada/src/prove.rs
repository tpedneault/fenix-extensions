//! GNATprove's findings as problems (`SPC m p`, `ada.prove`): the proof
//! run as the `prove` task runs it, one message a line
//! (`--output=oneline`; GNATprove 16's own is several lines a message),
//! going on past an error (`-k`). A line is `file:line:col: severity:
//! message`, the file by its name alone, which is found among the
//! project's sources. A check not proved is `high`, `medium` or `low`;
//! flow analysis and the compiler say `error`, `warning` and `info`. It's
//! a linter of lang-kit's that speaks of several files and runs only when
//! asked: it's for no file saved, since a proof takes minutes.

use fenix_extension_api::{Command, Problem, Severity};
use fenix_lang_kit::lint::{from_one, Files};
use fenix_lang_kit::{Linter, Output};

use crate::project::Project;

/// The linter's name, and the problems' source.
pub const NAME: &str = "gnatprove";

/// A finding's severity, as GNATprove says it.
pub fn severity(word: &str) -> Option<Severity> {
    Some(match word {
        "error" | "high" => Severity::Error,
        "medium" | "warning" => Severity::Warning,
        "low" | "info" => Severity::Info,
        _ => return None,
    })
}

/// What a run found, file by file, each file placed by its name among
/// `project`'s sources. One that names no place, and failed (no project
/// file, no GNATprove), is an error, so what was shown stays.
pub fn parse(out: &Output, project: &Project) -> Result<Files, String> {
    let mut files: Files = Vec::new();
    for line in out.stdout.lines().chain(out.stderr.lines()) {
        let mut parts = line.splitn(5, ':');
        let (Some(file), Some(l), Some(c), Some(word), Some(message)) = (parts.next(), parts.next(), parts.next(), parts.next(), parts.next()) else { continue };
        let (Ok(l), Ok(c), Some(sev)) = (l.parse::<u32>(), c.parse::<u32>(), severity(word.trim())) else { continue };
        if file.is_empty() || file.contains(' ') {
            continue;
        }
        let at = from_one(l, c);
        let problem = Problem::new(sev, at, (at.0, at.1 + 1), message.trim().to_string()).source(NAME);
        let path = project.source(file).unwrap_or(file).to_string();
        let p = problem;
        match files.iter_mut().find(|(f, _)| *f == path) {
            Some((_, ps)) => ps.push(p),
            None => files.push((path, vec![p])),
        }
    }
    if files.is_empty() && !out.ok() {
        return Err(out.why());
    }
    Ok(files)
}

/// The proof, `prove` (the task's command), for findings: one a line,
/// past the first error.
pub fn linter(prove: Command, project: Project) -> Linter {
    let cmd = prove.args(["--output=oneline", "-k"]);
    Linter::across(NAME, &[], move |_| cmd.clone(), move |out, _| parse(out, &project))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> Project {
        Project { sources: vec!["src/altitudes.ads".into(), "src/altitudes.adb".into(), "src/beacons.ads".into(), "src/beacons.adb".into()], ..Project::default() }
    }

    #[test]
    fn gnatproves_findings_are_problems_by_file() {
        let out = Output { code: Some(1), stdout: include_str!("samples/gnatprove.txt").into(), stderr: String::new() };
        let files = parse(&out, &project()).unwrap();
        let names: Vec<&str> = files.iter().map(|(f, _)| f.as_str()).collect();
        assert_eq!(names, ["src/altitudes.adb", "src/beacons.adb", "src/beacons.ads"], "placed among the sources");
        let rows = |f: usize| files[f].1.iter().map(|p| (p.severity, p.start, p.message.clone())).collect::<Vec<_>>();
        let altitudes = rows(0);
        assert_eq!(altitudes.len(), 3);
        assert_eq!((altitudes[0].0, altitudes[0].1), (Severity::Warning, (5, 13)));
        assert!(altitudes[0].2.starts_with("range check might fail, cannot prove upper bound for H + By [reason for check:"), "{}", altitudes[0].2);
        assert_eq!(rows(1), [(Severity::Error, (3, 36), "\"Count\" must be listed in the Global aspect of \"Next\" (SPARK RM 6.1.4(15))".to_string())]);
        let spec: Vec<(Severity, (u32, u32))> = rows(2).into_iter().map(|(s, at, _)| (s, at)).collect();
        assert_eq!(spec, [(Severity::Error, (6, 10)), (Severity::Warning, (8, 19)), (Severity::Error, (8, 19))], "error, warning, high");
        assert!(files.iter().flat_map(|(_, ps)| ps).all(|p| p.source.as_deref() == Some(NAME)));
    }

    #[test]
    fn a_run_that_found_nothing_or_failed() {
        let clean = Output { code: Some(0), stdout: "Phase 1 of 3: generation of data representation information ...\nSummary logged in obj/gnatprove/gnatprove.out\n".into(), stderr: String::new() };
        assert_eq!(parse(&clean, &project()).unwrap(), Vec::new());
        let failed = Output { code: Some(4), stdout: String::new(), stderr: "gnatprove: project file \"gone.gpr\" not found\n".into() };
        assert!(parse(&failed, &project()).is_err());
        // A file not among the sources keeps its name.
        let other = Output { code: Some(0), stdout: "gen.adb:2:4: low: assertion might fail\n".into(), stderr: String::new() };
        assert_eq!(parse(&other, &project()).unwrap()[0].0, "gen.adb");
    }

    #[test]
    fn the_proof_runs_for_no_saved_file() {
        let l = linter(Command::new("gnatprove").args(["-P", "orbit.gpr", "--level=2", "-j0"]), project());
        assert_eq!(crate::tests::line(&(l.command)("orbit.gpr")), "gnatprove -P orbit.gpr --level=2 -j0 --output=oneline -k");
        assert!(!l.takes("src/altitudes.adb") && !l.takes("orbit.gpr"));
    }
}
