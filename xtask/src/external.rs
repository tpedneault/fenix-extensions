//! Extensions built elsewhere: `external.toml` lists releases an outside
//! author built and hosts as a GitHub release asset of their own
//! repository, and `publishers.toml` says which GitHub owners may release
//! under each publisher id.
//!
//! ```text
//! cargo xtask external-check [--offline] [--index <index.json>]
//! cargo xtask index-add-external <index.json> [--only <id>@<version>]
//! ```
//!
//! What goes into the index is read from the manifest **inside** the
//! package, after its hash is checked: never from what a pull request
//! says of it. Nothing in a downloaded package is ever run.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{check_agent_tools, check_chords, check_dependencies, files, index_write, indexed, manifest, valid_id, Known, Manifest, MAX_SCREENSHOT, SCREENSHOTS};

/// The publisher id this repository's own extensions are under: nobody
/// else's.
pub(crate) const RESERVED: &str = "fenix";
/// The most a package may be, packed. Fenix unpacks at most 64 MiB
/// (`fenix-extensions`' `MAX_UNPACKED`), and refuses a release whose
/// `size` is more than that; a package is compressed, so 50 MiB packed is
/// already far more than any extension here.
pub(crate) const MAX_PACKAGE: u64 = 50 * 1024 * 1024;
/// What a package may unpack to, and the files it may hold: Fenix's own
/// limits (`MAX_UNPACKED`, `MAX_FILES`).
const MAX_UNPACKED: u64 = 64 * 1024 * 1024;
const MAX_FILES: usize = 10_000;
/// Redirects a download may follow: a release asset is one redirect, to
/// GitHub's storage.
const MAX_REDIRECTS: u32 = 5;

/// A different name for each scratch file and folder this run makes.
fn unique() -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    format!("{}-{}", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct External {
    #[serde(default)]
    release: Vec<Entry>,
}

/// One `[[release]]` of `external.toml`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Entry {
    id: String,
    version: String,
    url: String,
    sha256: String,
    repository: String,
}

impl Entry {
    fn label(&self) -> String {
        format!("{}@{}", shown(&self.id), shown(&self.version))
    }

