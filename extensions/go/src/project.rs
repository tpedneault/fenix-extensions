//! What the project is, as its files say: `go.mod` (its module, the Go
//! it asks for and the toolchain it suggests, its requirements and
//! which are indirect), whether there's a `go.work`, golangci-lint's
//! config, and its main packages (the top folder's, and `cmd/*`'s).

use std::path::Path;

/// A `require` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Require {
    pub path: String,
    pub version: String,
    /// `// indirect`: needed by a dependency, not the module's own code.
    pub indirect: bool,
}

/// `go.mod`, as far as the pack needs it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GoMod {
    pub module: Option<String>,
    /// The `go` line: the least Go that builds it (`1.22`, `1.23.4`).
    pub go: Option<String>,
    /// The `toolchain` line, without its `go` (`1.24.7`).
    pub toolchain: Option<String>,
    pub requires: Vec<Require>,
    /// The paths `replace` lines replace.
    pub replaced: Vec<String>,
}

/// A word of `go.mod`: a quoted one unquoted.
fn word(w: &str) -> String {
    w.trim().trim_matches(|c| c == '"' || c == '`').to_string()
}

impl GoMod {
    pub fn parse(text: &str) -> GoMod {
        let mut m = GoMod::default();
        // The verb of the block we're in: `require (`.
        let mut block: Option<String> = None;
        for raw in text.lines() {
            let (code, comment) = match raw.find("//") {
                Some(at) => (&raw[..at], raw[at + 2..].trim()),
                None => (raw, ""),
            };
            let code = code.trim();
            if code.is_empty() {
                continue;
            }
            if code == ")" {
                block = None;
                continue;
            }
            let (verb, rest) = match &block {
                Some(verb) => (verb.clone(), code),
                None => {
                    let (verb, rest) = code.split_once(char::is_whitespace).unwrap_or((code, ""));
                    let rest = rest.trim();
                    if rest == "(" {
                        block = Some(verb.to_string());
                        continue;
                    }
                    (verb.to_string(), rest)
                }
            };
            let words: Vec<String> = rest.split_whitespace().map(word).collect();
            match (verb.as_str(), words.as_slice()) {
                ("module", [path, ..]) => m.module = Some(path.clone()),
                ("go", [v, ..]) => m.go = Some(v.clone()),
                ("toolchain", [v, ..]) => m.toolchain = v.strip_prefix("go").map(str::to_string),
                ("require", [path, version, ..]) => {
                    let indirect = comment.split([';', ' ']).any(|c| c.trim() == "indirect");
                    m.requires.push(Require { path: path.clone(), version: version.clone(), indirect });
                }
                ("replace", [path, ..]) => m.replaced.push(path.clone()),
                _ => {}
            }
        }
        m
    }

