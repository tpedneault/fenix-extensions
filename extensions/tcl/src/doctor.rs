//! What the project doctor says of a Tcl project: whether Tcl is there to
//! run its scripts, and Universal Ctags for its definitions.

use std::path::Path;

/// How a check came out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    Ok,
    Info,
    Warn,
}

/// One line on the doctor's page, under Toolchain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub label: &'static str,
    pub health: Health,
    pub detail: String,
}

/// Fenix's MIB tables: a folder holding one is a MIB project.
const MIB_TABLES: &[&str] = &["vdf.dat", "ccf.dat", "pcf.dat", "pid.dat", "tpcf.dat", "caf.dat", "txf.dat"];

/// What makes a folder another kind of project, as Fenix tells them
/// apart: these come before Tcl.
const OTHER_MARKERS: &[&str] = &["sketch.yaml", "library.properties", "Cargo.toml", "pyproject.toml", "setup.py", "requirements.txt", "uv.lock", "go.mod", "package.json", "CMakeLists.txt"];

/// Whether the project at `root` is a Tcl project, as Fenix decides a
/// project's kind: what `.fenix/project.toml` (or 1.x's `project.ini`)
/// declares, else its files -- a `pkgIndex.tcl` or a `.tcl` file at its
/// top, with nothing there that makes it another kind first.
pub fn is_tcl_project(root: &Path) -> bool {
    let declared = std::fs::read_to_string(root.join(".fenix/project.toml")).ok().and_then(|t| value(&t, "project", "kind")).or_else(|| std::fs::read_to_string(root.join(".fenix/project.ini")).ok().and_then(|t| value(&t, "project", "kind")));
    if let Some(kind) = declared {
        return kind.eq_ignore_ascii_case("tcl");
    }
    let names: Vec<String> = std::fs::read_dir(root).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    let has = |name: &str| names.iter().any(|n| n == name);
    let with = |ext: &str| names.iter().any(|n| Path::new(n).extension().is_some_and(|e| e.eq_ignore_ascii_case(ext)));
    // A sketch is `<folder>.ino`; inside the sandbox the folder is
    // `/project`, so any `.ino` at the top does.
    let sketch = with("ino") || with("pde");
    let mib = MIB_TABLES.iter().any(|t| has(t) || root.join("mib").join(t).is_file());
    if sketch || mib || OTHER_MARKERS.iter().any(|m| has(m)) {
        return false;
    }
    has("pkgIndex.tcl") || with("tcl")
}

/// `key`'s value in `[section]` of a TOML or INI text, quotes taken off.
fn value(text: &str, section: &str, key: &str) -> Option<String> {
    let mut inside = false;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        if let Some(header) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            inside = header.trim() == section;
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        if inside && k.trim() == key {
            let v = v.split('#').next().unwrap_or_default().trim().trim_matches(['"', '\'']);
            return (!v.is_empty()).then(|| v.to_string());
        }
    }
    None
}

/// The doctor's checks for a Tcl project, with where `tclsh` and `ctags`
/// were found.
pub fn checks(tclsh: Option<&str>, ctags: Option<&str>) -> Vec<Check> {
    vec![
        match tclsh {
            Some(path) => Check { label: "tclsh", health: Health::Ok, detail: path.to_string() },
            None => Check { label: "tclsh", health: Health::Warn, detail: "not found -- install Tcl to run scripts".into() },
        },
        match ctags {
            Some(path) => Check { label: "ctags", health: Health::Ok, detail: path.to_string() },
            None => Check { label: "ctags", health: Health::Info, detail: "not found -- completion of the project's procs, gd and SPC m s need Universal Ctags".into() },
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for file in files {
            let path = dir.path().join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "").unwrap();
        }
        dir
    }

    #[test]
    fn a_folder_of_tcl_scripts_is_a_tcl_project() {
        assert!(is_tcl_project(project(&["main.tcl"]).path()));
        assert!(is_tcl_project(project(&["pkgIndex.tcl", "lib/a.tm"]).path()));
        assert!(!is_tcl_project(project(&["notes.txt", "lib/a.tcl"]).path()), "only what's at its top");
        assert!(!is_tcl_project(project(&["Cargo.toml", "build.tcl"]).path()), "a Rust project with a script in it");
        assert!(!is_tcl_project(project(&["ccf.dat", "procedure.tcl"]).path()), "a MIB");
        assert!(!is_tcl_project(project(&["mib/pcf.dat", "procedure.tcl"]).path()), "a MIB, its tables in mib/");
        assert!(!is_tcl_project(project(&["Blink.ino", "upload.tcl"]).path()), "an Arduino sketch");
    }

    #[test]
    fn what_a_project_declares_wins() {
        let dir = project(&["Cargo.toml"]);
        std::fs::create_dir_all(dir.path().join(".fenix")).unwrap();
        std::fs::write(dir.path().join(".fenix/project.toml"), "[project]\nkind = \"tcl\" # it is\n").unwrap();
        assert!(is_tcl_project(dir.path()));
        let dir = project(&["main.tcl"]);
        std::fs::create_dir_all(dir.path().join(".fenix")).unwrap();
        std::fs::write(dir.path().join(".fenix/project.ini"), "[project]\nkind = python\n").unwrap();
        assert!(!is_tcl_project(dir.path()));
    }

    #[test]
    fn tcl_missing_warns_and_ctags_missing_informs() {
        let found = checks(Some("/usr/bin/tclsh"), Some("/usr/bin/ctags"));
        assert!(found.iter().all(|c| c.health == Health::Ok));
        assert_eq!(found[0].detail, "/usr/bin/tclsh");
        let missing = checks(None, None);
        assert_eq!(missing[0], Check { label: "tclsh", health: Health::Warn, detail: "not found -- install Tcl to run scripts".into() });
        assert_eq!(missing[1].health, Health::Info);
    }
}