    fn publisher(&self) -> &str {
        self.id.split('.').next().unwrap_or_default()
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Publishers {
    #[serde(default)]
    publishers: BTreeMap<String, Publisher>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Publisher {
    owners: Vec<String>,
}

/// How a package is downloaded: a URL in, at most `limit` bytes out.
pub(crate) trait Fetch {
    fn get(&self, url: &str, limit: u64) -> Result<Vec<u8>, String>;
}

/// With `curl`: https only, redirects too, a few of them, and no more
/// than the limit. No HTTP crate in `xtask` for the sake of one download.
pub(crate) struct Curl;

impl Fetch for Curl {
    fn get(&self, url: &str, limit: u64) -> Result<Vec<u8>, String> {
        let out = std::env::temp_dir().join(format!("xtask-download-{}", unique()));
        let _ = std::fs::remove_file(&out);
        let status = std::process::Command::new("curl")
            // `-q` first: no ~/.curlrc.
            .args(["-q", "--silent", "--show-error", "--fail", "--location", "--proto", "=https", "--proto-redir", "=https"])
            .args(["--max-redirs", &MAX_REDIRECTS.to_string(), "--max-filesize", &limit.to_string(), "--max-time", "300", "--retry", "2"])
            .arg("--output")
            .arg(&out)
            .args(["--url", url])
            .status()
            .map_err(|e| format!("curl: {e}"))?;
        let bytes = std::fs::read(&out);
        let _ = std::fs::remove_file(&out);
        if !status.success() {
            return Err(format!("{url}: the download failed (curl: {status})"));
        }
        let bytes = bytes.map_err(|e| format!("{url}: {e}"))?;
        if bytes.len() as u64 > limit {
            return Err(format!("{url} is more than {limit} bytes"));
        }
        Ok(bytes)
    }
}

/// Text from a pull request or a package, made safe to print: control
/// characters (a terminal's escapes among them) and the ones that turn
/// text round are shown escaped, so a log can't be made to hide a line.
fn shown(text: &str) -> String {
    text.chars().map(|c| if c.is_control() || matches!(c, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}') { c.escape_unicode().to_string() } else { c.to_string() }).collect()
}

fn read_external(root: &Path) -> Result<Vec<Entry>, String> {
    let path = root.join("external.toml");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(toml::from_str::<External>(&text).map_err(|e| format!("external.toml: {}", e.message()))?.release)
}

/// Each publisher with its owners, lower case (GitHub's names aren't).
fn read_publishers(root: &Path) -> Result<BTreeMap<String, Vec<String>>, String> {
    let path = root.join("publishers.toml");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let p: Publishers = toml::from_str(&text).map_err(|e| format!("publishers.toml: {}", e.message()))?;
    let mut out = BTreeMap::new();
    for (id, publisher) in p.publishers {
        if id == RESERVED {
            return Err(format!("publishers.toml: {RESERVED} is this repository's own, and no one else's"));
        }
        if !valid_id(&format!("{id}.x")) {
            return Err(format!("publishers.toml: {:?} isn't a publisher id (lower case letters, digits, - and _)", shown(&id)));
        }
        if publisher.owners.is_empty() {
            return Err(format!("publishers.toml: {id} has no owners"));
        }
        let mut owners = Vec::new();
        for owner in &publisher.owners {
            if !github_name(owner) {
                return Err(format!("publishers.toml: {id}: {:?} isn't a GitHub user or organisation", shown(owner)));
            }
            let owner = owner.to_ascii_lowercase();
            if owners.contains(&owner) {
                return Err(format!("publishers.toml: {id}: {owner} is there twice"));
            }
            owners.push(owner);
        }
        out.insert(id, owners);
    }
    Ok(out)
}

/// A GitHub user's or organisation's name: letters, digits and single
/// hyphens, neither first nor last, at most 39.
fn github_name(name: &str) -> bool {
    (1..=39).contains(&name.len()) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') && !name.starts_with('-') && !name.ends_with('-') && !name.contains("--")
}

/// A repository's name, a tag's or a file's, as a release asset's URL
/// spells it: nothing that needs escaping, and no `.` or `..`.
fn url_part(part: &str) -> bool {
    !part.is_empty() && part.len() <= 255 && part != "." && part != ".." && part.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
}

/// `https://github.com/<owner>/<repo>/releases/download/<tag>/<file>`:
/// its owner, lower case. A release asset stays put, as a branch or a raw
/// file doesn't; and its owner is who published it.
fn release_owner(url: &str) -> Result<String, String> {
    let shape = "a GitHub release asset, https://github.com/<owner>/<repo>/releases/download/<tag>/<file>.fenixext";
    let rest = url.strip_prefix("https://github.com/").ok_or_else(|| format!("{} isn't {shape}", shown(url)))?;
    let parts: Vec<&str> = rest.split('/').collect();
    match parts.as_slice() {
        [owner, repo, "releases", "download", tag, file] if github_name(owner) && url_part(repo) && url_part(tag) && url_part(file) && file.ends_with(".fenixext") => Ok(owner.to_ascii_lowercase()),
        _ => Err(format!("{} isn't {shape}", shown(url))),
    }
}

/// `https://github.com/<owner>/<repo>`: its owner, lower case.
fn repository_owner(url: &str) -> Result<String, String> {
    let rest = url.strip_prefix("https://github.com/").ok_or_else(|| format!("repository {} isn't https://github.com/<owner>/<repo>", shown(url)))?;
    match rest.split('/').collect::<Vec<_>>().as_slice() {
        [owner, repo] if github_name(owner) && url_part(repo) && !repo.ends_with(".git") => Ok(owner.to_ascii_lowercase()),
        _ => Err(format!("repository {} isn't https://github.com/<owner>/<repo>", shown(url))),
    }
}

/// What can be told of an entry without downloading it.
fn check_entry(entry: &Entry, publishers: &BTreeMap<String, Vec<String>>) -> Result<(), String> {
    if !valid_id(&entry.id) {
        return Err(format!("id {:?} isn't publisher.name", shown(&entry.id)));
    }
    let publisher = entry.publisher();
    if publisher == RESERVED {
        return Err(format!("{RESERVED}. is this repository's own: an extension built elsewhere has a publisher id of its own"));
    }
    let owners = publishers.get(publisher).ok_or_else(|| format!("publisher {publisher} isn't in publishers.toml -- a first submission claims it there"))?;
    semver::Version::parse(&entry.version).map_err(|e| format!("version {:?}: {e}", shown(&entry.version)))?;
    if entry.sha256.len() != 64 || !entry.sha256.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')) {
        return Err("sha256 is 64 lower case hex digits".into());
    }
    let owner = release_owner(&entry.url)?;
    if !owners.contains(&owner) {
        return Err(format!("{} is {owner}'s, and publisher {publisher} is {}'s", shown(&entry.url), owners.join(", ")));
    }
    if repository_owner(&entry.repository)? != owner {
        return Err(format!("repository {} isn't {owner}'s, whose the release is", shown(&entry.repository)));
    }
    Ok(())
}

/// Every release in `extensions/`, `(id, version)`.
fn here(root: &Path) -> BTreeSet<(String, String)> {
    let mut out = BTreeSet::new();
    for dir in std::fs::read_dir(root.join("extensions")).into_iter().flatten().flatten() {
        let Ok(text) = std::fs::read_to_string(dir.path().join("extension.toml")) else { continue };
        let Ok(table) = toml::from_str::<toml::Table>(&text) else { continue };
        let field = |k: &str| table.get("extension").and_then(|e| e.get(k)).and_then(|v| v.as_str()).map(str::to_string);
        if let (Some(id), Some(version)) = (field("id"), field("version")) {
            out.insert((id, version));
        }
    }
    out
}

/// `external.toml` and `publishers.toml` read, and every entry checked as
/// far as it can be without the network: the entries, sorted by id and
/// version (so a listing ends up with its newest's name).
fn entries(root: &Path) -> Result<Vec<Entry>, String> {
    let publishers = read_publishers(root)?;
    let mut entries = read_external(root)?;
    let here = here(root);
    let mut seen = BTreeSet::new();
    let mut errors = Vec::new();
    for entry in &entries {
        let mut problem = check_entry(entry, &publishers).err();
        if problem.is_none() && !seen.insert((entry.id.clone(), entry.version.clone())) {
            problem = Some("is there twice".into());
        }
        if problem.is_none() && here.contains(&(entry.id.clone(), entry.version.clone())) {
            problem = Some("is in extensions/: it's released from here".into());
        }
        if let Some(p) = problem {
            errors.push(format!("external.toml: {}: {p}", entry.label()));
        }
    }
    if !errors.is_empty() {
        return Err(errors.join("\n"));
    }
    entries.sort_by(|a, b| (&a.id, semver::Version::parse(&a.version).ok()).cmp(&(&b.id, semver::Version::parse(&b.version).ok())));
    Ok(entries)
}

/// A folder of its own under the system's temp folder, gone when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Scratch, String> {
        let dir = std::env::temp_dir().join(format!("xtask-external-{}", unique()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        Ok(Scratch(dir))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A name inside a package, as written to disk: relative, plain parts, no
/// way above the folder, nothing a file system reads as more than a name.
fn safe_name(name: &str) -> Option<PathBuf> {
    let name = name.strip_suffix('/').unwrap_or(name);
    let parts: Vec<&str> = name.split('/').collect();
    let plain = |p: &&str| !p.is_empty() && *p != "." && *p != ".." && !p.chars().any(|c| c.is_control() || matches!(c, '\\' | ':'));
    parts.iter().all(plain).then(|| parts.iter().collect())
}

/// The package's files written under `dir`, read as defensively as Fenix
/// reads them and then some: no name above the folder, no link, no
/// device, no name twice (even in another case), no encrypted file, a
/// bound on files and on what they unpack to -- and a file's size as its
/// header claims it, not trusted.
fn unpack(bytes: &[u8], dir: &Path) -> Result<(), String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| format!("the package isn't a zip: {e}"))?;
    if zip.len() > MAX_FILES {
        return Err(format!("the package holds more than {MAX_FILES} files"));
    }
    let mut names = BTreeSet::new();
    let mut total = 0u64;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| format!("the package: {e}"))?;
        let name = entry.name().map(|n| n.to_string()).map_err(|e| format!("the package: {e}"))?;
        let rel = safe_name(&name).filter(|_| entry.enclosed_name().is_some()).ok_or_else(|| format!("the package has a file named {:?}, outside the extension's own folder", shown(&name)))?;
        if !names.insert(rel.to_string_lossy().to_lowercase()) {
            return Err(format!("the package has {:?} twice", shown(&name)));
        }
        let kind = entry.unix_mode().map(|m| m & 0o170000).unwrap_or(0);
        if entry.is_symlink() || !matches!(kind, 0 | 0o100000 | 0o040000) {
            return Err(format!("the package's {:?} is a link or a device, not a file", shown(&name)));
        }
        if entry.encrypted() {
            return Err(format!("the package's {:?} is encrypted", shown(&name)));
        }
        if entry.is_dir() {
            continue;
        }
        let claimed = entry.size();
        total = total.saturating_add(claimed);
        if total > MAX_UNPACKED {
            return Err(format!("the package unpacks to more than {} MiB", MAX_UNPACKED / 1024 / 1024));
        }
        let mut data = Vec::with_capacity(claimed as usize);
        (&mut entry).take(claimed + 1).read_to_end(&mut data).map_err(|e| format!("the package's {:?}: {e}", shown(&name)))?;
        if data.len() as u64 > claimed {
            return Err(format!("the package's {:?} is bigger than it says", shown(&name)));
        }
        let to = dir.join(&rel);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::write(&to, data).map_err(|e| format!("{}: {e}", to.display()))?;
    }
    Ok(())
}

/// What `validate` checks of a folder's files besides its manifest,
/// keymaps and agent tools: themes, snippets, screenshots.
fn check_contents(dir: &Path) -> Result<(), String> {
    for rel in files(dir) {
        let path = dir.join(&rel);
        let kind = rel.extension().map(|x| x.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        if rel.starts_with("themes") && kind == "toml" {
            let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", rel.display()))?;
            let value: toml::Table = toml::from_str(&text).map_err(|e| format!("{}: {}", rel.display(), e.message()))?;
            if !value.get("name").is_some_and(|n| n.is_str()) {
                return Err(format!("{}: a theme needs a name", rel.display()));
            }
        }
        if rel.starts_with("snippets") && kind == "snippet" {
            let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", rel.display()))?;
            let (header, body) = text.split_once("# --\n").ok_or_else(|| format!("{}: no `# --` line ending the header", rel.display()))?;
            if !header.lines().any(|l| l.strip_prefix("# key:").is_some_and(|k| !k.trim().is_empty())) {
                return Err(format!("{}: a snippet needs a `# key:`", rel.display()));
            }
            if body.trim().is_empty() {
                return Err(format!("{}: the snippet has no body", rel.display()));
            }
        }
        if rel.starts_with("screenshots") {
            if !SCREENSHOTS.contains(&kind.as_str()) {
                return Err(format!("{}: a screenshot is one of {}", rel.display(), SCREENSHOTS.join(", ")));
            }
            if std::fs::metadata(&path).map_err(|e| e.to_string())?.len() > MAX_SCREENSHOT {
                return Err(format!("{}: a screenshot is at most {} MB", rel.display(), MAX_SCREENSHOT / 1024 / 1024));
            }
        }
    }
    Ok(())
}

/// A package checked: what it was, and its manifest, from inside it.
struct Checked {
    manifest: Manifest,
    size: u64,
}

/// The package an entry names, downloaded and checked: its size and
/// hash, then unpacked and held to the rules a folder here is, its id and
/// version the entry's, and (given what the index has) its dependencies
/// there.
fn inspect(entry: &Entry, fetch: &dyn Fetch, known: Option<BTreeMap<String, Known>>) -> Result<Checked, String> {
    let bytes = fetch.get(&entry.url, MAX_PACKAGE)?;
    if bytes.len() as u64 > MAX_PACKAGE {
        return Err(format!("the package is more than {} MiB", MAX_PACKAGE / 1024 / 1024));
    }
    let sha: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
    if sha != entry.sha256 {
        return Err(format!("{} serves a package whose SHA-256 is {sha}, not {}", shown(&entry.url), entry.sha256));
    }
    let scratch = Scratch::new()?;
    unpack(&bytes, &scratch.0)?;
    let m = manifest(&scratch.0).map_err(|e| e.replace(&scratch.0.display().to_string(), "the package"))?;
    if m.extension.id != entry.id || m.extension.version != entry.version {
        return Err(format!("the package's own manifest is {} {}, not {}", shown(&m.extension.id), shown(&m.extension.version), entry.label()));
    }
    if m.extension.runtime == "wasm" && !scratch.0.join("extension.wasm").is_file() {
        return Err("a wasm extension's package carries its extension.wasm, and this one doesn't".into());
    }
    if let Some(bad) = [&m.extension.name, &m.extension.description].into_iter().find(|t| shown(t) != **t) {
        return Err(format!("the manifest's name or description has control characters: {}", shown(bad)));
    }
    let tidy = |e: String| e.replace(&scratch.0.display().to_string(), "the package");
    check_chords(&scratch.0).map_err(tidy)?;
    check_agent_tools(&scratch.0).map_err(tidy)?;
    check_contents(&scratch.0)?;
    if let Some(mut known) = known {
        let version = semver::Version::parse(&m.extension.version).map_err(|e| e.to_string())?;
        let k = known.entry(m.extension.id.clone()).or_insert(Known { versions: Vec::new(), dependencies: BTreeMap::new() });
        k.versions.push(version);
        k.dependencies = m.dependencies.clone();
        check_dependencies(&known, &BTreeSet::from([m.extension.id.clone()]))?;
    }
    Ok(Checked { manifest: m, size: bytes.len() as u64 })
}

/// What a reviewer reads of a package: its listing, and what it may do.
fn report(entry: &Entry, checked: &Checked) -> String {
    let m = &checked.manifest;
    let mut out = format!("ok  {} -- {} bytes, sha256 {}\n", entry.label(), checked.size, entry.sha256);
    out += &format!("    name         {}\n    description  {}\n", shown(&m.extension.name), shown(&m.extension.description));
    out += &format!("    repository   {}\n    runtime      {}", shown(&entry.repository), shown(&m.extension.runtime));
    if let Some(api) = &m.engine.api {
        out += &format!(", api {}", shown(api));
    }
    out += &format!(", fenix {}\n    categories   {}\n", shown(&m.engine.fenix), shown(&m.extension.categories.join(", ")));
    let deps: Vec<String> = m.dependencies.iter().map(|(d, r)| format!("{d} {r}")).collect();
    out += &format!("    dependencies {}\n", if deps.is_empty() { "none".to_string() } else { shown(&deps.join(", ")) });
    match m.permissions.as_ref().filter(|p| !p.is_empty()) {
        None => out += "    permissions  none\n",
        Some(p) => {
            out += "    permissions\n";
            for line in toml::to_string(p).unwrap_or_default().lines().filter(|l| !l.trim().is_empty()) {
                out += &format!("      {}\n", shown(line));
            }
        }
    }
    out
}

fn read_index(path: &Path) -> Result<serde_json::Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// The index's release of `id` at `version`, if it has one.
fn released<'a>(index: &'a serde_json::Value, id: &str, version: &str) -> Option<&'a serde_json::Value> {
    ["extensions", "bundles"].iter().filter_map(|k| index[*k].as_array()).flatten().filter(|l| l["id"] == id).filter_map(|l| l["versions"].as_array()).flatten().find(|r| r["version"] == version)
}

