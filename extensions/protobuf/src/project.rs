//! What the pack knows of the project, from its files: where its buf
//! workspace is (`buf.yaml`, or v1's `buf.work.yaml`) and the modules in
//! it, the templates `buf generate` runs (`buf.gen.yaml`, and
//! `buf.gen.<name>.yaml` beside it), the branch `buf breaking` holds it to,
//! and its `.proto` files.

use std::path::Path;

use yaml_rust2::{Yaml, YamlLoader};

/// Folders never looked in for `.proto` files.
const SKIP: &[&str] = &["node_modules", "target", "dist", "build", "vendor", "third_party", "__pycache__", "venv", ".venv", "bin", "obj"];

/// How many `.proto` files are listed, and how deep they're looked for.
pub const MAX_PROTOS: usize = 500;
const MAX_DEPTH: usize = 8;

/// `buf.yaml`, as far as the pack reads it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BufYaml {
    /// `v1`, `v1beta1`, `v2`.
    pub version: String,
    /// v2's modules' paths, v1 workspace's directories (`buf.work.yaml`);
    /// `.` for a v1 module on its own.
    pub modules: Vec<String>,
    /// The modules it depends on (`buf.build/googleapis/googleapis`).
    pub deps: Vec<String>,
}

impl BufYaml {
    /// `buf.yaml`'s text; `None` when it isn't YAML.
    pub fn parse(text: &str) -> Option<BufYaml> {
        let doc = YamlLoader::load_from_str(text).ok()?.into_iter().next().unwrap_or(Yaml::Null);
        let version = doc["version"].as_str().unwrap_or("v1").to_string();
        let modules: Vec<String> = doc["modules"].as_vec().map(|m| m.iter().filter_map(|m| m["path"].as_str().map(clean)).collect()).unwrap_or_default();
        let deps = strings(&doc["deps"]);
        let modules = if modules.is_empty() { vec![".".to_string()] } else { modules };
        Some(BufYaml { version, modules, deps })
    }
}

/// A path as buf.yaml writes it, without `./` or a trailing `/`.
fn clean(path: &str) -> String {
    let p = path.trim().trim_start_matches("./").trim_end_matches('/');
    if p.is_empty() {
        ".".into()
    } else {
        p.to_string()
    }
}

