//! What the pack knows of the project, from its files: whether it's an
//! Alire crate (`alire.toml`: its name, its executables, and whether it
//! depends on GNATprove), the GNAT project files (`.gpr`) at its top and
//! the mains the chosen one lists, its Ada sources, and whether it uses
//! SPARK (`SPARK_Mode` in a source, or GNATprove a dependency).

use std::path::Path;

/// How many Ada sources are listed, how deep, and how many of them, and
/// how much of each, are read to see whether it's SPARK.
pub const SOURCES: usize = 2000;
const DEPTH: usize = 6;
const READ_FILES: usize = 300;
const READ_BYTES: usize = 256 << 10;

/// Folders that hold what was built or fetched, not the project's own
/// sources: GNAT's objects and executables, Alire's dependencies.
const SKIP: &[&str] = &["obj", "bin", "lib", "alire", "target", "node_modules"];

/// `alire.toml`, as far as the pack reads it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Crate {
    pub name: Option<String>,
    pub version: Option<String>,
    pub executables: Vec<String>,
    /// GNATprove is one of its dependencies (`alr with gnatprove`): it's
    /// in the crate's environment, `alr exec`'s.
    pub gnatprove: bool,
}

/// A string's value: `"orbit"` without its quotes.
fn unquote(value: &str) -> Option<String> {
    let v = value.trim();
    v.strip_prefix('"').and_then(|v| v.strip_suffix('"')).map(str::to_string)
}

impl Crate {
    /// Its top-level `name`, `version` and `executables`, and the
    /// dependencies in each `[[depends-on]]`. Comments are left out; an
    /// array may go over several lines.
    pub fn parse(text: &str) -> Crate {
        let mut c = Crate::default();
        let mut table: Option<String> = None;
        let mut array: Option<String> = None;
        for line in text.lines() {
            let code = line.split('#').next().unwrap_or_default().trim();
            if let Some(open) = array.as_mut() {
                open.push_str(code);
                if code.contains(']') {
                    c.executables = list(open);
                    array = None;
                }
                continue;
            }
            if code.starts_with('[') {
                table = Some(code.trim_matches(['[', ']']).trim().to_string());
                continue;
            }
            let Some((key, value)) = code.split_once('=') else { continue };
            let key = key.trim().trim_matches('"');
            match table.as_deref() {
                None => match key {
                    "name" => c.name = unquote(value),
                    "version" => c.version = unquote(value),
                    "executables" if value.contains(']') => c.executables = list(value),
                    "executables" => array = Some(value.to_string()),
                    _ => {}
                },
                Some("depends-on") if key == "gnatprove" => c.gnatprove = true,
                _ => {}
            }
        }
        c
    }

    /// The project file Alire builds it with: `<name>.gpr`.
    pub fn gpr(&self) -> Option<String> {
        self.name.as_ref().map(|n| format!("{n}.gpr"))
    }
}

/// The quoted strings in `["a", "b"]`.
fn list(text: &str) -> Vec<String> {
    text.split('"').skip(1).step_by(2).map(str::to_string).collect()
}

/// The mains a project file lists: `for Main use ("orbit.adb", ...);`,
/// in any case, over as many lines as it takes.
pub fn mains(gpr: &str) -> Vec<String> {
    let code: Vec<&str> = gpr.lines().map(|l| l.split("--").next().unwrap_or_default()).collect();
    let code = code.join(" ");
    let lower = code.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = lower[from..].find("for main use") {
        let start = from + at + "for main use".len();
        let end = lower[start..].find(';').map_or(lower.len(), |e| start + e);
        out.extend(list(&code[start..end]));
        from = end;
    }
    out
}

/// What the project's files say.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Project {
    /// `alire.toml` at the top: an Alire crate, which `alr` builds.
    pub alire: Option<Crate>,
    /// The project files at the top, by name, sorted.
    pub gprs: Vec<String>,
    /// The Ada sources, by their paths in the project.
    pub sources: Vec<String>,
    /// `SPARK_Mode` in a source, or GNATprove a dependency.
    pub spark: bool,
}

/// The Ada sources under `dir` (`rel` in the project), the built and
/// fetched and hidden folders left out.
fn walk(dir: &Path, rel: &str, depth: usize, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = entries.flatten().map(|e| (e.file_name().to_string_lossy().into_owned(), e.path())).collect();
    entries.sort();
    for (name, path) in entries {
        if out.len() >= SOURCES {
            return;
        }
        let here = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
        if path.is_dir() {
            if depth > 0 && !name.starts_with('.') && !SKIP.contains(&name.as_str()) {
                walk(&path, &here, depth - 1, out);
            }
        } else if is_ada(&name) {
            out.push(here);
        }
    }
}

/// Whether `name` is an Ada source: GNAT's `.ads` and `.adb`, or `.ada`.
pub fn is_ada(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| ["ads", "adb", "ada"].iter().any(|e| ext.eq_ignore_ascii_case(e)))
}

