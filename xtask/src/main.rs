//! The repository's tasks:
//!
//! ```text
//! cargo xtask validate [index.json]          every extension's manifest, checked; dependencies
//!                                            found here or in the index, their versions there
//! cargo xtask build <extension-dir>          a code extension's extension.wasm, from its source
//! cargo xtask pack <extension-dir> [dist]    a .fenixext (built first); prints its id, version, sha256, size
//! cargo xtask for-tag <name>-v<version>      the extension folder a release tag names
//! cargo xtask index-add <index.json> <extension-dir> <url> <sha256> <size>
//! ```
//!
//! The manifest is read the way Fenix reads it (`fenix-extensions`'
//! `manifest.rs`): what's refused here would be refused there.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Deserialize)]
struct Manifest {
    extension: Info,
    engine: Engine,
    #[serde(default)]
    dependencies: BTreeMap<String, String>,
    /// What a code extension may do: copied into the index as it is, so
    /// Fenix shows it before installing.
    #[serde(default)]
    permissions: Option<toml::Table>,
}

#[derive(Debug, Deserialize)]
struct Info {
    id: String,
    name: String,
    version: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    categories: Vec<String>,
    #[serde(default)]
    repository: Option<String>,
    runtime: String,
}

#[derive(Debug, Deserialize)]
struct Engine {
    fenix: String,
    /// The extension API a code extension was built against.
    #[serde(default)]
    api: Option<String>,
}

/// What runs an extension: nothing (data), its code, or -- a bundle --
/// nothing of its own: it's the extensions it depends on.
const RUNTIMES: &[&str] = &["none", "wasm", "lua", "bundle"];

/// What an extension may be filed under: the Extensions page has a place
/// for each.
const CATEGORIES: &[&str] = &["languages", "themes", "snippets", "keymaps", "templates", "tools", "data", "web", "mission", "fun"];

/// The pictures `screenshots/` may hold, as Fenix reads them, and the
/// most one may be.
const SCREENSHOTS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp"];
const MAX_SCREENSHOT: u64 = 2 * 1024 * 1024;

/// What a bundle's folder may carry besides its manifest.
const BUNDLE_FILES: &[&str] = &["README.md", "CHANGELOG.md", "screenshots"];

fn valid_id(id: &str) -> bool {
    let parts: Vec<&str> = id.split('.').collect();
    parts.len() >= 2 && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_'))
}