/// An entry the index has already: fine when it's the same package, and
/// refused when the entry was changed since -- a release doesn't change.
fn already(index: &serde_json::Value, entry: &Entry) -> Result<bool, String> {
    match released(index, &entry.id, &entry.version) {
        None => Ok(false),
        Some(r) if r["url"] == entry.url.as_str() && r["sha256"] == entry.sha256.as_str() => Ok(true),
        Some(_) => Err(format!("{} is in the index with another url or sha256: a release doesn't change, so give it a new version", entry.label())),
    }
}

/// `cargo xtask external-check`: every entry checked; online, each
/// package downloaded and checked too, every problem told rather than
/// the first.
fn check(root: &Path, offline: bool, index: Option<&Path>, fetch: &dyn Fetch) -> Result<(), String> {
    let entries = entries(root)?;
    let index = index.map(read_index).transpose()?;
    let mut errors = Vec::new();
    if let Some(index) = &index {
        errors.extend(entries.iter().filter_map(|e| already(index, e).err()));
    }
    if offline {
        for entry in &entries {
            println!("ok  {} (not downloaded)", entry.label());
        }
    } else {
        for entry in &entries {
            match inspect(entry, fetch, index.as_ref().map(indexed)) {
                Ok(checked) => print!("{}", report(entry, &checked)),
                Err(e) => errors.push(format!("{}: {e}", entry.label())),
            }
        }
    }
    if !errors.is_empty() {
        return Err(format!("external.toml:\n{}", errors.join("\n")));
    }
    println!("{} external release(s)", entries.len());
    Ok(())
}

