//! CMake's presets, as CMake reads them: `CMakePresets.json` and
//! `CMakeUserPresets.json` (which includes the other), each file's
//! `include`s, and each preset's `inherits`, the earlier parent winning.
//! Only what the pack needs is kept: a configure preset's binary folder,
//! generator and cache variables, and which build and test presets go
//! with it.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use fenix_lang_kit::serde_json::{Map, Value};

/// A configure preset, its parents' fields filled in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Configure {
    pub name: String,
    pub hidden: bool,
    pub display: Option<String>,
    pub generator: Option<String>,
    /// Its binary folder relative to the project (`build/debug`), or a
    /// full path outside it; `None` when it says none, or one that can't
    /// be known here (`$env{...}`, `${hostSystemName}`).
    pub binary_dir: Option<String>,
    pub cache: BTreeMap<String, String>,
    pub toolchain: Option<String>,
    /// It has a `condition`, which may leave it out on this system.
    pub conditional: bool,
}

impl Configure {
    /// Whether it writes `compile_commands.json`: its cache says so.
    pub fn exports_commands(&self) -> bool {
        self.cache.get("CMAKE_EXPORT_COMPILE_COMMANDS").is_some_and(|v| matches!(v.to_ascii_uppercase().as_str(), "ON" | "TRUE" | "1" | "YES" | "Y"))
    }

    /// Ninja's, single or multi-config.
    pub fn ninja(&self) -> bool {
        self.generator.as_deref().is_some_and(|g| g.starts_with("Ninja"))
    }

    /// Visual Studio's, which writes no `compile_commands.json`.
    pub fn visual_studio(&self) -> bool {
        self.generator.as_deref().is_some_and(|g| g.starts_with("Visual Studio"))
    }
}

/// A build or test preset: the configure preset it's for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Step {
    pub name: String,
    pub hidden: bool,
    pub configure: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Presets {
    /// The highest schema `version` of the files read.
    pub version: u64,
    /// The CMake they ask for: `cmakeMinimumRequired`, or what the
    /// schema version needs.
    pub min_cmake: Option<String>,
    /// By name.
    pub configure: Vec<Configure>,
    pub build: Vec<Step>,
    pub test: Vec<Step>,
}

/// The CMake a presets schema version needs.
fn cmake_for(version: u64) -> Option<&'static str> {
    Some(match version {
        0 | 1 => return None,
        2 => "3.20",
        3 => "3.21",
        4 => "3.23",
        5 => "3.24",
        6 => "3.25",
        7 => "3.27",
        8 => "3.28",
        9 => "3.30",
        _ => "4.0",
    })
}

fn version_of(v: &Value) -> Option<String> {
    let n = |k: &str| v.get(k).and_then(Value::as_u64).unwrap_or(0);
    v.is_object().then(|| format!("{}.{}.{}", n("major"), n("minor"), n("patch")))
}

/// The raw presets of each kind, by name, from every file read.
#[derive(Default)]
struct Raw {
    version: u64,
    min: Vec<String>,
    kinds: [BTreeMap<String, Map<String, Value>>; 3],
}

const KINDS: [&str; 3] = ["configurePresets", "buildPresets", "testPresets"];