fn manifest(dir: &Path) -> Result<Manifest, String> {
    let path = dir.join("extension.toml");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let m: Manifest = toml::from_str(&text).map_err(|e| format!("{}: {}", path.display(), e.message()))?;
    let at = path.display();
    if !valid_id(&m.extension.id) {
        return Err(format!("{at}: id {:?} isn't publisher.name", m.extension.id));
    }
    if m.extension.name.trim().is_empty() {
        return Err(format!("{at}: name is empty"));
    }
    semver::Version::parse(&m.extension.version).map_err(|e| format!("{at}: version: {e}"))?;
    semver::VersionReq::parse(&m.engine.fenix).map_err(|e| format!("{at}: engine.fenix: {e}"))?;
    if !RUNTIMES.contains(&m.extension.runtime.as_str()) {
        return Err(format!("{at}: runtime {:?} -- one of {}", m.extension.runtime, RUNTIMES.join(", ")));
    }
    if let Some(c) = m.extension.categories.iter().find(|c| !CATEGORIES.contains(&c.as_str())) {
        return Err(format!("{at}: category {c:?} -- one of {}", CATEGORIES.join(", ")));
    }
    // Fenix 2.1.0 and before read a setting's default as text only: a bare
    // `default = true` made fenix.java 1.1.0 refuse to install.
    let raw: toml::Table = toml::from_str(&text).map_err(|e| format!("{at}: {}", e.message()))?;
    let settings = raw.get("contributes").and_then(|c| c.get("settings")).and_then(|s| s.as_array()).cloned().unwrap_or_default();
    for setting in &settings {
        if let Some(default) = setting.get("default").filter(|d| !d.is_str()) {
            let key = setting.get("key").and_then(|k| k.as_str()).unwrap_or("?");
            return Err(format!("{at}: setting {key}: default = {default} -- write it as text (\"{}\"): Fenix 2.1.0 and before read no other", default.to_string().trim_matches('"')));
        }
    }
    if m.extension.runtime == "bundle" {
        if m.dependencies.is_empty() {
            return Err(format!("{at}: a bundle is the extensions it depends on, and it has none"));
        }
        if m.engine.api.is_some() || m.permissions.as_ref().is_some_and(|p| !p.is_empty()) {
            return Err(format!("{at}: a bundle runs nothing: no engine.api, no permissions"));
        }
        let first = |rel: &Path| rel.components().next().map(|c| c.as_os_str().to_string_lossy().into_owned()).unwrap_or_default();
        if let Some(extra) = files(dir).iter().map(|rel| first(rel)).find(|f| f != "extension.toml" && !BUNDLE_FILES.contains(&f.as_str())) {
            return Err(format!("{at}: a bundle carries only its manifest, {} -- not {extra}", BUNDLE_FILES.join(", ")));
        }
    }
    if m.extension.runtime == "wasm" {
        let api = m.engine.api.as_deref().ok_or(format!("{at}: a wasm extension says its API (engine.api)"))?;
        let full = match api.matches('.').count() { 0 => format!("{api}.0.0"), 1 => format!("{api}.0"), _ => api.to_string() };
        semver::Version::parse(&full).map_err(|e| format!("{at}: engine.api: {e}"))?;
        if !dir.join("extension.wasm").is_file() && !dir.join("Cargo.toml").is_file() {
            return Err(format!("{at}: a wasm extension needs its extension.wasm, or a Cargo.toml to build it from"));
        }
    }
    for (dep, range) in &m.dependencies {
        if !valid_id(dep) {
            return Err(format!("{at}: dependency {dep:?} isn't an id"));
        }
        semver::VersionReq::parse(range).map_err(|e| format!("{at}: dependency {dep}: {e}"))?;
    }
    Ok(m)
}

/// What a code extension's source leaves out of its package: the
/// crate, and what building it makes besides `extension.wasm`.
fn source_only(rel: &Path) -> bool {
    let first = rel.components().next().map(|c| c.as_os_str().to_string_lossy().into_owned()).unwrap_or_default();
    ["src", "target", "tests", "Cargo.toml", "Cargo.lock", ".gitignore"].contains(&first.as_str())
}

/// A code extension with a `Cargo.toml`: built for `wasm32-wasip2`, its
/// component copied to `extension.wasm`. One without has nothing to build.
fn build(dir: &Path) -> Result<(), String> {
    let cargo_toml = dir.join("Cargo.toml");
    if !cargo_toml.is_file() {
        return Ok(());
    }
    let text = std::fs::read_to_string(&cargo_toml).map_err(|e| e.to_string())?;
    let crate_name = toml::from_str::<toml::Table>(&text)
        .map_err(|e| format!("{}: {}", cargo_toml.display(), e.message()))?
        .get("package")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .ok_or(format!("{}: no package name", cargo_toml.display()))?
        .replace('-', "_");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let mut command = std::process::Command::new(cargo);
    command.args(["build", "--release", "--locked", "--target", "wasm32-wasip2"]).current_dir(dir);
    // What `cargo run` set for xtask is xtask's, not the extension's build.
    for (key, _) in std::env::vars().filter(|(k, _)| k.starts_with("CARGO_") && k != "CARGO_HOME") {
        command.env_remove(key);
    }
    let status = command
        .status()
        .map_err(|e| format!("cargo: {e}"))?;
    if !status.success() {
        return Err(format!("{}: the build failed", dir.display()));
    }
    let built = dir.join("target/wasm32-wasip2/release").join(format!("{crate_name}.wasm"));
    std::fs::copy(&built, dir.join("extension.wasm")).map_err(|e| format!("{}: {e}", built.display()))?;
    // stderr: `pack` keeps stdout for its JSON, which the release reads.
    eprintln!("built {}", dir.join("extension.wasm").display());
    Ok(())
}

