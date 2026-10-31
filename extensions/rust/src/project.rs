//! What the project is, as its files say -- `Cargo.toml` (a package, a
//! workspace, its edition and `rust-version`), `rust-toolchain.toml` or
//! `rust-toolchain`, `rustfmt.toml` -- and, once cargo has answered, its
//! packages and their targets from `cargo metadata --no-deps`.

use std::path::Path;

use fenix_lang_kit::Output;
use serde::Deserialize;

/// The toolchain `rust-toolchain.toml` (or the older `rust-toolchain`)
/// pins.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Toolchain {
    /// `stable`, `1.80.0`, `nightly-2026-01-15`.
    pub channel: String,
    pub components: Vec<String>,
    pub targets: Vec<String>,
}

impl Toolchain {
    /// From the file's text: TOML's `[toolchain]`, or a line naming the
    /// channel. A path (a toolchain of your own) isn't one rustup installs.
    pub fn parse(text: &str) -> Option<Toolchain> {
        let strings = |t: &toml::Table, key: &str| -> Vec<String> { t.get(key).and_then(toml::Value::as_array).map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect()).unwrap_or_default() };
        if let Ok(doc) = toml::from_str::<toml::Table>(text) {
            let t = doc.get("toolchain").and_then(toml::Value::as_table)?;
            let channel = t.get("channel").and_then(toml::Value::as_str)?.trim().to_string();
            return (!channel.is_empty()).then(|| Toolchain { channel, components: strings(t, "components"), targets: strings(t, "targets") });
        }
        let line = text.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with('#'))?;
        (!line.contains(['/', '\\', ' ', '='])).then(|| Toolchain { channel: line.to_string(), ..Toolchain::default() })
    }
}

/// What `Cargo.toml` and the files beside it say.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Project {
    pub cargo: bool,
    /// `[package].name`.
    pub name: Option<String>,
    /// A `[workspace]` table.
    pub workspace: bool,
    /// The edition rustfmt formats for: the package's, else the
    /// workspace's `[workspace.package]`.
    pub edition: Option<String>,
    /// `rust-version`, the oldest Rust it builds with.
    pub rust_version: Option<String>,
    pub toolchain: Option<Toolchain>,
    /// Its file's name: `rust-toolchain.toml` or `rust-toolchain`.
    pub toolchain_file: Option<String>,
    /// `rustfmt.toml` (or `.rustfmt.toml`) says its own edition.
    pub rustfmt_edition: bool,
}

/// `key`'s value in `[package]`, else `[workspace.package]`'s: a string,
/// not `{ workspace = true }`.
fn package_str(doc: &toml::Table, key: &str) -> Option<String> {
    let own = doc.get("package").and_then(|p| p.get(key)).and_then(toml::Value::as_str);
    let shared = doc.get("workspace").and_then(|w| w.get("package")).and_then(|p| p.get(key)).and_then(toml::Value::as_str);
    own.or(shared).map(str::to_string)
}

impl Project {
    /// From `Cargo.toml`'s text alone.
    pub fn parse(cargo_toml: &str) -> Result<Project, String> {
        let doc: toml::Table = toml::from_str(cargo_toml).map_err(|e| format!("Cargo.toml: {}", e.message()))?;
        Ok(Project {
            cargo: true,
            name: doc.get("package").and_then(|p| p.get("name")).and_then(toml::Value::as_str).map(str::to_string),
            workspace: doc.contains_key("workspace"),
            edition: package_str(&doc, "edition"),
            rust_version: package_str(&doc, "rust-version"),
            ..Project::default()
        })
    }

    /// The project in folder `root` (`/project`).
    pub fn read(root: &str) -> Project {
        let root = Path::new(root);
        let text = |name: &str| std::fs::read_to_string(root.join(name)).ok();
        let mut p = text("Cargo.toml").and_then(|t| Project::parse(&t).ok()).unwrap_or_default();
        p.cargo |= root.join("Cargo.toml").is_file();
        for file in ["rust-toolchain.toml", "rust-toolchain"] {
            if let Some(t) = text(file) {
                p.toolchain = Toolchain::parse(&t);
                p.toolchain_file = Some(file.into());
                break;
            }
        }
        p.rustfmt_edition = ["rustfmt.toml", ".rustfmt.toml"].iter().filter_map(|f| text(f)).any(|t| t.parse::<toml::Table>().is_ok_and(|d| d.contains_key("edition")));
        p
    }