fn strings(yaml: &Yaml) -> Vec<String> {
    yaml.as_vec().map(|v| v.iter().filter_map(|s| s.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

/// A plugin a template runs, and where its output goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plugin {
    /// `buf.build/protocolbuffers/go`, `protoc-gen-go-grpc`, `go`.
    pub name: String,
    pub out: String,
}

/// Whether `file` is a template, by its name -- `buf.gen.yaml`,
/// `buf.gen.<name>.yaml`, `buf.<name>.gen.yaml` -- and that name.
pub fn template_name(file: &str) -> Option<Option<String>> {
    let stem = file.strip_suffix(".yaml").or_else(|| file.strip_suffix(".yml"))?;
    if stem == "buf.gen" {
        return Some(None);
    }
    let name = stem.strip_prefix("buf.gen.").or_else(|| stem.strip_prefix("buf.")?.strip_suffix(".gen"))?;
    (!name.is_empty() && !name.contains('.')).then(|| Some(name.to_string()))
}

/// A `buf generate` template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    /// Its file, in the workspace's folder: `buf.gen.yaml`.
    pub file: String,
    /// Its name's own part: none for `buf.gen.yaml` itself, `docs` for
    /// `buf.gen.docs.yaml` or `buf.docs.gen.yaml`.
    pub name: Option<String>,
    pub plugins: Vec<Plugin>,
}

impl Template {
    /// The template's text: its plugins, v1's (`plugin`, `name`, `remote`)
    /// and v2's (`remote`, `local`, `protoc_builtin`), each with its `out`.
    pub fn parse(file: &str, text: &str) -> Template {
        let name = template_name(file).flatten();
        let doc = YamlLoader::load_from_str(text).ok().and_then(|d| d.into_iter().next()).unwrap_or(Yaml::Null);
        let plugins = doc["plugins"]
            .as_vec()
            .map(|ps| {
                ps.iter()
                    .filter_map(|p| {
                        let name = ["remote", "local", "protoc_builtin", "plugin", "name"].iter().find_map(|k| match &p[*k] {
                            Yaml::String(s) => Some(s.clone()),
                            // v2's `local` may be a command and its arguments.
                            Yaml::Array(words) => words.first().and_then(|w| w.as_str()).map(str::to_string),
                            _ => None,
                        })?;
                        Some(Plugin { name, out: p["out"].as_str().unwrap_or("").to_string() })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Template { file: file.to_string(), name, plugins }
    }

    /// Its plugins in a few words: `go, go-grpc → gen/go`.
    pub fn summary(&self) -> String {
        let short = |n: &str| n.rsplit('/').next().unwrap_or(n).trim_start_matches("protoc-gen-").to_string();
        let names: Vec<String> = self.plugins.iter().map(|p| short(&p.name)).collect();
        let mut outs: Vec<&str> = self.plugins.iter().map(|p| p.out.as_str()).filter(|o| !o.is_empty()).collect();
        outs.dedup();
        match outs.as_slice() {
            [] => names.join(", "),
            [one] => format!("{} → {one}", names.join(", ")),
            _ => format!("{} → {}", names.join(", "), outs.join(", ")),
        }
    }
}

/// What the project's files say.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Project {
    /// The buf workspace's folder below the project: "" at its top, `proto`
    /// when `proto/buf.yaml` is; none without one.
    pub buf_dir: Option<String>,
    pub buf: Option<BufYaml>,
    pub templates: Vec<Template>,
    /// The branch `buf breaking` compares with: origin's default, else
    /// `main` or `master`; none outside a Git repository.
    pub default_branch: Option<String>,
    /// The `.proto` files, below the project, sorted.
    pub protos: Vec<String>,
}

/// The workspace's folder: the top, or a folder a level down, holding
/// `buf.yaml` or `buf.work.yaml`.
fn find_buf_dir(root: &Path) -> Option<String> {
    let holds = |d: &Path| d.join("buf.yaml").is_file() || d.join("buf.work.yaml").is_file();
    if holds(root) {
        return Some(String::new());
    }
    let mut dirs: Vec<String> = std::fs::read_dir(root).into_iter().flatten().flatten().filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| !n.starts_with('.') && !SKIP.contains(&n.as_str())).collect();
    dirs.sort();
    dirs.into_iter().find(|d| holds(&root.join(d)))
}

/// The branch the repository at `root` calls its default: what
/// `origin/HEAD` points at, else `main` or `master` when there's one.
pub fn default_branch(root: &Path) -> Option<String> {
    let git = root.join(".git");
    if !git.exists() {
        return None;
    }
    if let Ok(head) = std::fs::read_to_string(git.join("refs/remotes/origin/HEAD")) {
        if let Some(branch) = head.trim().strip_prefix("ref: refs/remotes/origin/") {
            return Some(branch.to_string());
        }
    }
    let packed = std::fs::read_to_string(git.join("packed-refs")).unwrap_or_default();
    let has = |b: &str| git.join("refs/heads").join(b).is_file() || packed.lines().any(|l| l.ends_with(&format!(" refs/heads/{b}")));
    ["main", "master"].into_iter().find(|b| has(b)).or(Some("main")).map(str::to_string)
}

/// The `.proto` files below `root`, by their paths there.
pub fn protos(root: &Path) -> Vec<String> {
    fn walk(dir: &Path, rel: &str, depth: usize, out: &mut Vec<String>) {
        if depth > MAX_DEPTH || out.len() >= MAX_PROTOS {
            return;
        }
        let mut entries: Vec<_> = std::fs::read_dir(dir).into_iter().flatten().flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = e.file_name().to_string_lossy().into_owned();
            let path = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
            let Ok(kind) = e.file_type() else { continue };
            if kind.is_dir() {
                if !name.starts_with('.') && !SKIP.contains(&name.as_str()) {
                    walk(&e.path(), &path, depth + 1, out);
                }
            } else if name.ends_with(".proto") && out.len() < MAX_PROTOS {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, "", 0, &mut out);
    out
}

impl Project {
    pub fn read(root: &str) -> Project {
        let root = Path::new(root);
        let buf_dir = find_buf_dir(root);
        let (buf, templates) = match &buf_dir {
            Some(dir) => {
                let at = root.join(dir);
                let mut buf = std::fs::read_to_string(at.join("buf.yaml")).ok().and_then(|t| BufYaml::parse(&t));
                // v1's workspace: its directories are the modules.
                if let Some(work) = std::fs::read_to_string(at.join("buf.work.yaml")).ok().and_then(|t| YamlLoader::load_from_str(&t).ok()).and_then(|d| d.into_iter().next()) {
                    let dirs: Vec<String> = strings(&work["directories"]).iter().map(|d| clean(d)).collect();
                    if !dirs.is_empty() {
                        let b = buf.get_or_insert_with(|| BufYaml { version: "v1".into(), ..BufYaml::default() });
                        b.modules = dirs;
                    }
                }
                let mut files: Vec<String> = std::fs::read_dir(&at).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| template_name(n).is_some()).collect();
                // buf.gen.yaml first, then the others by name.
                files.sort_by_key(|f| (f != "buf.gen.yaml", f.clone()));
                let templates = files.iter().filter_map(|f| Some(Template::parse(f, &std::fs::read_to_string(at.join(f)).ok()?))).collect();
                (buf, templates)
            }
            None => (None, Vec::new()),
        };
        Project { default_branch: default_branch(root), protos: protos(root), buf_dir, buf, templates }
    }

    /// Whether it's a Protobuf project at all: a buf workspace, or a
    /// `.proto` file.
    pub fn is_proto(&self) -> bool {
        self.buf_dir.is_some() || !self.protos.is_empty()
    }

    /// Where buf runs: the workspace's folder, as Fenix gives it a command's
    /// `cwd`.
    pub fn cwd(&self) -> String {
        match self.buf_dir.as_deref() {
            Some(d) if !d.is_empty() => format!("{}/{d}", crate::PROJECT),
            _ => crate::PROJECT.to_string(),
        }
    }

    /// A path buf printed (in its workspace's folder) as one below the
    /// project.
    pub fn from_buf(&self, path: &str) -> String {
        let path = path.trim_start_matches("./");
        match self.buf_dir.as_deref() {
            Some(d) if !d.is_empty() => format!("{d}/{path}"),
            _ => path.to_string(),
        }
    }

    /// What `buf breaking --against` takes for `branch`: the repository at
    /// the project's top, from the workspace's folder, and that folder in
    /// it (`../.git#branch=main,subdir=proto`).
    pub fn against(&self, branch: &str) -> String {
        match self.buf_dir.as_deref() {
            Some(d) if !d.is_empty() => {
                let up = "../".repeat(d.split('/').count());
                format!("{up}.git#branch={branch},subdir={d}")
            }
            _ => format!(".git#branch={branch}"),
        }
    }

    /// The folders grpcurl imports from without buf: the modules' (below
    /// the project), else the project's top.
    pub fn import_paths(&self) -> Vec<String> {
        match (&self.buf_dir, &self.buf) {
            (Some(d), Some(b)) => b.modules.iter().map(|m| {
                let m = if m == "." { String::new() } else { m.clone() };
                match (d.is_empty(), m.is_empty()) {
                    (true, true) => ".".to_string(),
                    (true, false) => m,
                    (false, true) => d.clone(),
                    (false, false) => format!("{d}/{m}"),
                }
            }).collect(),
            _ => vec![".".into()],
        }
    }

    /// The `.proto` files as grpcurl names them: below the import path
    /// they're under.
    pub fn protos_for_grpcurl(&self) -> Vec<(String, String)> {
        let imports = self.import_paths();
        self.protos
            .iter()
            .filter_map(|p| {
                let import = imports.iter().filter(|i| *i == "." || p.starts_with(&format!("{i}/"))).max_by_key(|i| i.len())?;
                let rel = if import == "." { p.clone() } else { p[import.len() + 1..].to_string() };
                Some((import.clone(), rel))
            })
            .collect()
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A fresh folder for a test.
    pub fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("fenix-protobuf-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn buf_yaml_says_its_modules_and_deps() {
        let b = BufYaml::parse(include_str!("samples/buf.yaml")).unwrap();
        assert_eq!(b, BufYaml { version: "v2".into(), modules: vec!["proto".into()], deps: vec!["buf.build/googleapis/googleapis".into()] });
        let v1 = BufYaml::parse("version: v1\nlint:\n  use: [DEFAULT]\n").unwrap();
        assert_eq!((v1.version.as_str(), v1.modules.as_slice()), ("v1", [".".to_string()].as_slice()), "a v1 module is its folder");
        assert!(BufYaml::parse("version: [").is_none());
    }

    #[test]
    fn buf_gen_yaml_says_its_plugins() {
        let t = Template::parse("buf.gen.yaml", include_str!("samples/buf.gen.yaml"));
        assert_eq!(t.name, None);
        let names: Vec<(&str, &str)> = t.plugins.iter().map(|p| (p.name.as_str(), p.out.as_str())).collect();
        assert_eq!(names, [("buf.build/protocolbuffers/go", "gen/go"), ("buf.build/grpc/go", "gen/go"), ("protoc-gen-doc", "gen/docs")]);
        assert_eq!(t.summary(), "go, go, doc → gen/go, gen/docs");
        let v1 = Template::parse("buf.gen.ts.yaml", "version: v1\nplugins:\n  - plugin: es\n    out: gen/ts\n  - name: python\n    out: gen/py\n");
        assert_eq!((v1.name.as_deref(), v1.plugins.len(), v1.summary().as_str()), (Some("ts"), 2, "es, python → gen/ts, gen/py"));
        assert!(Template::parse("buf.gen.yaml", "not: [yaml").plugins.is_empty());
        assert_eq!(template_name("buf.go.gen.yaml"), Some(Some("go".into())));
        assert_eq!(template_name("buf.gen.yml"), Some(None));
        for not in ["buf.yaml", "buf.lock", "buf.work.yaml", "buf.gen.yaml.bak"] {
            assert_eq!(template_name(not), None, "{not}");
        }
    }

    #[test]
    fn a_workspace_a_level_down_runs_there_and_breaking_reaches_the_repository() {
        let dir = temp("read");
        std::fs::create_dir_all(dir.join("api/orbit/v1")).unwrap();
        std::fs::create_dir_all(dir.join(".git/refs/remotes/origin")).unwrap();
        std::fs::write(dir.join(".git/refs/remotes/origin/HEAD"), "ref: refs/remotes/origin/trunk\n").unwrap();
        std::fs::write(dir.join("api/buf.yaml"), "version: v2\n").unwrap();
        std::fs::write(dir.join("api/buf.gen.yaml"), include_str!("samples/buf.gen.yaml")).unwrap();
        std::fs::write(dir.join("api/buf.gen.docs.yaml"), "version: v2\nplugins:\n  - local: [protoc-gen-doc, --x]\n    out: docs\n").unwrap();
        std::fs::write(dir.join("api/orbit/v1/orbit.proto"), "syntax = \"proto3\";\n").unwrap();
        std::fs::create_dir_all(dir.join("node_modules/x")).unwrap();
        std::fs::write(dir.join("node_modules/x/x.proto"), "").unwrap();
        let p = Project::read(&dir.to_string_lossy());
        assert_eq!(p.buf_dir.as_deref(), Some("api"));
        assert_eq!(p.templates.iter().map(|t| t.file.as_str()).collect::<Vec<_>>(), ["buf.gen.yaml", "buf.gen.docs.yaml"]);
        assert_eq!(p.templates[1].plugins[0].name, "protoc-gen-doc", "a command and its arguments");
        assert_eq!(p.default_branch.as_deref(), Some("trunk"));
        assert_eq!(p.protos, ["api/orbit/v1/orbit.proto"], "node_modules left alone");
        assert_eq!((p.cwd().as_str(), p.from_buf("orbit/v1/orbit.proto").as_str()), ("/project/api", "api/orbit/v1/orbit.proto"));
        assert_eq!(p.against("trunk"), "../.git#branch=trunk,subdir=api");
        assert_eq!(p.import_paths(), ["api"]);
        assert_eq!(p.protos_for_grpcurl(), [("api".to_string(), "orbit/v1/orbit.proto".to_string())]);
        assert!(p.is_proto());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn without_buf_the_protos_import_from_the_top() {
        let dir = temp("plain");
        std::fs::create_dir_all(dir.join(".git/refs/heads")).unwrap();
        std::fs::write(dir.join(".git/refs/heads/master"), "abc\n").unwrap();
        std::fs::write(dir.join("orbit.proto"), "").unwrap();
        let p = Project::read(&dir.to_string_lossy());
        assert_eq!((p.buf_dir.clone(), p.default_branch.as_deref()), (None, Some("master")));
        assert_eq!(p.against("master"), ".git#branch=master");
        assert_eq!(p.protos_for_grpcurl(), [(".".to_string(), "orbit.proto".to_string())]);
        std::fs::remove_dir_all(dir.join(".git")).unwrap();
        assert_eq!(Project::read(&dir.to_string_lossy()).default_branch, None, "not a repository");
        std::fs::remove_file(dir.join("orbit.proto")).unwrap();
        assert!(!Project::read(&dir.to_string_lossy()).is_proto());

        // v1's workspace names its modules.
        std::fs::write(dir.join("buf.work.yaml"), "version: v1\ndirectories:\n  - proto\n  - vendor/googleapis\n").unwrap();
        let p = Project::read(&dir.to_string_lossy());
        assert_eq!((p.buf_dir.as_deref(), p.import_paths()), (Some(""), vec!["proto".to_string(), "vendor/googleapis".to_string()]));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
