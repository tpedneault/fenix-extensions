//! What the project is, as its files say: CMake (`CMakeLists.txt`'s
//! `cmake_minimum_required`, its presets), vcpkg's manifest, Conan's
//! conanfile, clangd's `.clangd`, and where `compile_commands.json` is.

use std::path::Path;

use fenix_lang_kit::serde_json::{self, Value};

use crate::presets::{Configure, Presets};

/// A dependency the project asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dep {
    pub name: String,
    /// The version it names (Conan's `fmt/11.1.4`), when it does.
    pub version: Option<String>,
    /// `host` (vcpkg's host tools), `build` (Conan's tool requires),
    /// `test`; `None` for the rest.
    pub group: Option<String>,
}

/// Conan's conanfile.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Conan {
    /// `conanfile.txt` or `conanfile.py`.
    pub file: String,
    /// What `conanfile.txt` requires; a `conanfile.py`'s are code.
    pub deps: Vec<Dep>,
    /// A layout (`cmake_layout`): `conan install` writes CMake presets.
    pub layout: bool,
    /// `conan.lock` beside it.
    pub locked: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Project {
    pub cmake: bool,
    /// `cmake_minimum_required(VERSION 3.25...3.30)`'s 3.25.
    pub cmake_min: Option<String>,
    pub presets: Option<Presets>,
    /// Why the presets didn't read.
    pub presets_error: Option<String>,
    /// vcpkg's manifest (`vcpkg.json`): its dependencies.
    pub vcpkg: Option<Vec<Dep>>,
    pub conan: Option<Conan>,
    /// `.clangd`'s `CompilationDatabase`.
    pub clangd_db: Option<String>,
    /// An Arduino sketch: the Arduino extension's.
    pub arduino: bool,
    /// C or C++ files at the top or in `src/`.
    pub sources: bool,
}

const SOURCES: &[&str] = &["c", "cc", "cpp", "cxx", "c++", "h", "hh", "hpp", "hxx", "ixx", "cppm"];

/// `cmake_minimum_required`'s lowest version.
pub fn cmake_minimum(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let at = lower.lines().filter(|l| !l.trim_start().starts_with('#')).find_map(|l| l.find("cmake_minimum_required").map(|i| l[i..].to_string()))?;
    let args = at.split_once('(')?.1.split(')').next()?.to_string();
    let mut words = args.split_whitespace();
    words.find(|w| *w == "version")?;
    let v = words.next()?;
    let v = v.split("...").next()?.trim_matches('"');
    (!v.is_empty()).then(|| v.to_string())
}

/// `.clangd`'s `CompilationDatabase: build/debug`.
pub fn clangd_database(text: &str) -> Option<String> {
    text.lines().find_map(|l| l.trim().strip_prefix("CompilationDatabase:")).map(|v| v.trim().trim_matches(['"', '\'']).to_string()).filter(|v| !v.is_empty() && v != "Ancestors" && v != "None")
}

/// vcpkg.json's dependencies: a name, or `{ "name", "host" }`.
pub fn vcpkg_deps(text: &str) -> Result<Vec<Dep>, String> {
    let doc: Value = serde_json::from_str(text).map_err(|e| format!("vcpkg.json: {e}"))?;
    let mut out = Vec::new();
    for d in doc.get("dependencies").and_then(Value::as_array).into_iter().flatten() {
        let (name, host) = match d {
            Value::String(s) => (s.clone(), false),
            Value::Object(o) => match o.get("name").and_then(Value::as_str) {
                Some(n) => (n.to_string(), o.get("host").and_then(Value::as_bool).unwrap_or(false)),
                None => continue,
            },
            _ => continue,
        };
        out.push(Dep { name, version: None, group: host.then(|| "host".to_string()) });
    }
    Ok(out)
}

/// conanfile.txt's requirements, by section.
pub fn conan_txt(text: &str) -> (Vec<Dep>, bool) {
    let mut deps = Vec::new();
    let mut section = String::new();
    let mut layout = false;
    for line in text.lines().map(|l| l.split('#').next().unwrap_or("").trim()).filter(|l| !l.is_empty()) {
        if let Some(s) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            section = s.trim().to_string();
            continue;
        }
        let group = match section.as_str() {
            "requires" => None,
            "tool_requires" | "build_requires" => Some("build"),
            "test_requires" => Some("test"),
            "layout" => {
                layout = true;
                continue;
            }
            _ => continue,
        };
        let reference = line.split('@').next().unwrap_or(line);
        let (name, version) = match reference.split_once('/') {
            Some((n, v)) => (n.trim(), Some(v.trim().to_string()).filter(|v| !v.is_empty())),
            None => (reference.trim(), None),
        };
        deps.push(Dep { name: name.to_string(), version, group: group.map(str::to_string) });
    }
    (deps, layout)
}