impl Raw {
    /// `file` (under `root`), and what it includes.
    fn read(&mut self, root: &Path, file: &Path, seen: &mut HashSet<std::path::PathBuf>) -> Result<(), String> {
        let path = root.join(file);
        if !seen.insert(path.clone()) {
            return Ok(());
        }
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", file.display()))?;
        let doc: Value = fenix_lang_kit::serde_json::from_str(&text).map_err(|e| format!("{}: {e}", file.display()))?;
        self.version = self.version.max(doc.get("version").and_then(Value::as_u64).unwrap_or(0));
        if let Some(v) = doc.get("cmakeMinimumRequired").and_then(version_of) {
            self.min.push(v);
        }
        // Included first, so this file's own presets win a clash.
        let dir = file.parent().unwrap_or(Path::new(""));
        for inc in doc.get("include").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
            // A full path (Conan writes relative ones) can't be read here.
            if !Path::new(inc).is_absolute() && !inc.contains("$") {
                self.read(root, &dir.join(inc), seen)?;
            }
        }
        for (i, kind) in KINDS.iter().enumerate() {
            for p in doc.get(*kind).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_object) {
                if let Some(name) = p.get("name").and_then(Value::as_str) {
                    let mut p = p.clone();
                    p.insert("__dir".into(), Value::String(dir.to_string_lossy().replace('\\', "/")));
                    self.kinds[i].insert(name.to_string(), p);
                }
            }
        }
        Ok(())
    }

    /// Preset `name`'s `key`: its own, else its parents', in order.
    fn field(&self, kind: usize, name: &str, key: &str, depth: usize) -> Option<Value> {
        let p = self.kinds[kind].get(name)?;
        if let Some(v) = p.get(key) {
            return Some(v.clone());
        }
        if depth > 32 {
            return None;
        }
        parents(p).into_iter().find_map(|parent| self.field(kind, &parent, key, depth + 1))
    }

    /// Its cache variables: its parents', the earlier winning, then its own.
    fn cache(&self, name: &str, depth: usize, out: &mut BTreeMap<String, String>) {
        let Some(p) = self.kinds[0].get(name) else { return };
        if depth <= 32 {
            for parent in parents(p).iter().rev() {
                self.cache(parent, depth + 1, out);
            }
        }
        for (k, v) in p.get("cacheVariables").and_then(Value::as_object).into_iter().flatten() {
            let value = match v {
                Value::Null => {
                    out.remove(k);
                    continue;
                }
                Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
                Value::String(s) => s.clone(),
                Value::Object(o) => match o.get("value") {
                    Some(Value::String(s)) => s.clone(),
                    Some(Value::Bool(b)) => if *b { "TRUE" } else { "FALSE" }.to_string(),
                    _ => continue,
                },
                _ => continue,
            };
            out.insert(k.clone(), value);
        }
    }
}

fn parents(p: &Map<String, Value>) -> Vec<String> {
    match p.get("inherits") {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(a)) => a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
        _ => Vec::new(),
    }
}

/// A preset's `binaryDir` with its macros filled in, relative to the
/// project when it's in it. `None` when it uses one only CMake knows.
pub fn expand(text: &str, preset: &str, generator: Option<&str>, file_dir: &str) -> Option<String> {
    const SOURCE: &str = "\u{0}";
    let mut out = text.replace("${sourceDir}", SOURCE).replace("${sourceParentDir}", &format!("{SOURCE}/..")).replace("${presetName}", preset).replace("${pathListSep}", if cfg!(windows) { ";" } else { ":" });
    out = out.replace("${fileDir}", &format!("{SOURCE}/{file_dir}"));
    if let Some(g) = generator {
        out = out.replace("${generator}", g);
    }
    if out.contains("${") || out.contains("$env{") || out.contains("$penv{") || out.contains("$vendor{") {
        return None;
    }
    let out = out.replace("${dollar}", "$").replace('\\', "/");
    let rel = match out.strip_prefix(SOURCE) {
        Some(rest) => rest.trim_start_matches('/').to_string(),
        None if out.starts_with('/') || out.get(1..2) == Some(":") => return Some(out),
        None => out,
    };
    Some(normalize(&rel))
}

/// `a/./b/../c` as `a/c`; `.` for the project itself.
pub fn normalize(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|p| *p != "..") => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    if parts.is_empty() {
        ".".into()
    } else {
        parts.join("/")
    }
}

/// A full path from a preset Conan wrote (`/home/u/app/build/Release`)
/// as the project's own (`build/Release`): its longest tail that's a
/// folder in `root`.
pub fn inside(root: &Path, full: &str) -> Option<String> {
    let parts: Vec<&str> = full.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
    (0..parts.len()).map(|i| parts[i..].join("/")).find(|tail| root.join(tail).is_dir())
}

impl Presets {
    /// The project's presets in `root`: `None` when it has neither file.
    pub fn read(root: &Path) -> Option<Result<Presets, String>> {
        let user = root.join("CMakeUserPresets.json").is_file();
        let main = root.join("CMakePresets.json").is_file();
        if !user && !main {
            return None;
        }
        let mut raw = Raw::default();
        let mut seen = HashSet::new();
        let read = (|| {
            if main {
                raw.read(root, Path::new("CMakePresets.json"), &mut seen)?;
            }
            if user {
                raw.read(root, Path::new("CMakeUserPresets.json"), &mut seen)?;
            }
            Ok::<(), String>(())
        })();
        Some(read.map(|_| Presets::resolve(&raw, root)))
    }