/// `cargo xtask index-add-external`: each entry the index hasn't (or the
/// one named), checked and added from its own manifest.
fn add(root: &Path, index: &Path, only: Option<&str>, fetch: &dyn Fetch) -> Result<(), String> {
    let mut entries = entries(root)?;
    if let Some(only) = only {
        entries.retain(|e| format!("{}@{}", e.id, e.version) == only);
        if entries.is_empty() {
            return Err(format!("{} isn't in external.toml", shown(only)));
        }
    }
    let mut added = 0;
    for entry in &entries {
        let doc = if index.exists() { read_index(index)? } else { serde_json::json!({ "format": 1, "extensions": [] }) };
        if already(&doc, entry)? {
            println!("in the index already: {}", entry.label());
            continue;
        }
        let checked = inspect(entry, fetch, Some(indexed(&doc))).map_err(|e| format!("{}: {e}", entry.label()))?;
        print!("{}", report(entry, &checked));
        let mut m = checked.manifest;
        // The listing's repository is the one checked against the
        // publisher, not whatever the package says.
        m.extension.repository = Some(entry.repository.clone());
        index_write(index, &m, &entry.url, &entry.sha256, checked.size)?;
        added += 1;
    }
    println!("{added} external release(s) added");
    Ok(())
}

/// `external-check [--offline] [--index <index.json>]`
pub(crate) fn check_command(root: &Path, args: &[String]) -> Result<(), String> {
    let (mut offline, mut index) = (false, None);
    let mut args = args.iter();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--offline" => offline = true,
            "--index" => index = Some(PathBuf::from(args.next().ok_or("--index <index.json>")?)),
            other => return Err(format!("external-check: {other:?}? -- [--offline] [--index <index.json>]")),
        }
    }
    check(root, offline, index.as_deref(), &Curl)
}