impl Project {
    pub fn read(root: &str) -> Project {
        let root = Path::new(root);
        let alire = std::fs::read_to_string(root.join("alire.toml")).ok().map(|t| Crate::parse(&t));
        let mut gprs: Vec<String> = std::fs::read_dir(root).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.to_ascii_lowercase().ends_with(".gpr") && root.join(n).is_file()).collect();
        gprs.sort();
        let mut sources = Vec::new();
        walk(root, "", DEPTH, &mut sources);
        let spark = alire.as_ref().is_some_and(|c| c.gnatprove)
            || sources.iter().take(READ_FILES).any(|s| {
                let text = std::fs::read(root.join(s)).map(|b| String::from_utf8_lossy(&b[..b.len().min(READ_BYTES)]).to_ascii_lowercase()).unwrap_or_default();
                text.lines().any(|l| l.split("--").next().unwrap_or_default().contains("spark_mode"))
            });
        Project { alire, gprs, sources, spark }
    }

    /// Whether it's an Ada project at all: a crate, a project file, or an
    /// Ada source.
    pub fn is_ada(&self) -> bool {
        self.alire.is_some() || !self.gprs.is_empty() || !self.sources.is_empty()
    }

    /// The project file the server and the tasks use: the one `chosen`
    /// (`ada.projectFile`), else the crate's, else the only one at the
    /// top. `Err` when it's not to be had: the one chosen isn't there, or
    /// there are several and none is chosen.
    pub fn gpr(&self, chosen: &str, root: &str) -> Result<Option<String>, String> {
        let chosen = chosen.trim().replace('\\', "/");
        if !chosen.is_empty() {
            return if Path::new(root).join(&chosen).is_file() { Ok(Some(chosen)) } else { Err(format!("{chosen}, which ada.projectFile names, isn't there")) };
        }
        if let Some(gpr) = self.alire.as_ref().and_then(Crate::gpr).filter(|g| self.gprs.contains(g)) {
            return Ok(Some(gpr));
        }
        match self.gprs.as_slice() {
            [] => Ok(None),
            [one] => Ok(Some(one.clone())),
            several => Err(format!("{}: choose one with ada.projectFile", several.join(", "))),
        }
    }

    /// A source by its file name, as GNAT's tools name it
    /// (`altitudes.adb`): its path in the project, when one has that name.
    pub fn source(&self, name: &str) -> Option<&str> {
        self.sources.iter().find(|s| s.rsplit('/').next() == Some(name)).map(String::as_str)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A fresh folder for a test.
    pub fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("fenix-ada-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn alire_toml_says_its_name_executables_and_gnatprove() {
        let c = Crate::parse(include_str!("samples/alire.toml"));
        assert_eq!(c, Crate { name: Some("orbit".into()), version: Some("0.1.0-dev".into()), executables: vec!["orbit".into()], gnatprove: false });
        assert_eq!(c.gpr().as_deref(), Some("orbit.gpr"));
        let proved = Crate::parse("name = \"ground\" # the station\nexecutables = [\n  \"ground\",\n  \"replay\",\n]\n\n[[depends-on]]\naunit = \"^26.0.0\"\n\n[[depends-on]]\ngnatprove = \"^16.1.0\"\n\n[build-switches]\nname = \"not this\"\n");
        assert_eq!((proved.name.as_deref(), proved.executables.as_slice(), proved.gnatprove), (Some("ground"), &["ground".to_string(), "replay".to_string()][..], true));
        assert_eq!(Crate::parse(""), Crate::default());
    }

    #[test]
    fn a_project_file_lists_its_mains() {
        assert_eq!(mains(include_str!("samples/orbit.gpr")), ["orbit.adb"]);
        assert_eq!(mains("project Ground is\n   FOR Main USE (\"ground.adb\", -- the station\n      \"replay.adb\");\nend Ground;\n"), ["ground.adb", "replay.adb"]);
        assert!(mains("project Lib is\n   --  for Main use (\"x.adb\");\nend Lib;\n").is_empty(), "a comment");
    }

    #[test]
    fn the_project_is_read_from_its_files() {
        let dir = temp("read");
        std::fs::create_dir_all(dir.join("src/nested")).unwrap();
        std::fs::create_dir_all(dir.join("obj/development")).unwrap();
        std::fs::create_dir_all(dir.join("alire/cache")).unwrap();
        std::fs::write(dir.join("src/orbit.adb"), "procedure Orbit is\nbegin\n   null;\nend Orbit;\n").unwrap();
        std::fs::write(dir.join("src/nested/altitudes.ads"), "package Altitudes is\n   --  SPARK_Mode, but in a comment\nend Altitudes;\n").unwrap();
        std::fs::write(dir.join("obj/development/b__orbit.adb"), "").unwrap();
        std::fs::write(dir.join("alire/cache/dep.ads"), "").unwrap();
        let root = dir.to_string_lossy().into_owned();
        let p = Project::read(&root);
        assert_eq!(p.sources, ["src/nested/altitudes.ads", "src/orbit.adb"], "objects and dependencies left out");
        assert!(p.is_ada() && !p.spark && p.alire.is_none());
        assert_eq!(p.gpr("", &root), Ok(None));
        assert_eq!(p.source("altitudes.ads"), Some("src/nested/altitudes.ads"));

        std::fs::write(dir.join("src/nested/altitudes.ads"), "package Altitudes\n  with SPARK_Mode\nis\nend Altitudes;\n").unwrap();
        std::fs::write(dir.join("orbit.gpr"), include_str!("samples/orbit.gpr")).unwrap();
        std::fs::write(dir.join("tools.gpr"), "project Tools is\nend Tools;\n").unwrap();
        let p = Project::read(&root);
        assert!(p.spark);
        assert_eq!(p.gprs, ["orbit.gpr", "tools.gpr"]);
        assert_eq!(p.gpr("", &root), Err("orbit.gpr, tools.gpr: choose one with ada.projectFile".into()));
        assert_eq!(p.gpr(" tools.gpr ", &root), Ok(Some("tools.gpr".into())));
        assert!(p.gpr("gone.gpr", &root).is_err());

        std::fs::write(dir.join("alire.toml"), include_str!("samples/alire.toml")).unwrap();
        assert_eq!(Project::read(&root).gpr("", &root), Ok(Some("orbit.gpr".into())), "the crate's");
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(!Project::read(&root).is_ada());
    }
}