    /// `rust-version` as a requirement on rustc: `1.74` is `>=1.74`.
    pub fn wants_rustc(&self) -> Option<String> {
        self.rust_version.as_deref().map(|v| format!(">={}", v.trim()))
    }
}

/// A package of the workspace, from `cargo metadata --no-deps`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Package {
    pub name: String,
    pub manifest_path: String,
    #[serde(default)]
    pub edition: String,
    #[serde(default)]
    pub rust_version: Option<String>,
    #[serde(default)]
    pub default_run: Option<String>,
    #[serde(default)]
    pub targets: Vec<Target>,
    #[serde(default)]
    pub dependencies: Vec<Dep>,
}

/// A dependency a package asks for, by its crate's name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Dep {
    pub name: String,
    /// `registry+https://github.com/rust-lang/crates.io-index` for
    /// crates.io's; none for a path's.
    #[serde(default)]
    pub source: Option<String>,
}

impl Dep {
    pub fn from_crates_io(&self) -> bool {
        self.source.as_deref().is_some_and(|s| s.contains("github.com/rust-lang/crates.io-index") || s.contains("index.crates.io"))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Target {
    pub name: String,
    /// `bin`, `lib`, `example`, `test`, `bench`, `custom-build`.
    pub kind: Vec<String>,
    pub src_path: String,
    #[serde(default, rename = "required-features")]
    pub required_features: Vec<String>,
}

impl Target {
    pub fn is(&self, kind: &str) -> bool {
        self.kind.iter().any(|k| k == kind)
    }
}

/// The workspace as cargo has it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Metadata {
    pub packages: Vec<Package>,
    #[serde(default)]
    pub workspace_root: String,
    #[serde(default)]
    pub target_directory: String,
}

impl Metadata {
    /// `cargo metadata --format-version 1 --no-deps`'s answer.
    pub fn parse(out: &Output) -> Result<Metadata, String> {
        if !out.ok() {
            return Err(out.why());
        }
        out.json()
    }

    /// More than one package: cargo's commands need `-p` to say which.
    pub fn several(&self) -> bool {
        self.packages.len() > 1
    }

    /// Each package's targets of `kind`, with their package.
    pub fn targets(&self, kind: &str) -> Vec<(&Package, &Target)> {
        self.packages.iter().flat_map(|p| p.targets.iter().filter(|t| t.is(kind)).map(move |t| (p, t))).collect()
    }

    /// The crates.io crates its packages ask for, each once.
    pub fn crates_io_deps(&self) -> Vec<String> {
        let mut names: Vec<String> = self.packages.iter().flat_map(|p| &p.dependencies).filter(|d| d.from_crates_io()).map(|d| d.name.clone()).collect();
        names.sort();
        names.dedup();
        names
    }

    /// The edition most of its packages are in, the first's on a tie.
    pub fn edition(&self) -> Option<String> {
        let mut counts: Vec<(&str, usize)> = Vec::new();
        for p in self.packages.iter().filter(|p| !p.edition.is_empty()) {
            match counts.iter_mut().find(|(e, _)| *e == p.edition) {
                Some((_, n)) => *n += 1,
                None => counts.push((&p.edition, 1)),
            }
        }
        let most = counts.iter().map(|(_, n)| *n).max()?;
        counts.into_iter().find(|(_, n)| *n == most).map(|(e, _)| e.to_string())
    }