/// Every file under `dir`, relative, sorted.
fn files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = entry.path();
            if p.is_dir() {
                // A code extension's build, never shipped.
                if !(d == dir && entry.file_name() == "target" && dir.join("Cargo.toml").is_file()) {
                    stack.push(p);
                }
            } else if let Ok(rel) = p.strip_prefix(dir) {
                out.push(rel.to_path_buf());
            }
        }
    }
    out.sort();
    out
}

/// An extension as the dependency checks know it: its versions (the one
/// here, or those the index has) and what it depends on.
struct Known {
    versions: Vec<semver::Version>,
    dependencies: BTreeMap<String, String>,
}

/// What the index (`index.json`, from the registry branch) has: each
/// extension and bundle with its versions, and what the newest depends
/// on.
fn indexed(index: &serde_json::Value) -> BTreeMap<String, Known> {
    let mut out = BTreeMap::new();
    let lists = ["extensions", "bundles"].map(|k| index[k].as_array().cloned().unwrap_or_default());
    for listing in lists.iter().flatten() {
        let Some(id) = listing["id"].as_str() else { continue };
        let releases = listing["versions"].as_array().cloned().unwrap_or_default();
        let mut versions: Vec<(semver::Version, &serde_json::Value)> = releases.iter().filter_map(|r| Some((semver::Version::parse(r["version"].as_str()?).ok()?, r))).collect();
        versions.sort_by(|a, b| a.0.cmp(&b.0));
        let dependencies = versions.last().and_then(|(_, r)| r["dependencies"].as_object()).map(|d| d.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect()).unwrap_or_default();
        out.insert(id.to_string(), Known { versions: versions.into_iter().map(|(v, _)| v).collect(), dependencies });
    }
    out
}

/// Every dependency (a bundle's members with them) is an extension here
/// or in the index, in a version its range takes, and nothing depends on
/// itself however far round.
fn check_dependencies(known: &BTreeMap<String, Known>, here: &BTreeSet<String>) -> Result<(), String> {
    for id in here {
        for (dep, range) in &known[id].dependencies {
            let req = semver::VersionReq::parse(range).map_err(|e| format!("{id}: dependency {dep}: {e}"))?;
            let Some(k) = known.get(dep) else {
                return Err(format!("{id} depends on {dep}, which is neither in extensions/ nor in the index"));
            };
            if !k.versions.iter().any(|v| req.matches(v)) {
                let have: Vec<String> = k.versions.iter().map(|v| v.to_string()).collect();
                return Err(format!("{id} depends on {dep} {range}, and there's only {}", have.join(", ")));
            }
        }
    }
    // Depth first: one met again on the way down is a circle.
    fn visit<'a>(id: &'a str, known: &'a BTreeMap<String, Known>, path: &mut Vec<&'a str>, done: &mut BTreeSet<&'a str>) -> Result<(), String> {
        if let Some(at) = path.iter().position(|p| *p == id) {
            let mut circle = path[at..].to_vec();
            circle.push(id);
            return Err(format!("dependencies go round in a circle: {}", circle.join(" -> ")));
        }
        if !done.insert(id) {
            return Ok(());
        }
        path.push(id);
        for dep in known.get(id).map(|k| k.dependencies.keys()).into_iter().flatten() {
            visit(dep, known, path, done)?;
        }
        path.pop();
        Ok(())
    }
    let mut done = BTreeSet::new();
    for id in here {
        visit(id, known, &mut Vec::new(), &mut done)?;
    }
    Ok(())
}