    /// Whether the module's own code asks for `path` (a `require` that
    /// isn't `// indirect`); `None` when go.mod doesn't name it.
    pub fn direct(&self, path: &str) -> Option<bool> {
        self.requires.iter().find(|r| r.path == path).map(|r| !r.indirect)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Project {
    /// `go.mod` at the top, read; `None` when there's none.
    pub module: Option<GoMod>,
    /// A `go.work` at the top.
    pub work: bool,
    /// The folders with a main package, relative: `.`, `cmd/orbit`.
    pub mains: Vec<String>,
    /// A `.golangci.yml` (or `.yaml`, `.toml`, `.json`) at the top.
    pub golangci: bool,
}

/// The package clause of Go source `text`: `main`.
pub fn package_name(text: &str) -> Option<&str> {
    let mut in_comment = false;
    for line in text.lines() {
        let line = line.trim();
        if in_comment {
            if let Some(at) = line.find("*/") {
                in_comment = false;
                if let Some(rest) = line[at + 2..].trim().strip_prefix("package ") {
                    return rest.split_whitespace().next();
                }
            }
            continue;
        }
        if line.starts_with("/*") && !line.contains("*/") {
            in_comment = true;
            continue;
        }
        if let Some(rest) = line.strip_prefix("package ") {
            return rest.split(|c: char| c.is_whitespace() || c == ';' || c == '/').next();
        }
    }
    None
}

/// Whether folder `dir` holds a main package: a `.go` file, not a test,
/// that says `package main`.
fn is_main(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else { return false };
    entries.flatten().any(|e| {
        let name = e.file_name().to_string_lossy().into_owned();
        name.ends_with(".go") && !name.ends_with("_test.go") && std::fs::read_to_string(e.path()).is_ok_and(|t| package_name(&t) == Some("main"))
    })
}

impl Project {
    /// The project in folder `root` (`/project`).
    pub fn read(root: &str) -> Project {
        let root = Path::new(root);
        let module = std::fs::read_to_string(root.join("go.mod")).ok().map(|t| GoMod::parse(&t));
        let mut mains = Vec::new();
        if is_main(root) {
            mains.push(".".to_string());
        }
        if let Ok(entries) = std::fs::read_dir(root.join("cmd")) {
            let mut cmds: Vec<String> = entries.flatten().filter(|e| e.path().is_dir() && is_main(&e.path())).map(|e| format!("cmd/{}", e.file_name().to_string_lossy())).collect();
            cmds.sort();
            mains.extend(cmds);
        }
        Project {
            module,
            work: root.join("go.work").is_file(),
            mains,
            golangci: [".golangci.yml", ".golangci.yaml", ".golangci.toml", ".golangci.json"].iter().any(|f| root.join(f).is_file()),
        }
    }

    /// The Go the project asks for at least: `go.mod`'s `go` line.
    pub fn go(&self) -> Option<&str> {
        self.module.as_ref().and_then(|m| m.go.as_deref())
    }

    /// The toolchain it suggests, when that's newer than its `go` line.
    pub fn toolchain(&self) -> Option<&str> {
        let m = self.module.as_ref()?;
        let toolchain = m.toolchain.as_deref()?;
        let newer = match (fenix_lang_kit::Version::parse(toolchain), m.go.as_deref().and_then(fenix_lang_kit::Version::parse)) {
            (Some(t), Some(g)) => t > g,
            (Some(_), None) => true,
            _ => false,
        };
        newer.then_some(toolchain)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub const GO_MOD: &str = include_str!("samples/go.mod");

    /// A folder for a test, made empty.
    pub fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("fenix-go-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn go_mod_says_its_module_go_and_requirements() {
        let m = GoMod::parse(GO_MOD);
        assert_eq!(m.module.as_deref(), Some("example.com/orbit"));
        assert_eq!((m.go.as_deref(), m.toolchain.as_deref()), (Some("1.22"), Some("1.24.7")));
        let reqs: Vec<(&str, &str, bool)> = m.requires.iter().map(|r| (r.path.as_str(), r.version.as_str(), r.indirect)).collect();
        assert_eq!(
            reqs,
            [
                ("golang.org/x/text", "v0.3.5", false),
                ("gopkg.in/yaml.v2", "v2.2.2", false),
                ("github.com/google/uuid", "v1.3.0", true),
                ("github.com/spf13/pflag", "v1.0.5", false),
            ]
        );
        assert_eq!(m.replaced, ["github.com/spf13/pflag"]);
        assert_eq!(m.direct("gopkg.in/yaml.v2"), Some(true));
        assert_eq!(m.direct("github.com/google/uuid"), Some(false));
        assert_eq!(m.direct("golang.org/x/tools"), None);
        assert_eq!(GoMod::parse(""), GoMod::default());
    }

    #[test]
    fn a_package_clause_is_found_past_comments_and_build_tags() {
        assert_eq!(package_name("//go:build linux\n\n// Package main runs it.\npackage main\n"), Some("main"));
        assert_eq!(package_name("/*\nlicense\n*/\npackage store // the store\n"), Some("store"));
        assert_eq!(package_name("/* x */ package"), None);
        assert_eq!(package_name(""), None);
    }

    #[test]
    fn a_folder_says_its_main_packages_and_toolchain() {
        let dir = temp("read");
        let root = dir.to_string_lossy().into_owned();
        assert_eq!(Project::read(&root), Project::default(), "nothing there");

        std::fs::write(dir.join("go.mod"), "module example.com/orbit\n\ngo 1.23.0\n\ntoolchain go1.24.7\n").unwrap();
        std::fs::write(dir.join("main.go"), "package main\n\nfunc main() {}\n").unwrap();
        std::fs::write(dir.join("main_test.go"), "package main\n").unwrap();
        for (cmd, pkg) in [("orbitctl", "main"), ("admin", "main"), ("shared", "shared")] {
            std::fs::create_dir_all(dir.join("cmd").join(cmd)).unwrap();
            std::fs::write(dir.join("cmd").join(cmd).join("x.go"), format!("package {pkg}\n")).unwrap();
        }
        std::fs::write(dir.join(".golangci.yml"), "version: \"2\"\n").unwrap();
        let p = Project::read(&root);
        assert_eq!(p.mains, [".", "cmd/admin", "cmd/orbitctl"]);
        assert!(p.golangci && !p.work);
        assert_eq!((p.go(), p.toolchain()), (Some("1.23.0"), Some("1.24.7")));

        std::fs::write(dir.join("go.mod"), "module example.com/orbit\n\ngo 1.24.7\n\ntoolchain go1.24.7\n").unwrap();
        std::fs::write(dir.join("go.work"), "go 1.24.7\n").unwrap();
        let p = Project::read(&root);
        assert_eq!(p.toolchain(), None, "no newer than its go line");
        assert!(p.work);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