    /// The newest `rust-version` its packages ask for: the oldest Rust
    /// all of them build with.
    pub fn rust_version(&self) -> Option<String> {
        let mut versions: Vec<(fenix_lang_kit::Version, &str)> = self.packages.iter().filter_map(|p| p.rust_version.as_deref()).filter_map(|v| fenix_lang_kit::Version::parse(v).map(|parsed| (parsed, v))).collect();
        versions.sort_by(|a, b| a.0.cmp(&b.0));
        versions.pop().map(|(_, v)| v.to_string())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub const METADATA: &str = include_str!("samples/cargo-metadata-no-deps.json");

    /// A folder for a test, made empty.
    pub fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("fenix-rust-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    pub fn ok(stdout: &str) -> Output {
        Output { code: Some(0), stdout: stdout.into(), stderr: String::new() }
    }

    #[test]
    fn toolchain_files_say_their_channel() {
        let t = Toolchain::parse("[toolchain]\nchannel = \"1.80.0\"\ncomponents = [\"clippy\", \"rustfmt\"]\ntargets = [\"wasm32-wasip2\"]\nprofile = \"minimal\"\n").unwrap();
        assert_eq!(t, Toolchain { channel: "1.80.0".into(), components: vec!["clippy".into(), "rustfmt".into()], targets: vec!["wasm32-wasip2".into()] });
        assert_eq!(Toolchain::parse("nightly-2026-01-15\n").map(|t| t.channel).as_deref(), Some("nightly-2026-01-15"), "the older file");
        assert_eq!(Toolchain::parse("[toolchain]\npath = \"/opt/rust\"\n"), None, "a path is no channel");
        assert_eq!(Toolchain::parse("/opt/my-rust\n"), None);
        assert_eq!(Toolchain::parse(""), None);
    }

    #[test]
    fn cargo_toml_says_its_edition_and_rust_version() {
        let p = Project::parse("[package]\nname = \"orbit\"\nedition = \"2024\"\nrust-version = \"1.85\"\n").unwrap();
        assert_eq!((p.name.as_deref(), p.edition.as_deref(), p.wants_rustc().as_deref(), p.workspace), (Some("orbit"), Some("2024"), Some(">=1.85"), false));
        let w = Project::parse("[workspace]\nmembers = [\"crates/*\"]\n\n[workspace.package]\nedition = \"2021\"\nrust-version = \"1.70\"\n").unwrap();
        assert!(w.workspace && w.name.is_none());
        assert_eq!((w.edition.as_deref(), w.rust_version.as_deref()), (Some("2021"), Some("1.70")));
        let inherits = Project::parse("[package]\nname = \"a\"\nedition.workspace = true\n").unwrap();
        assert_eq!(inherits.edition, None, "{{ workspace = true }} isn't an edition");
        assert!(Project::parse("[package\n").unwrap_err().starts_with("Cargo.toml: "));
    }

    #[test]
    fn a_folder_says_its_toolchain_and_rustfmts_edition() {
        let dir = temp("read");
        let root = dir.to_string_lossy().into_owned();
        assert_eq!(Project::read(&root), Project::default());
        std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"orbit\"\nedition = \"2021\"\n").unwrap();
        std::fs::write(dir.join("rust-toolchain"), "1.79.0\n").unwrap();
        std::fs::write(dir.join(".rustfmt.toml"), "edition = \"2021\"\nmax_width = 120\n").unwrap();
        let p = Project::read(&root);
        assert!(p.cargo && p.rustfmt_edition);
        assert_eq!((p.toolchain.as_ref().map(|t| t.channel.as_str()), p.toolchain_file.as_deref()), (Some("1.79.0"), Some("rust-toolchain")));
        std::fs::write(dir.join("rust-toolchain.toml"), "[toolchain]\nchannel = \"stable\"\n").unwrap();
        assert_eq!(Project::read(&root).toolchain_file.as_deref(), Some("rust-toolchain.toml"), "the newer file wins");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn cargo_metadata_says_the_packages_and_their_targets() {
        let m = Metadata::parse(&ok(METADATA)).unwrap();
        assert_eq!(m.packages.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["orbit", "orbit-core"]);
        assert!(m.several());
        assert_eq!(m.workspace_root, "/home/u/orbit");
        let names = |kind: &str| m.targets(kind).iter().map(|(p, t)| format!("{}/{}", p.name, t.name)).collect::<Vec<_>>();
        assert_eq!(names("bin"), ["orbit/orbit", "orbit/probe"]);
        assert_eq!(names("example"), ["orbit/demo"]);
        assert_eq!(names("test"), ["orbit/smoke"]);
        assert_eq!(names("lib"), ["orbit-core/orbit_core"]);
        assert_eq!((m.edition().as_deref(), m.rust_version().as_deref()), (Some("2021"), Some("1.70")));
        assert_eq!(m.crates_io_deps(), ["cc", "clap", "smallvec", "time"], "orbit-core's path isn't crates.io's");
        let failed = Output { code: Some(101), stdout: String::new(), stderr: "error: could not find `Cargo.toml` in `/home/u` or any parent directory".into() };
        assert!(Metadata::parse(&failed).unwrap_err().contains("could not find"));
    }
}
