//! What the pack knows of the project, from its files: whether it builds
//! with `build.zig`, what `build.zig.zon` says (its name, version and the
//! least Zig it builds with), and, without a `build.zig`, the Zig files
//! at its top, each run or tested on its own.

use std::path::Path;

/// How many lone files get tasks, and how much of each is read to see
/// whether it has a `main`.
pub const LONE_FILES: usize = 20;
const READ_BYTES: usize = 256 << 10;

/// `build.zig.zon`, as far as the pack reads it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Zon {
    pub name: Option<String>,
    pub version: Option<String>,
    pub minimum_zig_version: Option<String>,
}

impl Zon {
    /// Its top-level fields: `.name = .orbit` (Zig 0.14's enum literal)
    /// or `.name = "orbit"`, and the strings `.version` and
    /// `.minimum_zig_version`. A dependency's fields, a level down, are
    /// left alone.
    pub fn parse(text: &str) -> Zon {
        let mut zon = Zon::default();
        let mut depth = 0i32;
        for line in text.lines() {
            let code = line.split("//").next().unwrap_or_default();
            if depth == 1 {
                if let Some((key, value)) = code.trim().strip_prefix('.').and_then(|f| f.split_once('=')) {
                    let value = value.trim().trim_end_matches(',').trim();
                    let value = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')).or_else(|| value.strip_prefix('.').filter(|v| !v.is_empty() && !v.contains('{'))).map(str::to_string);
                    match key.trim() {
                        "name" => zon.name = value,
                        "version" => zon.version = value,
                        "minimum_zig_version" => zon.minimum_zig_version = value,
                        _ => {}
                    }
                }
            }
            depth += code.matches('{').count() as i32 - code.matches('}').count() as i32;
        }
        zon
    }
}

/// A Zig file at the project's top, in a project without `build.zig`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lone {
    pub file: String,
    /// It has a `pub fn main`: it runs.
    pub main: bool,
}

/// What the project's files say.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Project {
    /// `build.zig` at the top: `zig build` builds it.
    pub build: bool,
    pub zon: Option<Zon>,
    pub lone: Vec<Lone>,
}

impl Project {
    pub fn read(root: &str) -> Project {
        let root = Path::new(root);
        let build = root.join("build.zig").is_file();
        let zon = std::fs::read_to_string(root.join("build.zig.zon")).ok().map(|t| Zon::parse(&t));
        let mut lone = Vec::new();
        if !build {
            let mut files: Vec<String> = std::fs::read_dir(root).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with(".zig") && root.join(n).is_file()).collect();
            files.sort();
            for file in files.into_iter().take(LONE_FILES) {
                let text = std::fs::read(root.join(&file)).map(|b| String::from_utf8_lossy(&b[..b.len().min(READ_BYTES)]).into_owned()).unwrap_or_default();
                lone.push(Lone { main: text.lines().any(|l| l.trim_start().starts_with("pub fn main(")), file });
            }
        }
        Project { build, zon, lone }
    }

    /// The least Zig it builds with, from `build.zig.zon`.
    pub fn minimum_zig(&self) -> Option<&str> {
        self.zon.as_ref()?.minimum_zig_version.as_deref()
    }

    /// Whether it's a Zig project at all: a build, or a Zig file.
    pub fn is_zig(&self) -> bool {
        self.build || self.zon.is_some() || !self.lone.is_empty()
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A fresh folder for a test.
    pub fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("fenix-zig-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn build_zig_zon_says_its_name_and_the_zig_it_needs() {
        let zon = Zon::parse(include_str!("samples/build.zig.zon"));
        assert_eq!(zon, Zon { name: Some("orbit".into()), version: Some("0.3.0".into()), minimum_zig_version: Some("0.17.0".into()) });
        let old = Zon::parse(".{\n    .name = \"orbit\", // the package\n    .version = \"1.0.0\",\n    .dependencies = .{},\n}\n");
        assert_eq!((old.name.as_deref(), old.minimum_zig_version), (Some("orbit"), None), "Zig 0.13's string name");
        assert_eq!(Zon::parse(""), Zon::default());
    }

    #[test]
    fn a_project_without_build_zig_has_its_files_run_alone() {
        let dir = temp("read");
        std::fs::write(dir.join("hello.zig"), "const std = @import(\"std\");\n\npub fn main() !void {}\n").unwrap();
        std::fs::write(dir.join("math.zig"), "pub fn add(a: i32, b: i32) i32 {\n    return a + b;\n}\n").unwrap();
        std::fs::write(dir.join("notes.md"), "").unwrap();
        let p = Project::read(&dir.to_string_lossy());
        assert_eq!(p.lone, [Lone { file: "hello.zig".into(), main: true }, Lone { file: "math.zig".into(), main: false }]);
        assert!(!p.build && p.is_zig() && p.minimum_zig().is_none());

        std::fs::write(dir.join("build.zig"), "").unwrap();
        std::fs::write(dir.join("build.zig.zon"), include_str!("samples/build.zig.zon")).unwrap();
        let p = Project::read(&dir.to_string_lossy());
        assert!(p.build && p.lone.is_empty(), "zig build builds them");
        assert_eq!(p.minimum_zig(), Some("0.17.0"));
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(!Project::read(&dir.to_string_lossy()).is_zig());
    }
}