impl Project {
    /// The project in folder `root` (`/project`).
    pub fn read(root: &str) -> Project {
        let root = Path::new(root);
        let text = |name: &str| std::fs::read_to_string(root.join(name)).ok();
        let mut p = Project::default();
        if let Some(t) = text("CMakeLists.txt") {
            p.cmake = true;
            p.cmake_min = cmake_minimum(&t);
        }
        match Presets::read(root) {
            Some(Ok(presets)) => p.presets = Some(presets),
            Some(Err(why)) => p.presets_error = Some(why),
            None => {}
        }
        p.vcpkg = text("vcpkg.json").map(|t| vcpkg_deps(&t).unwrap_or_default());
        p.conan = if let Some(t) = text("conanfile.txt") {
            let (deps, layout) = conan_txt(&t);
            Some(Conan { file: "conanfile.txt".into(), deps, layout, locked: false })
        } else {
            text("conanfile.py").map(|t| Conan { file: "conanfile.py".into(), deps: Vec::new(), layout: t.contains("layout("), locked: false })
        };
        if let Some(c) = p.conan.as_mut() {
            c.locked = root.join("conan.lock").is_file();
        }
        p.clangd_db = text(".clangd").and_then(|t| clangd_database(&t));
        let names = |dir: &Path| std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().to_ascii_lowercase()).collect::<Vec<_>>();
        let top = names(root);
        p.arduino = top.iter().any(|n| n.ends_with(".ino")) || top.iter().any(|n| n == "sketch.yaml");
        let is_source = |n: &String| n.rsplit_once('.').is_some_and(|(_, ext)| SOURCES.contains(&ext));
        p.sources = top.iter().any(is_source) || names(&root.join("src")).iter().any(is_source);
        p
    }

    /// Whether it's C or C++ this pack is for.
    pub fn is_cpp(&self) -> bool {
        !self.arduino && (self.cmake || self.presets.is_some() || self.vcpkg.is_some() || self.conan.is_some() || self.clangd_db.is_some() || self.sources)
    }

    /// The configure preset used (`cpp.preset`, else one called debug).
    pub fn preset(&self, wanted: Option<&str>) -> Option<&Configure> {
        self.presets.as_ref()?.pick(wanted)
    }

    /// The build folder: the preset's, else `build` for CMake.
    pub fn build_dir(&self, wanted: Option<&str>) -> Option<String> {
        match self.preset(wanted) {
            Some(c) => c.binary_dir.clone(),
            None => self.cmake.then(|| "build".to_string()),
        }
    }

    /// The folder `compile_commands.json` should be in: `.clangd`'s, else
    /// the build folder's.
    pub fn database_dir(&self, wanted: Option<&str>) -> Option<String> {
        self.clangd_db.clone().or_else(|| self.build_dir(wanted))
    }

    /// The folder (relative to `root`) that has a `compile_commands.json`:
    /// the one it should be in, the project itself, `build`, or another
    /// preset's.
    pub fn database(&self, root: &str, wanted: Option<&str>) -> Option<String> {
        let mut dirs: Vec<String> = self.database_dir(wanted).into_iter().collect();
        dirs.extend([".".to_string(), "build".to_string()]);
        dirs.extend(self.presets.iter().flat_map(|p| p.visible().filter_map(|c| c.binary_dir.clone())));
        dirs.into_iter().find(|d| Path::new(root).join(d).join("compile_commands.json").is_file())
    }

    /// The build folders whose CMake gets the file API's query: every
    /// preset's, or `build`.
    pub fn build_dirs(&self) -> Vec<String> {
        let mut out: Vec<String> = match &self.presets {
            Some(p) => p.visible().filter_map(|c| c.binary_dir.clone()).collect(),
            None => self.cmake.then(|| "build".to_string()).into_iter().collect(),
        };
        out.retain(|d| !d.starts_with('/') && !d.starts_with("..") && d.get(1..2) != Some(":"));
        out.dedup();
        out
    }

    /// The CMake it needs: `cmake_minimum_required`'s, or its presets'.
    pub fn cmake_needs(&self) -> Option<String> {
        let mut all: Vec<&String> = self.cmake_min.iter().chain(self.presets.iter().filter_map(|p| p.min_cmake.as_ref())).collect();
        all.sort_by(|a, b| fenix_lang_kit::Version::parse(a).cmp(&fenix_lang_kit::Version::parse(b)));
        all.last().map(|s| s.to_string())
    }

    /// The generator the build uses is Ninja's.
    pub fn wants_ninja(&self, wanted: Option<&str>) -> bool {
        self.preset(wanted).is_some_and(Configure::ninja)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A folder for a test, made empty.
    pub fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("fenix-cpp-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn cmake_minimum_required_says_the_lowest() {
        assert_eq!(cmake_minimum("cmake_minimum_required(VERSION 3.25...3.30)\nproject(x)").as_deref(), Some("3.25"));
        assert_eq!(cmake_minimum("# cmake_minimum_required(VERSION 2.8)\nCMAKE_MINIMUM_REQUIRED( version 3.16 FATAL_ERROR )").as_deref(), Some("3.16"));
        assert_eq!(cmake_minimum("project(x)"), None);
    }

    #[test]
    fn the_manifests_say_their_dependencies() {
        let v = vcpkg_deps(r#"{"name": "orbit", "dependencies": ["fmt", {"name": "catch2", "features": []}, {"name": "vcpkg-cmake", "host": true}, 3]}"#).unwrap();
        assert_eq!(v.iter().map(|d| (d.name.as_str(), d.group.as_deref())).collect::<Vec<_>>(), [("fmt", None), ("catch2", None), ("vcpkg-cmake", Some("host"))]);
        assert!(vcpkg_deps("{").unwrap_err().starts_with("vcpkg.json: "));
        let (deps, layout) = conan_txt("[requires]\nhello/1.0\ngreet/[>=2 <3]@acme/stable # pinned\n\n[tool_requires]\ncmake/3.30.1\n[test_requires]\ngtest/1.15.0\n[generators]\nCMakeDeps\n[layout]\ncmake_layout\n");
        assert!(layout);
        assert_eq!(
            deps.iter().map(|d| (d.name.as_str(), d.version.as_deref(), d.group.as_deref())).collect::<Vec<_>>(),
            [("hello", Some("1.0"), None), ("greet", Some("[>=2 <3]"), None), ("cmake", Some("3.30.1"), Some("build")), ("gtest", Some("1.15.0"), Some("test"))]
        );
        assert_eq!(clangd_database("CompileFlags:\n  CompilationDatabase: \"build/debug\"\n").as_deref(), Some("build/debug"));
        assert_eq!(clangd_database("CompileFlags:\n  CompilationDatabase: Ancestors\n"), None);
    }

    #[test]
    fn a_folder_says_what_it_builds_with() {
        let dir = temp("read");
        let root = dir.to_string_lossy().into_owned();
        let p = Project::read(&root);
        assert!(!p.is_cpp() && p.build_dir(None).is_none());

        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/main.c"), "").unwrap();
        assert!(Project::read(&root).is_cpp(), "a C file is enough");

        std::fs::write(dir.join("CMakeLists.txt"), "cmake_minimum_required(VERSION 3.20)\n").unwrap();
        std::fs::write(dir.join("CMakePresets.json"), include_str!("samples/CMakePresets.json")).unwrap();
        std::fs::write(dir.join("conanfile.py"), "def layout(self):\n    cmake_layout(self)\n").unwrap();
        std::fs::write(dir.join("vcpkg.json"), "{\"dependencies\": [\"fmt\"]}").unwrap();
        let p = Project::read(&root);
        assert_eq!((p.cmake_min.as_deref(), p.cmake_needs().as_deref()), (Some("3.20"), Some("3.25")), "the presets' version 6 needs more");
        assert_eq!(p.build_dir(None).as_deref(), Some("build/debug"));
        assert_eq!(p.build_dir(Some("release")).as_deref(), Some("build/release"));
        assert_eq!(p.build_dirs(), ["build/debug", "build/release"]);
        assert!(p.wants_ninja(None));
        assert_eq!(p.conan.as_ref().map(|c| (c.file.as_str(), c.layout, c.locked)), Some(("conanfile.py", true, false)));
        assert_eq!(p.vcpkg.as_ref().map(Vec::len), Some(1));

        assert_eq!(p.database(&root, None), None);
        std::fs::create_dir_all(dir.join("build/release")).unwrap();
        std::fs::write(dir.join("build/release/compile_commands.json"), "[]").unwrap();
        assert_eq!(p.database(&root, None).as_deref(), Some("build/release"), "another preset's, until debug's is there");
        std::fs::create_dir_all(dir.join("build/debug")).unwrap();
        std::fs::write(dir.join("build/debug/compile_commands.json"), "[]").unwrap();
        assert_eq!(p.database(&root, None).as_deref(), Some("build/debug"));

        std::fs::write(dir.join(".clangd"), "CompileFlags:\n  CompilationDatabase: out\n").unwrap();
        std::fs::write(dir.join("blink.ino"), "").unwrap();
        let p = Project::read(&root);
        assert_eq!(p.database_dir(None).as_deref(), Some("out"));
        assert!(p.arduino && !p.is_cpp(), "a sketch is the Arduino extension's");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