    fn resolve(raw: &Raw, root: &Path) -> Presets {
        let s = |v: Option<Value>| v.and_then(|v| v.as_str().map(str::to_string));
        let mut out = Presets { version: raw.version, ..Presets::default() };
        let mut mins: Vec<String> = raw.min.clone();
        mins.extend(cmake_for(raw.version).map(str::to_string));
        out.min_cmake = mins.into_iter().max_by(|a, b| fenix_lang_kit::Version::parse(a).cmp(&fenix_lang_kit::Version::parse(b)));
        for (name, p) in &raw.kinds[0] {
            let generator = s(raw.field(0, name, "generator", 0));
            let file_dir = p.get("__dir").and_then(Value::as_str).unwrap_or("");
            let binary_dir = s(raw.field(0, name, "binaryDir", 0)).and_then(|b| expand(&b, name, generator.as_deref(), file_dir)).map(|b| if b.starts_with('/') || b.get(1..2) == Some(":") { inside(root, &b).unwrap_or(b) } else { b });
            let mut cache = BTreeMap::new();
            raw.cache(name, 0, &mut cache);
            out.configure.push(Configure {
                name: name.clone(),
                hidden: p.get("hidden").and_then(Value::as_bool).unwrap_or(false),
                display: p.get("displayName").and_then(Value::as_str).map(str::to_string),
                generator,
                binary_dir,
                toolchain: s(raw.field(0, name, "toolchainFile", 0)).or_else(|| cache.get("CMAKE_TOOLCHAIN_FILE").cloned()),
                cache,
                conditional: raw.field(0, name, "condition", 0).is_some(),
            });
        }
        for (kind, list) in [(1, &mut out.build), (2, &mut out.test)] {
            for (name, p) in &raw.kinds[kind] {
                list.push(Step { name: name.clone(), hidden: p.get("hidden").and_then(Value::as_bool).unwrap_or(false), configure: s(raw.field(kind, name, "configurePreset", 0)) });
            }
        }
        out
    }

    /// The configure presets one can use.
    pub fn visible(&self) -> impl Iterator<Item = &Configure> {
        self.configure.iter().filter(|c| !c.hidden)
    }

    /// The configure preset to use: `wanted` when it's one, else one
    /// called debug, else the first, those without a condition first.
    pub fn pick(&self, wanted: Option<&str>) -> Option<&Configure> {
        if let Some(c) = wanted.and_then(|w| self.visible().find(|c| c.name == w)) {
            return Some(c);
        }
        let mut all: Vec<&Configure> = self.visible().collect();
        all.sort_by_key(|c| c.conditional);
        all.iter().find(|c| c.name.eq_ignore_ascii_case("debug")).or_else(|| all.iter().find(|c| c.name.to_ascii_lowercase().contains("debug"))).or(all.first()).copied()
    }

    /// The build preset for configure preset `configure`.
    pub fn build_for(&self, configure: &str) -> Option<&Step> {
        self.build.iter().find(|b| !b.hidden && b.configure.as_deref() == Some(configure))
    }