fn validate(root: &Path, index: Option<&Path>) -> Result<(), String> {
    let mut known = match index {
        Some(path) => indexed(&serde_json::from_str(&std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?).map_err(|e| format!("{}: {e}", path.display()))?),
        None => BTreeMap::new(),
    };
    let mut here = BTreeSet::new();
    let mut ids = BTreeMap::new();
    let dirs = std::fs::read_dir(root.join("extensions")).map_err(|e| format!("extensions/: {e}"))?;
    let mut count = 0;
    for dir in dirs.flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
        let m = manifest(&dir)?;
        if let Some(other) = ids.insert(m.extension.id.clone(), dir.clone()) {
            return Err(format!("{} and {} have the same id {}", other.display(), dir.display(), m.extension.id));
        }
        // A theme is at least a TOML file with a name.
        for rel in files(&dir).into_iter().filter(|f| f.starts_with("themes") && f.extension().is_some_and(|x| x == "toml")) {
            let text = std::fs::read_to_string(dir.join(&rel)).map_err(|e| e.to_string())?;
            let value: toml::Table = toml::from_str(&text).map_err(|e| format!("{}: {}", dir.join(&rel).display(), e.message()))?;
            if !value.get("name").is_some_and(|n| n.is_str()) {
                return Err(format!("{}: a theme needs a name", dir.join(&rel).display()));
            }
        }
        // A snippet has its header -- a key, then `# --` -- and a body.
        for rel in files(&dir).into_iter().filter(|f| f.starts_with("snippets") && f.extension().is_some_and(|x| x == "snippet")) {
            let text = std::fs::read_to_string(dir.join(&rel)).map_err(|e| e.to_string())?;
            let (header, body) = text.split_once("# --\n").ok_or_else(|| format!("{}: no `# --` line ending the header", dir.join(&rel).display()))?;
            if !header.lines().any(|l| l.strip_prefix("# key:").is_some_and(|k| !k.trim().is_empty())) {
                return Err(format!("{}: a snippet needs a `# key:`", dir.join(&rel).display()));
            }
            if body.trim().is_empty() {
                return Err(format!("{}: the snippet has no body", dir.join(&rel).display()));
            }
        }
        // A screenshot is a picture Fenix reads, and not a huge one.
        for rel in files(&dir).into_iter().filter(|f| f.starts_with("screenshots")) {
            let kind = rel.extension().map(|x| x.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            if !SCREENSHOTS.contains(&kind.as_str()) {
                return Err(format!("{}: a screenshot is one of {}", dir.join(&rel).display(), SCREENSHOTS.join(", ")));
            }
            let size = std::fs::metadata(dir.join(&rel)).map_err(|e| e.to_string())?.len();
            if size > MAX_SCREENSHOT {
                return Err(format!("{}: a screenshot is at most {} MB", dir.join(&rel).display(), MAX_SCREENSHOT / 1024 / 1024));
            }
        }
        let version = semver::Version::parse(&m.extension.version).map_err(|e| e.to_string())?;
        let entry = known.entry(m.extension.id.clone()).or_insert(Known { versions: Vec::new(), dependencies: BTreeMap::new() });
        entry.versions.push(version);
        entry.dependencies = m.dependencies.clone();
        here.insert(m.extension.id.clone());
        let kind = if m.extension.runtime == "bundle" { "bundle" } else { "" };
        println!("ok  {} {} ({}){}", m.extension.id, m.extension.version, dir.display(), if kind.is_empty() { String::new() } else { format!(" {kind} of {}", m.dependencies.keys().cloned().collect::<Vec<_>>().join(", ")) });
        count += 1;
    }
    check_dependencies(&known, &here)?;
    println!("{count} extension(s)");
    Ok(())
}

fn pack(dir: &Path, dist: &Path) -> Result<(), String> {
    build(dir)?;
    let m = manifest(dir)?;
    let code = dir.join("Cargo.toml").is_file();
    std::fs::create_dir_all(dist).map_err(|e| e.to_string())?;
    let name = format!("{}-{}.fenixext", m.extension.id, m.extension.version);
    let out = dist.join(&name);
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&out).map_err(|e| e.to_string())?);
    for rel in files(dir).into_iter().filter(|rel| !(code && source_only(rel))) {
        let entry = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/");
        zip.start_file(entry, zip::write::SimpleFileOptions::default()).map_err(|e| e.to_string())?;
        zip.write_all(&std::fs::read(dir.join(&rel)).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    }
    zip.finish().map_err(|e| e.to_string())?;
    let bytes = std::fs::read(&out).map_err(|e| e.to_string())?;
    let sha: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
    println!("{}", serde_json::json!({ "id": m.extension.id, "version": m.extension.version, "file": out, "sha256": sha, "size": bytes.len() }));
    Ok(())
}