/// `index-add-external <index.json> [--only <id>@<version>]`
pub(crate) fn add_command(root: &Path, args: &[String]) -> Result<(), String> {
    let usage = "index-add-external <index.json> [--only <id>@<version>]";
    let (index, only) = match args {
        [index] => (index, None),
        [index, flag, only] if flag == "--only" => (index, Some(only.as_str())),
        _ => return Err(usage.into()),
    };
    add(root, Path::new(index), only, &Curl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::Write;

    /// Downloads from a map of URLs.
    struct Fake(HashMap<String, Vec<u8>>);

    impl Fetch for Fake {
        fn get(&self, url: &str, limit: u64) -> Result<Vec<u8>, String> {
            let bytes = self.0.get(url).cloned().ok_or_else(|| format!("{url}: 404"))?;
            if bytes.len() as u64 > limit {
                return Err(format!("{url} is more than {limit} bytes"));
            }
            Ok(bytes)
        }
    }

    fn sha(bytes: &[u8]) -> String {
        Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
    }

    /// A zip of these files.
    fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, bytes) in files {
            zip.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    fn manifest_text(id: &str, version: &str, runtime: &str, rest: &str) -> String {
        format!("[extension]\nid = \"{id}\"\nname = \"Dusk\"\nversion = \"{version}\"\ndescription = \"From the package\"\ncategories = [\"themes\"]\nrepository = \"https://example.com/elsewhere\"\nruntime = \"{runtime}\"\n\n[engine]\nfenix = \">=1.5, <2\"\n{rest}")
    }

    const OWNER_URL: &str = "https://github.com/Acme-Org/dusk/releases/download/v1.0.0/acme.dusk-1.0.0.fenixext";

    /// A repository root with these two files, and an `extensions/` of
    /// one in-repo release.
    struct Repo(PathBuf);

    impl Repo {
        fn new(name: &str, external: &str) -> Repo {
            let dir = std::env::temp_dir().join(format!("xtask-ext-test-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(dir.join("extensions/rust")).unwrap();
            std::fs::write(dir.join("extensions/rust/extension.toml"), "[extension]\nid = \"fenix.rust\"\nversion = \"2.0.0\"\n").unwrap();
            std::fs::write(dir.join("publishers.toml"), "[publishers.acme]\nowners = [\"acme-org\", \"alice\"]\n\n[publishers.bob]\nowners = [\"bob\"]\n").unwrap();
            std::fs::write(dir.join("external.toml"), external).unwrap();
            Repo(dir)
        }

        fn index(&self) -> PathBuf {
            self.0.join("index.json")
        }
    }

    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn entry_toml(id: &str, version: &str, url: &str, sha256: &str, repository: &str) -> String {
        format!("[[release]]\nid = \"{id}\"\nversion = \"{version}\"\nurl = \"{url}\"\nsha256 = \"{sha256}\"\nrepository = \"{repository}\"\n\n")
    }

    /// A good package at OWNER_URL, its external.toml entry, and a fetch
    /// that serves it.
    fn good(package: Vec<u8>) -> (String, Fake) {
        let entry = entry_toml("acme.dusk", "1.0.0", OWNER_URL, &sha(&package), "https://github.com/acme-org/dusk");
        (entry, Fake(HashMap::from([(OWNER_URL.to_string(), package)])))
    }

    fn theme_package() -> Vec<u8> {
        let m = manifest_text("acme.dusk", "1.0.0", "none", "\n[permissions]\nnetwork = [\"example.com\"]\n");
        zip_of(&[("extension.toml", m.as_bytes()), ("themes/dusk.toml", b"name = \"Dusk\""), ("README.md", b"# Dusk"), ("screenshots/hero.png", b"png")])
    }

    #[test]
    fn a_good_package_goes_in_from_its_own_manifest() {
        let (entry, fetch) = good(theme_package());
        let repo = Repo::new("good", &entry);
        check(&repo.0, true, None, &fetch).unwrap();
        check(&repo.0, false, None, &fetch).unwrap();
        add(&repo.0, &repo.index(), None, &fetch).unwrap();
        let doc = read_index(&repo.index()).unwrap();
        let listing = &doc["extensions"][0];
        assert_eq!(listing["id"], "acme.dusk");
        assert_eq!(listing["name"], "Dusk");
        assert_eq!(listing["description"], "From the package");
        assert_eq!(listing["repository"], "https://github.com/acme-org/dusk", "external.toml's, not the package's");
        let release = &listing["versions"][0];
        assert_eq!(release["url"], OWNER_URL);
        assert_eq!(release["sha256"], sha(&theme_package()));
        assert_eq!(release["size"], theme_package().len());
        assert_eq!(release["fenix"], ">=1.5, <2");
        assert_eq!(release["runtime"], "none");
        assert_eq!(release["categories"], serde_json::json!(["themes"]));
        assert_eq!(release["permissions"], serde_json::json!({ "network": ["example.com"] }));
        let report = report(&read_external(&repo.0).unwrap()[0], &inspect(&read_external(&repo.0).unwrap()[0], &fetch, None).unwrap());
        assert!(report.contains("network = [\"example.com\"]"), "{report}");

        // Again: it's there, so it's skipped, and the index is as it was.
        let before = std::fs::read_to_string(repo.index()).unwrap();
        add(&repo.0, &repo.index(), None, &Fake(HashMap::new())).unwrap();
        assert_eq!(std::fs::read_to_string(repo.index()).unwrap(), before);
        check(&repo.0, true, Some(&repo.index()), &fetch).unwrap();

        // An entry changed after it was released is refused, not skipped.
        let changed = entry_toml("acme.dusk", "1.0.0", &OWNER_URL.replace("v1.0.0", "v1.0.0-b"), &sha(&theme_package()), "https://github.com/acme-org/dusk");
        std::fs::write(repo.0.join("external.toml"), changed).unwrap();
        assert!(add(&repo.0, &repo.index(), None, &fetch).unwrap_err().contains("a release doesn't change"));
    }

    #[test]
    fn a_wasm_package_carries_its_code_and_resolves_its_dependencies() {
        let m = manifest_text("acme.dusk", "1.0.0", "wasm", "api = \"1.5\"\n\n[dependencies]\n\"fenix.config\" = \"^1\"\n");
        let (entry, fetch) = good(zip_of(&[("extension.toml", m.as_bytes()), ("Cargo.toml", b"[package]")]));
        let repo = Repo::new("wasm", &entry);
        assert!(check(&repo.0, false, None, &fetch).unwrap_err().contains("carries its extension.wasm"));

        let (entry, fetch) = good(zip_of(&[("extension.toml", m.as_bytes()), ("extension.wasm", b"\0asm")]));
        std::fs::write(repo.0.join("external.toml"), entry).unwrap();
        std::fs::write(repo.index(), r#"{"format": 1, "extensions": []}"#).unwrap();
        assert!(add(&repo.0, &repo.index(), None, &fetch).unwrap_err().contains("neither in extensions/ nor in the index"));
        std::fs::write(repo.index(), r#"{"format": 1, "extensions": [{"id": "fenix.config", "name": "Config", "versions": [{"version": "1.2.0"}]}]}"#).unwrap();
        add(&repo.0, &repo.index(), Some("acme.dusk@1.0.0"), &fetch).unwrap();
        let doc = read_index(&repo.index()).unwrap();
        let release = &doc["extensions"][0]["versions"][0];
        assert_eq!((release["api"].as_str(), release["dependencies"]["fenix.config"].as_str()), (Some("1.5"), Some("^1")));
        assert!(add(&repo.0, &repo.index(), Some("acme.dusk@9.0.0"), &fetch).unwrap_err().contains("isn't in external.toml"));
    }

    #[test]
    fn a_package_that_isnt_what_the_entry_says_is_refused() {
        let package = theme_package();
        let repo = Repo::new("mismatch", &entry_toml("acme.dusk", "1.0.0", OWNER_URL, &sha(b"something else"), "https://github.com/acme-org/dusk"));
        let fetch = Fake(HashMap::from([(OWNER_URL.to_string(), package.clone())]));
        assert!(check(&repo.0, false, None, &fetch).unwrap_err().contains("whose SHA-256 is"));
        assert!(add(&repo.0, &repo.index(), None, &fetch).is_err());
        assert!(!repo.index().exists(), "nothing written");

        // The manifest inside says another id, or another version.
        for (id, version) in [("acme.rust", "1.0.0"), ("fenix.dusk", "1.0.0"), ("acme.dusk", "1.0.1")] {
            let (entry, fetch) = good(zip_of(&[("extension.toml", manifest_text(id, version, "none", "").as_bytes())]));
            std::fs::write(repo.0.join("external.toml"), entry).unwrap();
            assert!(check(&repo.0, false, None, &fetch).unwrap_err().contains("the package's own manifest is"), "{id} {version}");
        }
    }

    #[test]
    fn who_may_publish_is_the_publishers_owners() {
        let sha = "a".repeat(64);
        let repo = Repo::new("publishers", "");
        let with = |id: &str, url: &str, repository: &str| {
            std::fs::write(repo.0.join("external.toml"), entry_toml(id, "1.0.0", url, &sha, repository)).unwrap();
            check(&repo.0, true, None, &Fake(HashMap::new()))
        };
        let good_repo = "https://github.com/acme-org/dusk";
        with("acme.dusk", OWNER_URL, good_repo).unwrap();
        with("acme.dusk", "https://github.com/alice/dusk/releases/download/1.0/d.fenixext", "https://github.com/alice/other").unwrap();
        let refused = [
            ("fenix.dusk", OWNER_URL, good_repo, "this repository's own"),
            ("nobody.dusk", OWNER_URL, good_repo, "isn't in publishers.toml"),
            ("acme.dusk", "https://github.com/bob/dusk/releases/download/v1/d.fenixext", "https://github.com/bob/dusk", "is bob's, and publisher acme is acme-org, alice's"),
            ("acme.dusk", "http://github.com/acme-org/dusk/releases/download/v1/d.fenixext", good_repo, "isn't a GitHub release asset"),
            ("acme.dusk", "https://raw.githubusercontent.com/acme-org/dusk/main/d.fenixext", good_repo, "isn't a GitHub release asset"),
            ("acme.dusk", "https://github.com/acme-org/dusk/raw/main/d.fenixext", good_repo, "isn't a GitHub release asset"),
            ("acme.dusk", "https://github.com/acme-org/dusk/releases/latest/download/d.fenixext", good_repo, "isn't a GitHub release asset"),
            ("acme.dusk", "https://github.com/acme-org/dusk/releases/download/v1/../../../../bob/x/releases/download/v1/d.fenixext", good_repo, "isn't a GitHub release asset"),
            ("acme.dusk", "https://github.com/acme-org/dusk/releases/download/v1/d.fenixext?x=1", good_repo, "isn't a GitHub release asset"),
            ("acme.dusk", "https://github.com.evil.example/acme-org/dusk/releases/download/v1/d.fenixext", good_repo, "isn't a GitHub release asset"),
            ("acme.dusk", "https://user@github.com/acme-org/dusk/releases/download/v1/d.fenixext", good_repo, "isn't a GitHub release asset"),
            ("acme.dusk", "https://github.com/acme-org/dusk/releases/download/v1/d.zip", good_repo, "isn't a GitHub release asset"),
            ("acme.dusk", OWNER_URL, "https://github.com/bob/dusk", "isn't acme-org's"),
            ("acme.dusk", OWNER_URL, "https://gitlab.com/acme-org/dusk", "isn't https://github.com/<owner>/<repo>"),
            ("Acme.Dusk", OWNER_URL, good_repo, "isn't publisher.name"),
        ];
        for (id, url, repository, says) in refused {
            let err = with(id, url, repository).unwrap_err();
            assert!(err.contains(says), "{id} {url} {repository}: {err}");
        }

        std::fs::write(repo.0.join("external.toml"), entry_toml("acme.dusk", "1.0.0", OWNER_URL, "ABC", good_repo)).unwrap();
        assert!(check(&repo.0, true, None, &Fake(HashMap::new())).unwrap_err().contains("64 lower case hex"));
        std::fs::write(repo.0.join("publishers.toml"), "[publishers.fenix]\nowners = [\"someone\"]\n").unwrap();
        assert!(read_publishers(&repo.0).unwrap_err().contains("this repository's own"));
        std::fs::write(repo.0.join("publishers.toml"), "[publishers.acme]\nowners = [\"-bad\"]\n").unwrap();
        assert!(read_publishers(&repo.0).unwrap_err().contains("isn't a GitHub user"));
    }

    #[test]
    fn a_version_is_listed_once() {
        let sha = "b".repeat(64);
        let repo_url = "https://github.com/acme-org/dusk";
        let twice = entry_toml("acme.dusk", "1.0.0", OWNER_URL, &sha, repo_url) + &entry_toml("acme.dusk", "1.0.0", &OWNER_URL.replace("v1.0.0", "again"), &sha, repo_url);
        let repo = Repo::new("twice", &twice);
        assert!(check(&repo.0, true, None, &Fake(HashMap::new())).unwrap_err().contains("is there twice"));
        // And not one released from here: a publisher of its own would
        // already stop it, and this says why plainly.
        std::fs::write(repo.0.join("extensions/rust/extension.toml"), "[extension]\nid = \"acme.dusk\"\nversion = \"1.0.0\"\n").unwrap();
        std::fs::write(repo.0.join("external.toml"), entry_toml("acme.dusk", "1.0.0", OWNER_URL, &sha, repo_url)).unwrap();
        assert!(check(&repo.0, true, None, &Fake(HashMap::new())).unwrap_err().contains("is in extensions/"));
    }

    #[test]
    fn a_package_too_big_is_refused_before_its_hash() {
        let huge = vec![0u8; MAX_PACKAGE as usize + 1];
        let (entry, _) = good(huge.clone());
        let repo = Repo::new("huge", &entry);
        let fetch = Fake(HashMap::from([(OWNER_URL.to_string(), huge)]));
        assert!(check(&repo.0, false, None, &fetch).unwrap_err().contains("more than"));
    }

    #[test]
    fn a_package_is_unpacked_defensively() {
        let m = manifest_text("acme.dusk", "1.0.0", "none", "");
        let repo = Repo::new("unpack", "");
        let refused = |package: Vec<u8>, says: &str| {
            let (entry, fetch) = good(package);
            std::fs::write(repo.0.join("external.toml"), entry).unwrap();
            let err = check(&repo.0, false, None, &fetch).unwrap_err();
            assert!(err.contains(says), "{says}: {err}");
        };
        refused(zip_of(&[("extension.toml", m.as_bytes()), ("../../evil.txt", b"x")]), "outside the extension's own folder");
        refused(zip_of(&[("extension.toml", m.as_bytes()), ("/etc/evil", b"x")]), "outside the extension's own folder");
        refused(zip_of(&[("extension.toml", m.as_bytes()), ("themes\\..\\..\\evil", b"x")]), "outside the extension's own folder");
        refused(zip_of(&[("extension.toml", m.as_bytes()), ("C:/evil", b"x")]), "outside the extension's own folder");
        // Two manifests: Fenix reads the first, and the second would be
        // what's checked here if it were written over it.
        refused(zip_of(&[("extension.toml", m.as_bytes()), ("Extension.toml", manifest_text("acme.dusk", "1.0.0", "wasm", "").as_bytes())]), "twice");
        refused(zip_of(&[("themes/dusk.toml", b"name = \"Dusk\"")]), "extension.toml");
        refused(b"not a zip".to_vec(), "isn't a zip");
        refused(zip_of(&[("extension.toml", m.as_bytes()), ("screenshots/hero.bmp", b"bmp")]), "a screenshot is one of");
        refused(zip_of(&[("extension.toml", m.as_bytes()), ("themes/x.toml", b"colour = 1")]), "a theme needs a name");

        // A link.
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        zip.start_file("extension.toml", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(m.as_bytes()).unwrap();
        zip.add_symlink("README.md", "/etc/passwd", zip::write::SimpleFileOptions::default()).unwrap();
        refused(zip.finish().unwrap().into_inner(), "a link");

        // More than it may unpack to: highly compressible, under the
        // download limit, over the unpacked one.
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let deflated = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("extension.toml", deflated).unwrap();
        zip.write_all(m.as_bytes()).unwrap();
        zip.start_file("data/zeros", deflated).unwrap();
        let chunk = vec![0u8; 1024 * 1024];
        for _ in 0..(MAX_UNPACKED / chunk.len() as u64 + 1) {
            zip.write_all(&chunk).unwrap();
        }
        let bomb = zip.finish().unwrap().into_inner();
        assert!((bomb.len() as u64) < MAX_PACKAGE);
        refused(bomb, "unpacks to more than");
    }

    #[test]
    fn printed_text_hides_nothing() {
        assert_eq!(shown("Dusk\u{1b}[2K"), "Dusk\\u{1b}[2K");
        assert_eq!(shown("a\u{202e}b"), "a\\u{202e}b");
        assert_eq!(shown("Café -- ok"), "Café -- ok");
        let m = manifest_text("acme.dusk", "1.0.0", "none", "").replace("From the package", "Fine\\u001b[1A");
        let (entry, fetch) = good(zip_of(&[("extension.toml", m.as_bytes())]));
        let repo = Repo::new("shown", &entry);
        assert!(check(&repo.0, false, None, &fetch).unwrap_err().contains("control characters"));
    }

    #[test]
    fn an_empty_external_toml_is_fine() {
        let repo = Repo::new("empty", "# Nothing yet.\n");
        check(&repo.0, false, None, &Fake(HashMap::new())).unwrap();
        add(&repo.0, &repo.index(), None, &Fake(HashMap::new())).unwrap();
        assert!(!repo.index().exists());
    }

    #[test]
    fn an_in_repo_release_goes_in_the_index_as_it_always_did() {
        let dir = std::env::temp_dir().join(format!("xtask-inrepo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let ext = dir.join("extensions/http");
        std::fs::create_dir_all(&ext).unwrap();
        std::fs::write(ext.join("Cargo.toml"), "[package]\nname = \"http\"\n").unwrap();
        std::fs::write(
            ext.join("extension.toml"),
            "[extension]\nid = \"fenix.http\"\nname = \"HTTP client\"\nversion = \"1.1.0\"\ndescription = \"Requests\"\ncategories = [\"web\", \"tools\"]\nrepository = \"https://github.com/tpedneault/fenix-extensions\"\nruntime = \"wasm\"\n\n[engine]\nfenix = \">=1.5, <2\"\napi = \"1.5\"\n\n[dependencies]\n\"fenix.config\" = \"^1\"\n\n[permissions]\nnetwork = [\"*\"]\nsecrets = [\"http.secret.*\"]\nfilesystem = \"project-read\"\n",
        )
        .unwrap();
        let index = dir.join("index.json");
        crate::index_add(&index, Path::new(&ext), "https://github.com/tpedneault/fenix-extensions/releases/download/http-v1.1.0/fenix.http-1.1.0.fenixext", "ab", 12).unwrap();
        let expected = r#"{
  "format": 1,
  "extensions": [
    {
      "id": "fenix.http",
      "versions": [
        {
          "version": "1.1.0",
          "fenix": ">=1.5, <2",
          "runtime": "wasm",
          "url": "https://github.com/tpedneault/fenix-extensions/releases/download/http-v1.1.0/fenix.http-1.1.0.fenixext",
          "sha256": "ab",
          "size": 12,
          "dependencies": {
            "fenix.config": "^1"
          },
          "categories": [
            "web",
            "tools"
          ],
          "api": "1.5",
          "permissions": {
            "filesystem": "project-read",
            "network": [
              "*"
            ],
            "secrets": [
              "http.secret.*"
            ]
          }
        }
      ],
      "name": "HTTP client",
      "description": "Requests",
      "categories": [
        "web",
        "tools"
      ],
      "repository": "https://github.com/tpedneault/fenix-extensions"
    }
  ]
}
"#;
        let got = std::fs::read_to_string(&index).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(got, expected);
    }
}