    pub fn test_for(&self, configure: &str) -> Option<&Step> {
        self.test.iter().find(|t| !t.hidden && t.configure.as_deref() == Some(configure))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::temp;

    const SAMPLE: &str = include_str!("samples/CMakePresets.json");

    #[test]
    fn inherits_fills_in_a_presets_fields() {
        let dir = temp("presets");
        std::fs::write(dir.join("CMakePresets.json"), SAMPLE).unwrap();
        let p = Presets::read(&dir).unwrap().unwrap();
        assert_eq!((p.version, p.min_cmake.as_deref()), (6, Some("3.25")));
        let names: Vec<&str> = p.visible().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["debug", "release"], "base is hidden");
        let debug = p.pick(None).unwrap();
        assert_eq!((debug.binary_dir.as_deref(), debug.generator.as_deref()), (Some("build/debug"), Some("Ninja")));
        assert_eq!(debug.cache.get("CMAKE_BUILD_TYPE").map(String::as_str), Some("Debug"));
        assert!(debug.exports_commands() && debug.ninja() && !debug.visual_studio());
        let release = p.pick(Some("release")).unwrap();
        assert_eq!(release.cache.get("CMAKE_BUILD_TYPE").map(String::as_str), Some("Release"), "a {{type, value}} variable");
        assert_eq!(release.binary_dir.as_deref(), Some("build/release"));
        assert_eq!(p.build_for("release").map(|b| b.name.as_str()), Some("release"));
        assert_eq!(p.test_for("debug").map(|b| b.name.as_str()), Some("debug"));
        assert!(p.test_for("release").is_none());
        assert_eq!(p.pick(Some("nope")).map(|c| c.name.as_str()), Some("debug"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn conans_user_presets_include_the_ones_it_wrote() {
        let dir = temp("conan-presets");
        std::fs::create_dir_all(dir.join("build/Release/generators")).unwrap();
        std::fs::write(dir.join("CMakeUserPresets.json"), include_str!("samples/conan-CMakeUserPresets.json")).unwrap();
        std::fs::write(dir.join("build/Release/generators/CMakePresets.json"), include_str!("samples/conan-CMakePresets.json")).unwrap();
        let p = Presets::read(&dir).unwrap().unwrap();
        assert_eq!(p.min_cmake.as_deref(), Some("3.23"), "the user file's version 4 asks more than cmakeMinimumRequired's 3.15");
        let c = p.pick(None).unwrap();
        assert_eq!(c.name, "conan-release");
        assert_eq!(c.binary_dir.as_deref(), Some("build/Release"), "its full path, found in the project");
        assert_eq!(c.toolchain.as_deref(), Some("generators/conan_toolchain.cmake"));
        assert_eq!(c.generator.as_deref(), Some("Unix Makefiles"));
        assert!(!c.exports_commands());
        assert_eq!(p.build_for("conan-release").map(|b| b.name.as_str()), Some("conan-release"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn parents_are_read_in_order_and_a_bad_file_says_why() {
        let dir = temp("presets-order");
        std::fs::write(
            dir.join("CMakePresets.json"),
            r#"{"version": 3, "configurePresets": [
                {"name": "a", "hidden": true, "generator": "Ninja", "binaryDir": "out/a", "cacheVariables": {"X": "a", "Y": true}},
                {"name": "b", "hidden": true, "generator": "Visual Studio 17 2022", "binaryDir": "${sourceDir}/../elsewhere/${presetName}", "cacheVariables": {"X": "b", "Z": "b"}},
                {"name": "c", "inherits": ["a", "b"], "cacheVariables": {"Z": null}},
                {"name": "d", "inherits": "b", "binaryDir": "$env{HOME}/build"},
                {"name": "w", "condition": {"type": "equals", "lhs": "${hostSystemName}", "rhs": "Windows"}, "binaryDir": "${sourceDir}/./w/../win"}
            ],
            "buildPresets": [{"name": "c-build", "configurePreset": "c"}, {"name": "c-hidden", "hidden": true, "configurePreset": "c"}]}"#,
        )
        .unwrap();
        let p = Presets::read(&dir).unwrap().unwrap();
        let get = |n: &str| p.configure.iter().find(|c| c.name == n).unwrap();
        let c = get("c");
        assert_eq!((c.generator.as_deref(), c.binary_dir.as_deref()), (Some("Ninja"), Some("out/a")), "the first parent wins");
        assert_eq!(c.cache, BTreeMap::from([("X".to_string(), "a".to_string()), ("Y".to_string(), "TRUE".to_string())]), "Z taken away");
        assert_eq!(get("b").binary_dir.as_deref(), Some("../elsewhere/b"));
        assert!(get("b").visual_studio());
        assert_eq!(get("d").binary_dir, None, "an environment variable isn't known here");
        assert_eq!(get("w").binary_dir.as_deref(), Some("win"));
        assert_eq!(p.min_cmake.as_deref(), Some("3.21"));
        assert_eq!(p.pick(None).map(|c| c.name.as_str()), Some("c"), "one without a condition first");
        assert_eq!(p.build_for("c").map(|b| b.name.as_str()), Some("c-build"));
        std::fs::write(dir.join("CMakePresets.json"), "{ nope").unwrap();
        assert!(Presets::read(&dir).unwrap().unwrap_err().starts_with("CMakePresets.json: "));
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(Presets::read(&temp("presets-none")).is_none());
    }

    #[test]
    fn paths_are_made_plain() {
        assert_eq!(normalize("a/./b/../c/"), "a/c");
        assert_eq!(normalize(""), ".");
        assert_eq!(normalize("../x"), "../x");
        assert_eq!(expand("${sourceDir}", "p", None, ""), Some(".".into()));
        assert_eq!(expand("C:\\b\\${presetName}", "p", None, ""), Some("C:/b/p".into()));
        assert_eq!(expand("${fileDir}/out", "p", None, "cmake"), Some("cmake/out".into()));
        assert_eq!(expand("${sourceDir}/build/${generator}", "p", Some("Ninja"), ""), Some("build/Ninja".into()));
        assert_eq!(expand("${hostSystemName}", "p", None, ""), None);
    }
}