fn for_tag(root: &Path, tag: &str) -> Result<(), String> {
    let (name, version) = tag.rsplit_once("-v").ok_or_else(|| format!("{tag}: a release tag is <name>-v<version>"))?;
    let dir = root.join("extensions").join(name);
    let m = manifest(&dir)?;
    if m.extension.version != version {
        return Err(format!("{tag}: extensions/{name} is version {}", m.extension.version));
    }
    println!("{}", dir.display());
    Ok(())
}

fn index_add(index: &Path, dir: &Path, url: &str, sha256: &str, size: u64) -> Result<(), String> {
    let m = manifest(dir)?;
    let mut doc: serde_json::Value = match std::fs::read_to_string(index) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| format!("{}: {e}", index.display()))?,
        Err(_) => serde_json::json!({ "format": 1, "extensions": [] }),
    };
    // A bundle is listed apart: a Fenix that doesn't know bundles reads
    // past `bundles`, as it reads past any key it doesn't know.
    let bundle = m.extension.runtime == "bundle";
    let key = if bundle { "bundles" } else { "extensions" };
    if doc[key].is_null() {
        doc[key] = serde_json::json!([]);
    }
    let mut release = serde_json::json!({
        "version": m.extension.version,
        "fenix": m.engine.fenix,
        "runtime": m.extension.runtime,
        "url": url,
        "sha256": sha256,
        "size": size,
        "dependencies": m.dependencies,
        "categories": m.extension.categories,
    });
    if bundle {
        release["bundle"] = serde_json::json!(m.dependencies.keys().collect::<Vec<_>>());
    }
    if let Some(api) = &m.engine.api {
        release["api"] = api.clone().into();
    }
    if let Some(permissions) = &m.permissions {
        release["permissions"] = serde_json::to_value(permissions).map_err(|e| e.to_string())?;
    }
    let list = doc[key].as_array_mut().ok_or(format!("the index's {key} isn't a list"))?;
    let at = match list.iter().position(|e| e["id"] == m.extension.id.as_str()) {
        Some(i) => i,
        None => {
            list.push(serde_json::json!({ "id": m.extension.id, "versions": [] }));
            list.len() - 1
        }
    };
    let listing = &mut list[at];
    listing["name"] = m.extension.name.clone().into();
    listing["description"] = m.extension.description.clone().into();
    listing["categories"] = serde_json::json!(m.extension.categories);
    listing["repository"] = serde_json::json!(m.extension.repository);
    let versions = listing["versions"].as_array_mut().ok_or("versions isn't a list")?;
    versions.retain(|v| v["version"] != m.extension.version.as_str());
    versions.push(release);
    versions.sort_by_key(|v| semver::Version::parse(v["version"].as_str().unwrap_or("0.0.0")).unwrap_or(semver::Version::new(0, 0, 0)));
    list.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    std::fs::write(index, serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())? + "\n").map_err(|e| e.to_string())?;
    println!("{} {} in {}", m.extension.id, m.extension.version, index.display());
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().expect("the repository").to_path_buf();
    let arg = |i: usize| args.get(i).cloned().unwrap_or_default();
    let result = match args.first().map(String::as_str) {
        Some("validate") => validate(&root, args.get(1).map(Path::new)),
        Some("build") => build(Path::new(&arg(1))),
        Some("pack") => pack(Path::new(&arg(1)), Path::new(args.get(2).map(String::as_str).unwrap_or("dist"))),
        Some("for-tag") => for_tag(&root, &arg(1)),
        Some("index-add") => arg(5).parse::<u64>().map_err(|e| format!("size: {e}")).and_then(|size| index_add(Path::new(&arg(1)), Path::new(&arg(2)), &arg(3), &arg(4), size)),
        _ => Err("cargo xtask validate [index.json] | build <dir> | pack <dir> [dist] | for-tag <tag> | index-add <index.json> <dir> <url> <sha256> <size>".into()),
    };
    if let Err(e) = result {
        eprintln!("xtask: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of its own under the system's temp folder, gone when
    /// dropped.
    struct Temp(PathBuf);

    impl Temp {
        fn new(name: &str) -> Temp {
            let dir = std::env::temp_dir().join(format!("xtask-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Temp(dir)
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// `extensions/<name>` under `root`, with this manifest's `[extension]`
    /// lines and what follows `[engine]`.
    fn extension(root: &Path, name: &str, version: &str, runtime: &str, rest: &str) -> PathBuf {
        let dir = root.join("extensions").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("extension.toml"), format!("[extension]\nid = \"acme.{name}\"\nname = \"{name}\"\nversion = \"{version}\"\nruntime = \"{runtime}\"\n\n[engine]\nfenix = \">=2.0, <3\"\n{rest}")).unwrap();
        dir
    }

    #[test]
    fn a_bundle_is_its_members_and_nothing_else() {
        let t = Temp::new("bundle");
        extension(&t.0, "notes", "2.0.1", "none", "");
        let bundle = extension(&t.0, "personal", "1.0.0", "bundle", "\n[dependencies]\n\"acme.notes\" = \"^2\"\n");
        std::fs::write(bundle.join("README.md"), "# Personal").unwrap();
        std::fs::create_dir_all(bundle.join("screenshots")).unwrap();
        std::fs::write(bundle.join("screenshots/board.png"), "png").unwrap();
        validate(&t.0, None).unwrap();
        let m = manifest(&bundle).unwrap();
        assert_eq!(m.extension.runtime, "bundle");

        std::fs::write(bundle.join("screenshots/board.bmp"), "bmp").unwrap();
        assert!(validate(&t.0, None).unwrap_err().contains("a screenshot is one of"));
        std::fs::remove_file(bundle.join("screenshots/board.bmp")).unwrap();
        std::fs::create_dir_all(bundle.join("themes")).unwrap();
        std::fs::write(bundle.join("themes/x.toml"), "name = \"x\"").unwrap();
        assert!(manifest(&bundle).unwrap_err().contains("a bundle carries only its manifest"));
        std::fs::remove_dir_all(bundle.join("themes")).unwrap();

        let empty = extension(&t.0, "empty", "1.0.0", "bundle", "");
        assert!(manifest(&empty).unwrap_err().contains("has none"));
        let code = extension(&t.0, "empty", "1.0.0", "bundle", "api = \"0.8\"\n\n[dependencies]\n\"acme.notes\" = \"^2\"\n");
        assert!(manifest(&code).unwrap_err().contains("runs nothing"));
    }

    #[test]
    fn a_category_is_one_of_the_pages() {
        let t = Temp::new("category");
        let dir = extension(&t.0, "dusk", "1.0.0", "none", "");
        let with = |categories: &str| {
            let text = std::fs::read_to_string(dir.join("extension.toml")).unwrap().replace("runtime = ", &format!("categories = [{categories}]\nruntime = "));
            std::fs::write(dir.join("extension.toml"), text).unwrap();
            manifest(&dir)
        };
        assert!(with("\"themes\", \"fun\"").is_ok());
        extension(&t.0, "dusk", "1.0.0", "none", "");
        assert!(with("\"colours\"").unwrap_err().contains("category \"colours\" -- one of languages"));
    }

    /// An id, its versions, and what it depends on.
    type Entry<'a> = (&'a str, &'a [&'a str], &'a [(&'a str, &'a str)]);

    fn known(list: &[Entry]) -> BTreeMap<String, Known> {
        list.iter()
            .map(|(id, versions, deps)| {
                let versions = versions.iter().map(|v| semver::Version::parse(v).unwrap()).collect();
                (id.to_string(), Known { versions, dependencies: deps.iter().map(|(d, r)| (d.to_string(), r.to_string())).collect() })
            })
            .collect()
    }

    #[test]
    fn members_are_there_in_a_version_that_fits_and_never_in_a_circle() {
        let here: BTreeSet<String> = ["acme.pack".to_string()].into();
        let fine = known(&[("acme.pack", &["1.0.0"], &[("acme.a", "^2"), ("acme.b", "^1")]), ("acme.a", &["2.1.0"], &[]), ("acme.b", &["1.0.0", "1.3.0"], &[])]);
        check_dependencies(&fine, &here).unwrap();
        let missing = known(&[("acme.pack", &["1.0.0"], &[("acme.a", "^2")])]);
        assert!(check_dependencies(&missing, &here).unwrap_err().contains("neither in extensions/ nor in the index"));
        let old = known(&[("acme.pack", &["1.0.0"], &[("acme.a", "^2")]), ("acme.a", &["1.0.0", "1.4.0"], &[])]);
        assert!(check_dependencies(&old, &here).unwrap_err().contains("acme.a ^2, and there's only 1.0.0, 1.4.0"));
        let round = known(&[("acme.pack", &["1.0.0"], &[("acme.a", "*")]), ("acme.a", &["1.0.0"], &[("acme.b", "*")]), ("acme.b", &["1.0.0"], &[("acme.pack", "*")])]);
        assert_eq!(check_dependencies(&round, &here).unwrap_err(), "dependencies go round in a circle: acme.pack -> acme.a -> acme.b -> acme.pack");
        let shared = known(&[("acme.pack", &["1.0.0"], &[("acme.a", "*"), ("acme.b", "*")]), ("acme.a", &["1.0.0"], &[("acme.b", "*")]), ("acme.b", &["1.0.0"], &[])]);
        check_dependencies(&shared, &here).unwrap();
    }

    /// A member only the registry has is found there, from the index.
    #[test]
    fn the_index_has_what_extensions_left() {
        let index = serde_json::json!({ "format": 1,
            "extensions": [{ "id": "acme.a", "name": "A", "versions": [{ "version": "2.0.0" }, { "version": "2.2.0", "dependencies": { "acme.b": "^1" } }] }],
            "bundles": [{ "id": "acme.old", "name": "Old", "versions": [{ "version": "1.0.0", "dependencies": { "acme.a": "^2" } }] }] });
        let known = indexed(&index);
        assert_eq!(known["acme.a"].versions.len(), 2);
        assert_eq!(known["acme.a"].dependencies["acme.b"], "^1", "the newest's");
        assert!(known.contains_key("acme.old"));
    }

    #[test]
    fn a_bundle_goes_in_the_index_apart_with_its_members() {
        let t = Temp::new("index");
        let theme = extension(&t.0, "dusk", "1.0.0", "none", "");
        let text = std::fs::read_to_string(theme.join("extension.toml")).unwrap().replace("runtime = ", "categories = [\"themes\"]\nruntime = ");
        std::fs::write(theme.join("extension.toml"), text).unwrap();
        let bundle = extension(&t.0, "evening", "1.0.0", "bundle", "\n[dependencies]\n\"acme.dusk\" = \"^1\"\n");
        let index = t.0.join("index.json");
        index_add(&index, &theme, "u1", "aa", 1).unwrap();
        index_add(&index, &bundle, "u2", "bb", 2).unwrap();
        let doc: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&index).unwrap()).unwrap();
        assert_eq!(doc["extensions"].as_array().unwrap().len(), 1, "an index an older Fenix reads as it did");
        assert_eq!(doc["extensions"][0]["versions"][0]["categories"], serde_json::json!(["themes"]));
        assert_eq!(doc["bundles"][0]["id"], "acme.evening");
        let release = &doc["bundles"][0]["versions"][0];
        assert_eq!((release["runtime"].as_str(), release["bundle"].clone()), (Some("bundle"), serde_json::json!(["acme.dusk"])));
        assert_eq!(release["dependencies"]["acme.dusk"], "^1");
    }
}
