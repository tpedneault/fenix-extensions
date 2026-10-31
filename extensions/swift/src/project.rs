//! What the project is, as its files say: `Package.swift` (the tools
//! version its first line asks for, its executables), an Xcode project
//! or Swift files instead, and which of swift-format's, SwiftFormat's and
//! SwiftLint's configs it has.
//!
//! `Package.swift` is Swift, which only SwiftPM runs; it's read lightly
//! here (`swift package describe` would resolve the dependencies first,
//! on every save), which is enough for the products it names.

use std::path::Path;

/// The folders and files SwiftPM's tools take, where they're there.
pub const SOURCES: &[&str] = &["Package.swift", "Sources", "Tests", "Plugins"];

/// `Package.swift`, as far as the pack needs it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Package {
    /// `// swift-tools-version:5.9`: the least Swift that reads it.
    pub tools: Option<String>,
    /// The products `swift run` takes: its `.executable` products, else
    /// its `.executableTarget`s.
    pub executables: Vec<String>,
}

/// The name each `call` in `text` gives first: `.executable(name:
/// "orbitctl", ...)`'s `orbitctl`.
fn names(text: &str, call: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (at, _) in text.match_indices(call) {
        let rest = text[at + call.len()..].trim_start();
        let Some(rest) = rest.strip_prefix("name:") else { continue };
        let Some(rest) = rest.trim_start().strip_prefix('"') else { continue };
        if let Some((name, _)) = rest.split_once('"') {
            if !out.iter().any(|n| n == name) {
                out.push(name.to_string());
            }
        }
    }
    out
}

impl Package {
    pub fn parse(text: &str) -> Package {
        // The first line says the tools version: `// swift-tools-version:
        // 6.0`, and maybe `;(swiftLanguageVersions)` after it.
        let tools = text.lines().next().and_then(|l| l.trim_start_matches(['/', ' ']).strip_prefix("swift-tools-version")).and_then(|v| v.trim_start().strip_prefix(':')).map(|v| v.split(|c: char| c == ';' || c.is_whitespace()).find(|w| !w.is_empty()).unwrap_or_default().to_string()).filter(|v| !v.is_empty());
        // What's said, its line comments taken off.
        let code: String = text.lines().map(|l| l.split_once("//").map_or(l, |(code, _)| code)).collect::<Vec<_>>().join("\n");
        let mut executables = names(&code, ".executable(");
        if executables.is_empty() {
            executables = names(&code, ".executableTarget(");
        }
        Package { tools, executables }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Project {
    /// `Package.swift` at the top, read; `None` when there's none.
    pub package: Option<Package>,
    /// An `.xcodeproj` or `.xcworkspace` at the top.
    pub xcode: bool,
    /// A `.swift` file at the top or in a target's folder in `Sources/`.
    pub sources: bool,
    /// swift-format's `.swift-format`.
    pub swift_format: bool,
    /// SwiftFormat's `.swiftformat`.
    pub swiftformat: bool,
    /// SwiftLint's `.swiftlint.yml`.
    pub swiftlint: bool,
    /// Which of [`SOURCES`] are there.
    pub dirs: Vec<String>,
}

fn has_swift(dir: &Path) -> bool {
    std::fs::read_dir(dir).into_iter().flatten().flatten().any(|e| e.path().extension().is_some_and(|x| x == "swift"))
}

impl Project {
    /// The project in folder `root` (`/project`).
    pub fn read(root: &str) -> Project {
        let root = Path::new(root);
        let names: Vec<String> = std::fs::read_dir(root).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        let has = |name: &str| names.iter().any(|n| n == name);
        Project {
            package: std::fs::read_to_string(root.join("Package.swift")).ok().map(|t| Package::parse(&t)),
            xcode: names.iter().any(|n| n.ends_with(".xcodeproj") || n.ends_with(".xcworkspace")),
            sources: has_swift(root) || std::fs::read_dir(root.join("Sources")).into_iter().flatten().flatten().any(|e| has_swift(&e.path())),
            swift_format: has(".swift-format"),
            swiftformat: has(".swiftformat"),
            swiftlint: has(".swiftlint.yml") || has(".swiftlint.yaml"),
            dirs: SOURCES.iter().filter(|s| has(s)).map(|s| s.to_string()).collect(),
        }
    }

    /// Whether it's Swift's: a package, an Xcode project, or Swift files.
    pub fn is_swift(&self) -> bool {
        self.package.is_some() || self.xcode || self.sources
    }

    /// The Swift its `Package.swift` asks for at least.
    pub fn tools(&self) -> Option<&str> {
        self.package.as_ref().and_then(|p| p.tools.as_deref())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub const PACKAGE: &str = include_str!("samples/Package.swift");

    /// A folder for a test, made empty.
    pub fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("fenix-swift-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Files made under `dir`, with their folders.
    pub fn write(dir: &Path, files: &[(&str, &str)]) {
        for (path, text) in files {
            let path = dir.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
    }

    #[test]
    fn package_swift_says_its_tools_version_and_executables() {
        let p = Package::parse(PACKAGE);
        assert_eq!(p.tools.as_deref(), Some("5.9"));
        assert_eq!(p.executables, ["orbitctl", "orbit-server"], "the products, not the commented-out one");
        let targets = "// swift-tools-version: 6.0;(swiftLanguageVersions)\nlet package = Package(\n    name: \"Tool\",\n    targets: [\n        .executableTarget(name: \"Tool\"),\n        .testTarget(name: \"ToolTests\", dependencies: [\"Tool\"]),\n    ]\n)\n";
        let p = Package::parse(targets);
        assert_eq!((p.tools.as_deref(), p.executables.as_slice()), (Some("6.0"), &["Tool".to_string()][..]), "no products: its executable targets");
        assert_eq!(Package::parse("import PackageDescription\n"), Package::default(), "the tools line comes first, or not at all");
        assert_eq!(Package::parse("//swift-tools-version:5.7.1\n").tools.as_deref(), Some("5.7.1"));
    }

    #[test]
    fn a_folder_says_whether_its_swifts_and_its_configs() {
        let dir = temp("read");
        let root = dir.to_string_lossy().into_owned();
        assert!(!Project::read(&root).is_swift(), "nothing there");

        write(&dir, &[("Sources/Orbit/main.swift", "print(1)\n")]);
        let p = Project::read(&root);
        assert!(p.is_swift() && p.sources && p.package.is_none());
        assert_eq!(p.dirs, ["Sources"]);

        write(&dir, &[("Package.swift", PACKAGE), ("Tests/OrbitTests/a.swift", ""), (".swiftlint.yml", "disabled_rules: []\n"), (".swift-format", "{}\n")]);
        let p = Project::read(&root);
        assert_eq!(p.tools(), Some("5.9"));
        assert!(p.swiftlint && p.swift_format && !p.swiftformat && !p.xcode);
        assert_eq!(p.dirs, ["Package.swift", "Sources", "Tests"]);

        let app = temp("xcode");
        std::fs::create_dir_all(app.join("Orbit.xcodeproj")).unwrap();
        write(&app, &[(".swiftformat", "--indent 4\n")]);
        let p = Project::read(&app.to_string_lossy());
        assert!(p.xcode && p.is_swift() && p.swiftformat);
        std::fs::remove_dir_all(&dir).unwrap();
        std::fs::remove_dir_all(&app).unwrap();
    }
}
