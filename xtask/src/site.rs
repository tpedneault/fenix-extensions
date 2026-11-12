//! `cargo xtask site <index.json> <out-dir>`: the catalogue, a static site
//! of everything the registry's index lists, for GitHub Pages.
//!
//! ```text
//! index.html            every extension and bundle, searchable (the script only filters)
//! <id>/index.html       one's page: install, permissions, versions, and -- for one
//!                       built here -- its screenshots, README and CHANGELOG
//! <id>/screenshots/     its screenshots, the first its hero
//! <id>/media/           the pictures its README shows
//! assets/               the stylesheet, the script, the icon
//! 404.html, .nojekyll
//! ```
//!
//! The index can list packages built by anyone, so every string in it is
//! escaped, a URL from it is a link only when it's http(s), and an id is
//! a folder only when it's an id. The output is the same from the same
//! index and folders: the one date on it is `SOURCE_DATE_EPOCH`'s, and
//! there's none without it.

mod markdown;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use markdown::esc;
use serde_json::Value;

/// This repository: what an entry built here names as its repository.
const HOME: &str = "https://github.com/tpedneault/fenix-extensions";
/// Fenix itself.
const FENIX: &str = "https://github.com/tpedneault/fenix";
/// Where Pages serves the catalogue: what the 404 page links back to,
/// as it's served from any path.
const SITE: &str = "https://tpedneault.github.io/fenix-extensions/";

const STYLE: &str = include_str!("site/style.css");
const SCRIPT: &str = include_str!("site/filter.js");
const ICON: &str = include_str!("site/favicon.svg");
/// Fenix's mark, as its own site draws it.
const MARK: &str = r##"<svg width="26" height="26" viewBox="0 0 48 48" aria-hidden="true" focusable="false"><polygon points="8,45 16,45 16,7 8,11" fill="#D63A2F"/><polygon points="16,15 40,3 40,11 16,23" fill="#FF6A3D"/><polygon points="16,29 32,21 32,29 16,37" fill="#FFB547"/></svg>"##;

/// Ids that would be a page of the site's own.
const RESERVED: &[&str] = &["index.html", "404.html"];

/// One release of an entry, as the index has it.
#[derive(Debug, Clone, Default)]
struct Release {
    version: String,
    semver: Option<semver::Version>,
    fenix: String,
    api: Option<String>,
    runtime: String,
    url: Option<String>,
    sha256: String,
    size: Option<u64>,
    dependencies: BTreeMap<String, String>,
    members: Vec<String>,
    permissions: serde_json::Map<String, Value>,
}

/// An extension or a bundle, as the index has it.
#[derive(Debug, Clone)]
struct Entry {
    id: String,
    name: String,
    description: String,
    categories: Vec<String>,
    repository: Option<String>,
    bundle: bool,
    /// Newest first.
    releases: Vec<Release>,
}

impl Entry {
    fn latest(&self) -> &Release {
        &self.releases[0]
    }
}

/// What this repository has of an entry: its folder, and what's copied
/// from it.
struct Local {
    dir: PathBuf,
    /// The folder's name under `extensions/`.
    name: String,
    screenshots: Vec<String>,
}

fn text(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

fn http(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    (lower.starts_with("https://") || lower.starts_with("http://")) && markdown::safe_url(url)
}

fn release(v: &Value) -> Release {
    let dependencies: BTreeMap<String, String> = v["dependencies"].as_object().map(|d| d.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect()).unwrap_or_default();
    let mut members = strings(&v["bundle"]);
    if members.is_empty() && v["runtime"] == "bundle" {
        members = dependencies.keys().cloned().collect();
    }
    let version = text(&v["version"]);
    Release {
        semver: semver::Version::parse(&version).ok(),
        version,
        fenix: text(&v["fenix"]),
        api: v["api"].as_str().map(str::to_string),
        runtime: text(&v["runtime"]),
        url: v["url"].as_str().filter(|u| http(u)).map(str::to_string),
        sha256: text(&v["sha256"]),
        size: v["size"].as_u64(),
        dependencies,
        members,
        permissions: v["permissions"].as_object().cloned().unwrap_or_default(),
    }
}

/// The index's entries, those with an id that can be a folder and a
/// release; sorted by name.
fn entries(index: &Value) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    let mut seen = BTreeSet::new();
    for (key, bundle) in [("extensions", false), ("bundles", true)] {
        for listing in index[key].as_array().into_iter().flatten() {
            let id = text(&listing["id"]);
            if !crate::valid_id(&id) || RESERVED.contains(&id.as_str()) {
                eprintln!("site: skipped {id:?}: not an id the site can make a page of");
                continue;
            }
            if !seen.insert(id.clone()) {
                eprintln!("site: skipped {id}: listed twice");
                continue;
            }
            let mut releases: Vec<Release> = listing["versions"].as_array().into_iter().flatten().map(release).filter(|r| !r.version.is_empty()).collect();
            if releases.is_empty() {
                eprintln!("site: skipped {id}: no release");
                continue;
            }
            // Newest first; a version that isn't one after those that are.
            releases.sort_by(|a, b| match (&a.semver, &b.semver) {
                (Some(x), Some(y)) => y.cmp(x),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => a.version.cmp(&b.version),
            });
            let name = text(&listing["name"]);
            out.push(Entry {
                name: if name.trim().is_empty() { id.clone() } else { name },
                id,
                description: text(&listing["description"]),
                categories: strings(&listing["categories"]).into_iter().filter(|c| crate::CATEGORIES.contains(&c.as_str())).collect(),
                repository: listing["repository"].as_str().map(str::to_string),
                bundle: bundle || releases[0].runtime == "bundle",
                releases,
            });
        }
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.id.cmp(&b.id)));
    out
}

fn same_repository(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.trim().trim_end_matches('/').trim_end_matches(".git").to_ascii_lowercase();
    norm(a) == norm(b)
}

/// This repository's folders, by their manifest's id.
fn folders(root: &Path) -> BTreeMap<String, (PathBuf, String)> {
    let mut out = BTreeMap::new();
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(root.join("extensions")).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    for dir in dirs {
        match crate::manifest(&dir) {
            Ok(m) => {
                let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                out.entry(m.extension.id).or_insert((dir, name));
            }
            Err(e) => eprintln!("site: {e}"),
        }
    }
    out
}

/// A file name the site may carry as it is.
fn plain_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('.') && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

fn picture(path: &Path) -> bool {
    let kind = path.extension().map(|x| x.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    crate::SCREENSHOTS.contains(&kind.as_str()) && std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.len() <= crate::MAX_SCREENSHOT)
}

fn screenshots(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(dir.join("screenshots"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .filter(|n| plain_name(n) && picture(&dir.join("screenshots").join(n)))
        .collect();
    out.sort();
    out
}

/// A relative path under a folder, `..` and `.` worked out: `None` when
/// it leaves the folder.
fn under(rel: &str) -> Option<Vec<String>> {
    let mut parts: Vec<String> = Vec::new();
    for c in Path::new(rel).components() {
        match c {
            Component::Normal(p) => parts.push(p.to_str()?.to_string()),
            Component::CurDir => {}
            Component::ParentDir => {
                parts.pop()?;
            }
            _ => return None,
        }
    }
    (!parts.is_empty()).then_some(parts)
}

/// "1.0 or later, before 2", from `>=1.0, <2`; the range as it is when
/// it's something else.
fn fenix_long(range: &str) -> String {
    let parts: Vec<&str> = range.split(',').map(str::trim).filter(|p| !p.is_empty()).collect();
    let mut words = Vec::new();
    for p in &parts {
        if let Some(v) = p.strip_prefix(">=") {
            words.push(format!("{} or later", v.trim()));
        } else if let Some(v) = p.strip_prefix('<').filter(|v| !v.starts_with('=')) {
            words.push(format!("before {}", v.trim()));
        } else {
            return range.to_string();
        }
    }
    if words.is_empty() { range.to_string() } else { words.join(", ") }
}

/// "1.0+", from `>=1.0, <2`.
fn fenix_short(range: &str) -> String {
    match range.split(',').map(str::trim).find_map(|p| p.strip_prefix(">=")) {
        Some(v) => format!("{}+", v.trim()),
        None => range.to_string(),
    }
}

fn size(bytes: u64) -> String {
    match bytes {
        b if b < 1000 => format!("{b} bytes"),
        b if b < 1_000_000 => format!("{} KB", (b + 500) / 1000),
        b => format!("{:.1} MB", b as f64 / 1_000_000.0),
    }
}

fn category_name(c: &str) -> String {
    let mut s = c.to_string();
    if let Some(first) = s.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    s
}

/// `SOURCE_DATE_EPOCH`'s day, as 2026-11-12.
fn date(epoch: i64) -> String {
    // Howard Hinnant's days-to-civil.
    let z = epoch.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// What one permission lets it do, in words, and what it names.
fn permission(key: &str, value: &Value) -> Option<(String, String)> {
    let list = strings(value);
    let chips = |items: &[String], gloss: &dyn Fn(&str) -> Option<String>| -> String {
        let mut out = String::from("<ul class=\"chips\">");
        for item in items {
            out.push_str(&format!("<li><code>{}</code>", esc(item)));
            if let Some(g) = gloss(item) {
                out.push_str(&format!(" <span class=\"gloss\">{}</span>", esc(&g)));
            }
            out.push_str("</li>");
        }
        out.push_str("</ul>");
        out
    };
    let setting = |item: &str| item.strip_prefix("${setting:").and_then(|s| s.strip_suffix('}')).map(str::to_string);
    let none = |_: &str| None;
    let s = value.as_str().unwrap_or_default();
    let on = value.as_bool() == Some(true);
    if value.as_bool() == Some(false) || value.as_array().is_some_and(|a| a.is_empty()) || value.as_object().is_some_and(|o| o.is_empty()) || value.is_null() {
        return None;
    }
    let simple = |title: &str| Some((title.to_string(), String::new()));
    match key {
        "editor" if s == "read" => simple("Sees where you are in the editor – the file, the cursor, the selection – when its commands run"),
        "editor" if s == "write" => simple("Sees where you are in the editor, and changes what you're editing"),
        "filesystem" if s == "none" => None,
        "filesystem" if s == "project-read" => simple("Reads the files in your project"),
        "filesystem" if s == "project-write" => simple("Reads and changes the files in your project"),
        "process" if !list.is_empty() => Some((
            "Runs these programs on your computer".into(),
            chips(&list, &|p| {
                if let Some(k) = setting(p) {
                    Some(format!("the program the setting {k} names"))
                } else if p == "/tools/*" {
                    Some("any it installed in its tools folder".into())
                } else if p.starts_with("/data/") {
                    Some("one in its own folder".into())
                } else {
                    None
                }
            }),
        )),
        "network" if list.iter().any(|h| h == "*") => Some(("Makes web (HTTP) requests to any host on the internet".into(), String::new())),
        "network" if !list.is_empty() => Some(("Makes web (HTTP) requests to these hosts".into(), chips(&list, &|h| setting(h).map(|k| format!("those the setting {k} holds"))))),
        "sockets" if !list.is_empty() => Some((
            "Opens network connections (TCP and UDP sockets) to these addresses".into(),
            chips(&list, &|a| {
                if let Some(k) = setting(a) {
                    Some(format!("those the setting {k} holds"))
                } else if a == "tcp://*:*" || a == "*:*" {
                    Some("any host, any port".into())
                } else if a.starts_with("*:") || a.starts_with("tcp://*:") {
                    Some("any host, this port".into())
                } else {
                    None
                }
            }),
        )),
        "listen" if !list.is_empty() => Some(("Listens for connections on these of this computer's ports".into(), chips(&list, &none))),
        "ipc" if !list.is_empty() => Some(("Talks to other programs on this computer through local sockets with these names".into(), chips(&list, &none))),
        "secrets" if !list.is_empty() => Some(("Reads and keeps secrets in Fenix's secret store, under these keys".into(), chips(&list, &none))),
        "env" if !list.is_empty() => Some(("Reads these of Fenix's environment variables".into(), chips(&list, &|e| setting(e).map(|k| format!("those the setting {k} lists"))))),
        "settings" if !list.is_empty() => {
            let prefixes: Vec<String> = list.iter().map(|p| format!("{p}.*")).collect();
            Some(("Reads settings besides its own: those named".into(), chips(&prefixes, &none)))
        }
        "agents" if on => simple("Asks your coding agents for help, and hears what they say when their turns end"),
        "clipboard" if on => simple("Reads the text on your clipboard"),
        "sound" if on => simple("Plays sounds of its own"),
        "fonts" if on => simple("Reads your system's fonts"),
        "draw" if s == "over" => simple("Draws over the editor"),
        "folders" if value.is_object() => {
            let mut out = String::from("<ul class=\"folders\">");
            for (name, spec) in value.as_object().into_iter().flatten() {
                let ways: Vec<String> = spec
                    .as_str()
                    .unwrap_or_default()
                    .split('|')
                    .map(|w| match (setting(w), w.strip_prefix("data:")) {
                        (Some(k), _) => format!("the folder the setting <code>{}</code> names", esc(&k)),
                        (_, Some(d)) => format!("<code>{}</code> in Fenix's data folder", esc(d)),
                        _ => format!("<code>{}</code>", esc(w)),
                    })
                    .collect();
                out.push_str(&format!("<li><code>{}</code>: {}</li>", esc(name), ways.join(", else ")));
            }
            out.push_str("</ul>");
            Some(("Keeps files in folders of yours".into(), out))
        }
        _ => {
            let shown = match value {
                Value::String(s) => s.clone(),
                Value::Array(_) => list.join(", "),
                other => other.to_string(),
            };
            Some((format!("<code>{}</code>", esc(key)), format!("<p class=\"gloss\"><code>{}</code> – Fenix says what this allows before it installs.</p>", esc(&shown))))
        }
    }
    .map(|(title, detail)| if title.starts_with("<code>") { (title, detail) } else { (esc(&title), detail) })
}

/// The order permissions are listed in: what reaches furthest first.
const PERMISSION_ORDER: &[&str] = &["process", "network", "sockets", "listen", "ipc", "filesystem", "folders", "editor", "secrets", "env", "settings", "agents", "clipboard", "draw", "sound", "fonts"];

fn permissions(release: &Release) -> Vec<(String, String)> {
    let mut keys: Vec<&String> = release.permissions.keys().collect();
    keys.sort_by_key(|k| (PERMISSION_ORDER.iter().position(|o| o == k).unwrap_or(usize::MAX), k.to_string()));
    keys.into_iter().filter_map(|k| permission(k, &release.permissions[k])).collect()
}

/// A short name for what it may do, for a bundle's member.
fn permission_short(key: &str) -> &str {
    match key {
        "process" => "runs programs",
        "network" => "web requests",
        "sockets" => "network connections",
        "listen" => "listens on ports",
        "ipc" => "local sockets",
        "filesystem" => "project files",
        "folders" => "folders of yours",
        "editor" => "the editor",
        "secrets" => "secrets",
        "env" => "environment variables",
        "settings" => "other settings",
        "agents" => "coding agents",
        "clipboard" => "the clipboard",
        "draw" => "draws over the editor",
        "sound" => "sound",
        "fonts" => "fonts",
        other => other,
    }
}

struct Site<'a> {
    entries: &'a [Entry],
    by_id: BTreeMap<&'a str, &'a Entry>,
    local: BTreeMap<String, Local>,
    home: &'a str,
    updated: Option<String>,
}

impl Site<'_> {
    /// Whether it's built here: its repository is this one, or it says
    /// none and its folder is here.
    fn is_home(&self, e: &Entry) -> bool {
        match &e.repository {
            Some(r) => same_repository(r, self.home),
            None => self.local.contains_key(&e.id),
        }
    }

    fn local(&self, e: &Entry) -> Option<&Local> {
        self.is_home(e).then(|| self.local.get(&e.id)).flatten()
    }
}

/// A page: its head, the bar along the top, its main and the footer.
fn page(title: &str, description: &str, up: &str, main: &str, script: bool, updated: Option<&str>, home: &str) -> String {
    let script = if script { format!("<script src=\"{up}assets/filter.js\" defer></script>\n") } else { String::new() };
    let updated = updated.map(|d| format!(" Index as of <time datetime=\"{d}\">{d}</time>.")).unwrap_or_default();
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>{title}</title>\n<meta name=\"description\" content=\"{description}\">\n<meta name=\"color-scheme\" content=\"light dark\">\n<link rel=\"icon\" href=\"{up}assets/favicon.svg\" type=\"image/svg+xml\">\n<link rel=\"stylesheet\" href=\"{up}assets/style.css\">\n{script}</head>\n<body>\n<a class=\"skip\" href=\"#main\">Skip to the content</a>\n<header class=\"top\">\n<div class=\"wrap bar\">\n<a class=\"brand\" href=\"{up}index.html\">{MARK}<span class=\"word\">fenix</span><span class=\"sub\">extensions</span></a>\n<nav aria-label=\"Elsewhere\"><a href=\"{FENIX}\">Fenix</a><a href=\"{home}\">Source</a></nav>\n</div>\n</header>\n<main id=\"main\" class=\"wrap\">\n{main}</main>\n<footer class=\"foot\">\n<div class=\"wrap\">\n<p>Every extension here installs from inside Fenix: <kbd>SPC</kbd> <kbd>h</kbd> <kbd>x</kbd>. Made from the registry's signed index.{updated}</p>\n<p><a href=\"{home}\">fenix-extensions</a> · MIT OR Apache-2.0</p>\n</div>\n</footer>\n</body>\n</html>\n",
        title = esc(title),
        description = esc(description),
        home = esc(home),
    )
}

fn tags(categories: &[String], up: &str, linked: bool) -> String {
    if categories.is_empty() {
        return String::new();
    }
    let mut out = String::from("<ul class=\"tags\" aria-label=\"Categories\">");
    for c in categories {
        if linked {
            out.push_str(&format!("<li><a href=\"{up}index.html?category={c}\">{}</a></li>", esc(&category_name(c))));
        } else {
            out.push_str(&format!("<li>{}</li>", esc(&category_name(c))));
        }
    }
    out.push_str("</ul>");
    out
}

fn card(site: &Site, e: &Entry) -> String {
    let latest = e.latest();
    let mut search = vec![e.name.clone(), e.id.clone(), e.description.clone()];
    search.extend(e.categories.iter().cloned());
    for m in &latest.members {
        search.push(m.clone());
        if let Some(member) = site.by_id.get(m.as_str()) {
            search.push(member.name.clone());
        }
    }
    let search = search.join(" ").to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ");
    let shot = site
        .local(e)
        .and_then(|l| l.screenshots.first())
        .map(|s| format!("<img class=\"shot\" src=\"{id}/screenshots/{s}\" alt=\"\" loading=\"lazy\">", id = e.id, s = esc(s)))
        .unwrap_or_default();
    let mut meta = vec![format!("<span>v{}</span>", esc(&latest.version))];
    if !latest.fenix.is_empty() {
        meta.push(format!("<span title=\"Fenix {}\">Fenix {}</span>", esc(&latest.fenix), esc(&fenix_short(&latest.fenix))));
    }
    if e.bundle {
        meta.push(format!("<span>{} extension{}</span>", latest.members.len(), if latest.members.len() == 1 { "" } else { "s" }));
    }
    if !site.is_home(e) {
        meta.push("<span class=\"badge\">Built elsewhere</span>".into());
    }
    format!(
        "<li class=\"card\" data-search=\"{search}\" data-categories=\"{cats}\">\n{shot}<div class=\"body\">\n<h3><a href=\"{id}/index.html\">{name}</a></h3>\n<p class=\"id\">{id}</p>\n<p class=\"desc\">{desc}</p>\n<p class=\"meta\">{meta}</p>\n{tags}\n</div>\n</li>\n",
        search = esc(&search),
        cats = e.categories.join(" "),
        id = e.id,
        name = esc(&e.name),
        desc = esc(&e.description),
        meta = meta.join(""),
        tags = tags(&e.categories, "", false),
    )
}

fn index_page(site: &Site) -> String {
    let bundles: Vec<&Entry> = site.entries.iter().filter(|e| e.bundle).collect();
    let extensions: Vec<&Entry> = site.entries.iter().filter(|e| !e.bundle).collect();
    let mut main = String::new();
    let count = |n: usize, one: &str, many: &str| if n == 1 { format!("1 {one}") } else { format!("{n} {many}") };
    main.push_str(&format!(
        "<section class=\"hero\" aria-labelledby=\"title\">\n<h1 id=\"title\">Extensions for Fenix</h1>\n<p class=\"lead\">Languages, tools, viewers, themes and more for the keyboard-first editor. Each one installs from inside Fenix: press <kbd>SPC</kbd> <kbd>h</kbd> <kbd>x</kbd>, search for it, and install. Fenix shows what it may do first.</p>\n<p class=\"counts\">{} and {}</p>\n</section>\n",
        count(extensions.len(), "extension", "extensions"),
        count(bundles.len(), "bundle", "bundles")
    ));
    // The filters: hidden until the script shows them, as without it they'd do nothing.
    let mut cats: BTreeMap<&str, usize> = BTreeMap::new();
    for e in site.entries {
        for c in &e.categories {
            *cats.entry(c.as_str()).or_default() += 1;
        }
    }
    main.push_str("<form id=\"filters\" class=\"filters\" role=\"search\" aria-label=\"Extensions\" hidden>\n<div class=\"search\">\n<label for=\"q\">Search</label>\n<input id=\"q\" name=\"q\" type=\"search\" placeholder=\"Name, id or what it does\" autocomplete=\"off\" spellcheck=\"false\" aria-describedby=\"shown\">\n</div>\n<fieldset class=\"cats\">\n<legend>Category</legend>\n<input type=\"radio\" name=\"category\" id=\"cat-all\" value=\"\" checked><label for=\"cat-all\">All</label>\n");
    for c in crate::CATEGORIES.iter().filter(|c| cats.contains_key(**c)) {
        main.push_str(&format!("<input type=\"radio\" name=\"category\" id=\"cat-{c}\" value=\"{c}\"><label for=\"cat-{c}\">{} <span class=\"n\">{}</span></label>\n", category_name(c), cats[c]));
    }
    main.push_str("</fieldset>\n<p id=\"shown\" class=\"shown\" aria-live=\"polite\"></p>\n</form>\n");
    for (id, title, lead, list) in [
        ("bundles", "Bundles", "Extensions that go together, installed as one: what the welcome's packs offer.", &bundles),
        ("extensions", "Extensions", "", &extensions),
    ] {
        if list.is_empty() {
            continue;
        }
        main.push_str(&format!("<section class=\"group\" id=\"{id}\" aria-labelledby=\"{id}-h\">\n<h2 id=\"{id}-h\">{title}</h2>\n"));
        if !lead.is_empty() {
            main.push_str(&format!("<p class=\"group-lead\">{lead}</p>\n"));
        }
        main.push_str("<ul class=\"cards\">\n");
        for e in list.iter() {
            main.push_str(&card(site, e));
        }
        main.push_str("</ul>\n</section>\n");
    }
    main.push_str("<p id=\"none\" class=\"none\" hidden>Nothing matches. Try fewer words, or another category.</p>\n");
    page("Fenix extensions", "Every extension and bundle in Fenix's store: what each does, what it may do, and the Fenix it needs.", "", &main, true, site.updated.as_deref(), site.home)
}

/// A caption from a screenshot's file name: `02-the-board.png`, "the board".
fn caption(file: &str) -> String {
    let stem = Path::new(file).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let stem = stem.trim_start_matches(|c: char| c.is_ascii_digit()).trim_start_matches(['-', '_', ' ']);
    stem.replace(['-', '_'], " ")
}

/// The pictures and links a README points at, worked out for its page.
struct Media<'a> {
    local: &'a Local,
    home: &'a str,
    copies: RefCell<BTreeSet<Vec<String>>>,
}

impl Media<'_> {
    fn link(&self, url: &str) -> Option<String> {
        let (path, rest) = match url.find(['#', '?']) {
            Some(at) => (&url[..at], &url[at..]),
            None => (url, ""),
        };
        let mut parts = vec!["extensions".to_string(), self.local.name.clone()];
        for c in Path::new(path).components() {
            match c {
                Component::Normal(p) => parts.push(p.to_str()?.to_string()),
                Component::CurDir => {}
                Component::ParentDir => {
                    parts.pop()?;
                }
                _ => return None,
            }
        }
        Some(format!("{}/blob/main/{}{rest}", self.home.trim_end_matches('/'), parts.join("/")))
    }

    fn image(&self, url: &str) -> Option<String> {
        if url.contains(['#', '?']) {
            return None;
        }
        let parts = under(url)?;
        if !parts.iter().all(|p| plain_name(p)) || !picture(&self.local.dir.join(parts.join("/"))) {
            return None;
        }
        let src = format!("media/{}", parts.join("/"));
        self.copies.borrow_mut().insert(parts);
        Some(src)
    }
}

/// A Markdown file of its folder, rendered, its first `#` heading left
/// out (the page has its own).
fn document(path: &Path, media: &Media, prefix: &str) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut lines: Vec<&str> = text.lines().collect();
    if let Some(first) = lines.iter().position(|l| !l.trim().is_empty()) {
        if lines[first].starts_with("# ") {
            lines.drain(..=first);
        }
    }
    let link = |u: &str| media.link(u);
    let image = |u: &str| media.image(u);
    let html = markdown::render(&lines.join("\n"), &markdown::Options { id_prefix: prefix, shift: 1, link: &link, image: &image });
    (!html.trim().is_empty()).then_some(html)
}

fn entry_link(site: &Site, id: &str) -> String {
    match site.by_id.get(id) {
        Some(e) => format!("<a href=\"../{}/index.html\">{}</a> <code>{}</code>", e.id, esc(&e.name), esc(id)),
        None => format!("<code>{}</code> <span class=\"gloss\">(not in the index)</span>", esc(id)),
    }
}

/// An entry's page, and the files it copies from its folder (relative
/// to it, under the page's folder).
fn entry_page(site: &Site, e: &Entry) -> (String, Vec<(PathBuf, PathBuf)>) {
    let latest = e.latest();
    let local = site.local(e);
    let home = site.is_home(e);
    let mut copies: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut m = String::new();
    m.push_str("<nav class=\"crumbs\" aria-label=\"Breadcrumb\"><a href=\"../index.html\">All extensions</a></nav>\n");
    m.push_str("<div class=\"head\">\n");
    m.push_str(&format!("<h1>{}</h1>\n<p class=\"id\"><code>{}</code>", esc(&e.name), e.id));
    if e.bundle {
        m.push_str(" <span class=\"badge\">Bundle</span>");
    }
    if !home {
        m.push_str(" <span class=\"badge\">Built elsewhere</span>");
    }
    m.push_str("</p>\n");
    if !e.description.is_empty() {
        m.push_str(&format!("<p class=\"lead\">{}</p>\n", esc(&e.description)));
    }
    m.push_str(&tags(&e.categories, "../", true));
    m.push_str("\n</div>\n");

    let shots = local.map(|l| l.screenshots.clone()).unwrap_or_default();
    if let (Some(l), false) = (local, shots.is_empty()) {
        for s in &shots {
            copies.push((l.dir.join("screenshots").join(s), PathBuf::from("screenshots").join(s)));
        }
        let first = &shots[0];
        let cap = caption(first);
        m.push_str(&format!("<figure class=\"hero-shot\"><img src=\"screenshots/{s}\" alt=\"{alt}\"><figcaption>{cap}</figcaption></figure>\n", s = esc(first), alt = esc(&format!("{}: {cap}", e.name)), cap = esc(&cap)));
    }

    // At a glance.
    let mut facts = String::from("<aside class=\"facts\" aria-labelledby=\"facts-h\">\n<h2 id=\"facts-h\">At a glance</h2>\n<dl>\n");
    facts.push_str(&format!("<dt>Latest version</dt><dd>{}</dd>\n", esc(&latest.version)));
    if !latest.fenix.is_empty() {
        facts.push_str(&format!("<dt>Needs Fenix</dt><dd>{} <code>{}</code></dd>\n", esc(&fenix_long(&latest.fenix)), esc(&latest.fenix)));
    }
    if let Some(api) = &latest.api {
        facts.push_str(&format!("<dt>Extension API</dt><dd>{} <span class=\"gloss\">– a Fenix that offers it or a later 1.x runs it</span></dd>\n", esc(api)));
    }
    let kind = match latest.runtime.as_str() {
        "none" => "Data: no code of its own".to_string(),
        "wasm" => "Code, run in Fenix's sandbox (WebAssembly)".to_string(),
        "lua" => "Code, run in Fenix's sandbox (Lua)".to_string(),
        "bundle" => "A bundle: installs its members".to_string(),
        other => other.to_string(),
    };
    facts.push_str(&format!("<dt>What it is</dt><dd>{}</dd>\n", esc(&kind)));
    if let Some(bytes) = latest.size {
        facts.push_str(&format!("<dt>Download</dt><dd>{}</dd>\n", size(bytes)));
    }
    match (&e.repository, home) {
        (Some(r), _) if http(r) => facts.push_str(&format!("<dt>Repository</dt><dd><a href=\"{r}\">{}</a></dd>\n", esc(r.trim_start_matches("https://").trim_start_matches("http://")), r = esc(r))),
        (Some(r), _) => facts.push_str(&format!("<dt>Repository</dt><dd>{}</dd>\n", esc(r))),
        (None, _) => {}
    }
    if let Some(l) = local {
        facts.push_str(&format!("<dt>Source</dt><dd><a href=\"{}/tree/main/extensions/{}\">extensions/{}</a></dd>\n", esc(site.home.trim_end_matches('/')), esc(&l.name), esc(&l.name)));
    }
    let part_of: Vec<&Entry> = site.entries.iter().filter(|b| b.bundle && b.latest().members.contains(&e.id)).collect();
    if !part_of.is_empty() {
        let links: Vec<String> = part_of.iter().map(|b| format!("<a href=\"../{}/index.html\">{}</a>", b.id, esc(&b.name))).collect();
        facts.push_str(&format!("<dt>In the bundles</dt><dd>{}</dd>\n", links.join(", ")));
    }
    facts.push_str("</dl>\n</aside>\n");

    let mut c = String::new();
    // Install.
    c.push_str("<section aria-labelledby=\"install\">\n<h2 id=\"install\">Install</h2>\n<ol class=\"steps\">\n");
    c.push_str(&format!("<li>In Fenix, press <kbd>SPC</kbd> <kbd>h</kbd> <kbd>x</kbd> to open the Extensions page{}.</li>\n", if e.bundle { ", and its Bundles tab" } else { "" }));
    c.push_str(&format!("<li>Search for <strong>{}</strong>, and install it. Fenix shows what it may do before it installs anything.</li>\n</ol>\n", esc(&e.name)));
    if let Some(url) = &latest.url {
        c.push_str(&format!("<p class=\"aside\">Or <a href=\"{}\">download the package</a> and run <code>fenix --install-extension {}-{}.fenixext</code>.</p>\n", esc(url), e.id, esc(&latest.version)));
    }
    if !home {
        c.push_str("<p class=\"note\">Built elsewhere: its author builds and hosts it in their own repository, and this page shows only what the registry's index says about it. Fenix checks the package against the hash in the signed index, as it does for every package.</p>\n");
    }
    c.push_str("</section>\n");

    // Permissions.
    c.push_str("<section aria-labelledby=\"permissions\">\n<h2 id=\"permissions\">What it may do</h2>\n");
    let perms = permissions(latest);
    if e.bundle {
        c.push_str("<p>Nothing of its own: a bundle runs no code. Each extension it installs asks for its own permissions, below, and Fenix shows them before installing.</p>\n");
    } else if perms.is_empty() {
        c.push_str(if latest.runtime == "none" { "<p>Nothing: it's data (themes, snippets, keymaps, templates) and runs no code.</p>\n" } else { "<p>Nothing beyond running its own code in Fenix's sandbox: no programs, no network, no files outside its own.</p>\n" });
    } else {
        c.push_str("<p>Fenix holds it to these, and shows them before installing it. Everything else is closed to it.</p>\n<dl class=\"perms\">\n");
        for (title, detail) in &perms {
            c.push_str(&format!("<div><dt>{title}</dt>{}</div>\n", if detail.is_empty() { String::new() } else { format!("<dd>{detail}</dd>") }));
        }
        c.push_str("</dl>\n");
    }
    c.push_str("</section>\n");

    // A bundle's members, or what it depends on.
    if e.bundle {
        c.push_str("<section aria-labelledby=\"members\">\n<h2 id=\"members\">What it installs</h2>\n<ul class=\"members\">\n");
        for id in &latest.members {
            c.push_str("<li>");
            match site.by_id.get(id.as_str()) {
                Some(member) => {
                    c.push_str(&format!("<p class=\"member-name\"><a href=\"../{}/index.html\">{}</a> <code>{}</code>", member.id, esc(&member.name), member.id));
                    if let Some(range) = latest.dependencies.get(id) {
                        c.push_str(&format!(" <span class=\"gloss\">{}</span>", esc(range)));
                    }
                    c.push_str("</p>");
                    if !member.description.is_empty() {
                        c.push_str(&format!("<p>{}</p>", esc(&member.description)));
                    }
                    let mut keys: Vec<&String> = member.latest().permissions.keys().filter(|k| permission(k, &member.latest().permissions[*k]).is_some()).collect();
                    keys.sort_by_key(|k| (PERMISSION_ORDER.iter().position(|o| o == k).unwrap_or(usize::MAX), k.to_string()));
                    let words: Vec<String> = keys.iter().map(|k| esc(permission_short(k))).collect();
                    c.push_str(&format!("<p class=\"gloss\">May use: {}</p>", if words.is_empty() { "nothing beyond its own code".into() } else { words.join(", ") }));
                }
                None => c.push_str(&entry_link(site, id)),
            }
            c.push_str("</li>\n");
        }
        c.push_str("</ul>\n</section>\n");
    } else if !latest.dependencies.is_empty() {
        c.push_str("<section aria-labelledby=\"dependencies\">\n<h2 id=\"dependencies\">What it needs</h2>\n<p>Installing it installs these too.</p>\n<ul class=\"deps\">\n");
        for (id, range) in &latest.dependencies {
            c.push_str(&format!("<li>{} <span class=\"gloss\">{}</span></li>\n", entry_link(site, id), esc(range)));
        }
        c.push_str("</ul>\n</section>\n");
    }

    // Its README, and its other screenshots.
    let media = local.map(|l| Media { local: l, home: site.home, copies: RefCell::new(BTreeSet::new()) });
    if let Some(media) = &media {
        if let Some(html) = document(&media.local.dir.join("README.md"), media, "readme-") {
            c.push_str(&format!("<section class=\"doc\" aria-labelledby=\"readme\">\n<h2 id=\"readme\">About</h2>\n{html}</section>\n"));
        }
    }
    if shots.len() > 1 {
        c.push_str("<section aria-labelledby=\"screenshots\">\n<h2 id=\"screenshots\">Screenshots</h2>\n<div class=\"gallery\">\n");
        for s in &shots[1..] {
            let cap = caption(s);
            c.push_str(&format!("<figure><a href=\"screenshots/{s}\"><img src=\"screenshots/{s}\" alt=\"{alt}\" loading=\"lazy\"></a><figcaption>{cap}</figcaption></figure>\n", s = esc(s), alt = esc(&format!("{}: {cap}", e.name)), cap = esc(&cap)));
        }
        c.push_str("</div>\n</section>\n");
    }

    // Versions.
    c.push_str("<section aria-labelledby=\"versions\">\n<h2 id=\"versions\">Versions</h2>\n<div class=\"table\"><table>\n<thead><tr><th scope=\"col\">Version</th><th scope=\"col\">Fenix</th><th scope=\"col\">API</th><th scope=\"col\">Size</th><th scope=\"col\">Package</th></tr></thead>\n<tbody>\n");
    for r in &e.releases {
        let package = match &r.url {
            Some(u) => format!("<a href=\"{}\">Download<span class=\"visually-hidden\"> {}</span></a>", esc(u), esc(&r.version)),
            None => String::new(),
        };
        let sha = if r.sha256.is_empty() { String::new() } else { format!("<code class=\"sha\" title=\"SHA-256 {}\">{}…</code>", esc(&r.sha256), esc(&r.sha256.chars().take(10).collect::<String>())) };
        c.push_str(&format!(
            "<tr><th scope=\"row\">{}</th><td><code>{}</code></td><td>{}</td><td>{}</td><td>{package}{sha}</td></tr>\n",
            esc(&r.version),
            esc(&r.fenix),
            r.api.as_deref().map(esc).unwrap_or_else(|| "--".into()),
            r.size.map(size).unwrap_or_default()
        ));
    }
    c.push_str("</tbody>\n</table></div>\n</section>\n");

    if let Some(media) = &media {
        if let Some(html) = document(&media.local.dir.join("CHANGELOG.md"), media, "changelog-") {
            c.push_str(&format!("<section class=\"doc\" aria-labelledby=\"changelog\">\n<h2 id=\"changelog\">Changelog</h2>\n{html}</section>\n"));
        }
        for parts in media.copies.borrow().iter() {
            let rel = parts.join("/");
            copies.push((media.local.dir.join(&rel), PathBuf::from("media").join(&rel)));
        }
    }

    m.push_str(&format!("<div class=\"layout\">\n{facts}<div class=\"content\">\n{c}</div>\n</div>\n"));
    let description = if e.description.is_empty() { format!("{} for Fenix", e.name) } else { e.description.clone() };
    (page(&format!("{} · Fenix extensions", e.name), &description, "../", &m, false, site.updated.as_deref(), site.home), copies)
}

fn not_found(home: &str) -> String {
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>Not found · Fenix extensions</title>\n<meta name=\"color-scheme\" content=\"light dark\">\n<style>\n{STYLE}</style>\n</head>\n<body>\n<header class=\"top\">\n<div class=\"wrap bar\">\n<a class=\"brand\" href=\"{SITE}\">{MARK}<span class=\"word\">fenix</span><span class=\"sub\">extensions</span></a>\n</div>\n</header>\n<main id=\"main\" class=\"wrap\">\n<section class=\"hero\" aria-labelledby=\"title\">\n<h1 id=\"title\">Nothing here</h1>\n<p class=\"lead\">This page isn't in the catalogue: the extension may have a new id, or the link may be wrong.</p>\n<p><a href=\"{SITE}\">Every extension</a> · <a href=\"{}\">The repository</a></p>\n</section>\n</main>\n</body>\n</html>\n",
        esc(home)
    )
}

fn write(out: &Path, rel: &str, bytes: &[u8]) -> Result<(), String> {
    let path = out.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// The site from this index and the folders under `root/extensions`,
/// into `out`; how many pages it made.
fn generate(root: &Path, index: &Value, out: &Path, home: &str, updated: Option<String>) -> Result<usize, String> {
    let entries = entries(index);
    let by_id = entries.iter().map(|e| (e.id.as_str(), e)).collect();
    let local = folders(root).into_iter().map(|(id, (dir, name))| (id, Local { screenshots: screenshots(&dir), dir, name })).collect();
    let site = Site { entries: &entries, by_id, local, home, updated };
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    write(out, "index.html", index_page(&site).as_bytes())?;
    write(out, "404.html", not_found(home).as_bytes())?;
    write(out, ".nojekyll", b"")?;
    write(out, "assets/style.css", STYLE.as_bytes())?;
    write(out, "assets/filter.js", SCRIPT.as_bytes())?;
    write(out, "assets/favicon.svg", ICON.as_bytes())?;
    let mut pages = 2;
    for e in &entries {
        let (html, copies) = entry_page(&site, e);
        write(out, &format!("{}/index.html", e.id), html.as_bytes())?;
        for (from, to) in copies {
            let bytes = std::fs::read(&from).map_err(|err| format!("{}: {err}", from.display()))?;
            write(&out.join(&e.id), &to.to_string_lossy(), &bytes)?;
        }
        pages += 1;
    }
    Ok(pages)
}

/// `cargo xtask site <index.json> <out-dir>`.
pub(crate) fn site(root: &Path, index: &Path, out: &Path) -> Result<(), String> {
    if index.as_os_str().is_empty() || out.as_os_str().is_empty() {
        return Err("cargo xtask site <index.json> <out-dir>".into());
    }
    let text = std::fs::read_to_string(index).map_err(|e| format!("{}: {e}", index.display()))?;
    let doc: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", index.display()))?;
    let updated = match std::env::var("SOURCE_DATE_EPOCH") {
        Ok(s) if !s.trim().is_empty() => Some(date(s.trim().parse::<i64>().map_err(|e| format!("SOURCE_DATE_EPOCH: {e}"))?)),
        _ => None,
    };
    let pages = generate(root, &doc, out, HOME, updated)?;
    println!("{pages} pages in {}", out.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Temp(PathBuf);

    impl Temp {
        fn new(name: &str) -> Temp {
            let dir = std::env::temp_dir().join(format!("xtask-site-{name}-{}", std::process::id()));
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

    /// Every `href` and `src` in the site's pages that isn't http(s): the
    /// file it names (and its `#anchor`, an id there) is in the site.
    pub(super) fn broken_links(out: &Path) -> Vec<String> {
        let mut broken = Vec::new();
        let pages: Vec<PathBuf> = crate::files(out).into_iter().filter(|f| f.extension().is_some_and(|x| x == "html")).collect();
        for rel in &pages {
            let html = std::fs::read_to_string(out.join(rel)).unwrap();
            for attr in ["href=\"", "src=\""] {
                for piece in html.split(attr).skip(1) {
                    let url = piece.split('"').next().unwrap().replace("&amp;", "&");
                    if url.starts_with("https://") || url.starts_with("http://") {
                        continue;
                    }
                    let (path, anchor) = match url.split_once('#') {
                        Some((p, a)) => (p.to_string(), Some(a.to_string())),
                        None => (url.clone(), None),
                    };
                    let path = path.split('?').next().unwrap().to_string();
                    let target = if path.is_empty() { out.join(rel) } else { out.join(rel).parent().unwrap().join(&path) };
                    let target = if target.is_dir() { target.join("index.html") } else { target };
                    let mut parts: Vec<String> = Vec::new();
                    for c in target.strip_prefix(out).map(|p| p.components().collect::<Vec<_>>()).unwrap_or_default() {
                        match c {
                            Component::ParentDir => {
                                parts.pop();
                            }
                            Component::Normal(p) => parts.push(p.to_string_lossy().into_owned()),
                            _ => {}
                        }
                    }
                    let resolved = out.join(parts.join("/"));
                    if !resolved.is_file() || !resolved.starts_with(out) {
                        broken.push(format!("{}: {url}", rel.display()));
                        continue;
                    }
                    if let Some(a) = anchor.filter(|a| !a.is_empty() && a != "main") {
                        let page = std::fs::read_to_string(&resolved).unwrap_or_default();
                        if !page.contains(&format!("id=\"{a}\"")) {
                            broken.push(format!("{}: {url} (no such id)", rel.display()));
                        }
                    }
                }
            }
        }
        broken
    }

    fn fixture(t: &Temp) -> (PathBuf, Value) {
        let root = t.0.join("repo");
        let dir = root.join("extensions/notes");
        std::fs::create_dir_all(dir.join("screenshots")).unwrap();
        std::fs::create_dir_all(dir.join("img")).unwrap();
        std::fs::write(dir.join("extension.toml"), "[extension]\nid = \"acme.notes\"\nname = \"Notes\"\nversion = \"1.2.0\"\nruntime = \"none\"\n\n[engine]\nfenix = \">=1.0, <2\"\n").unwrap();
        std::fs::write(dir.join("README.md"), "# Notes\n\nNotes, [kept](docs/how.md) [here](#keys).\n\n## Keys\n\n![a plot](img/plot.png) ![gone](img/none.png) ![up](../../secret.png)\n").unwrap();
        std::fs::write(dir.join("CHANGELOG.md"), "# Changelog\n\n## 1.2.0\n\n- <b>Bold</b> claims.\n").unwrap();
        std::fs::write(dir.join("screenshots/02-the-list.png"), "png").unwrap();
        std::fs::write(dir.join("screenshots/01-board.png"), "png").unwrap();
        std::fs::write(dir.join("screenshots/notes.txt"), "not a picture").unwrap();
        std::fs::write(dir.join("img/plot.png"), "png").unwrap();
        std::fs::write(root.join("secret.png"), "png").unwrap();
        let index = serde_json::json!({ "format": 1,
            "extensions": [
                { "id": "acme.notes", "name": "Notes", "description": "Notes & <i>more</i>", "categories": ["tools", "made-up"], "repository": HOME,
                  "versions": [
                    { "version": "1.10.0", "fenix": ">=1.5, <2", "runtime": "none", "url": "https://example.com/n-1.10.0.fenixext", "sha256": "abcdef0123456789", "size": 2048 },
                    { "version": "1.2.0", "fenix": ">=1.0, <2", "runtime": "none", "url": "javascript:alert(1)", "size": 1024 }
                  ] },
                { "id": "evil.tool", "name": "<script>alert('x')</script>", "description": "\"><img src=x onerror=alert(1)>", "categories": ["web"], "repository": "javascript:alert(1)",
                  "versions": [{ "version": "0.1.0", "fenix": ">=1.0\"><b>", "runtime": "wasm", "api": "1.0", "url": "https://evil.example/x.fenixext",
                    "dependencies": { "acme.notes": "^1", "acme.gone": "*" },
                    "permissions": { "process": ["sh", "${setting:evil.tool}", "/tools/*"], "network": ["*"], "secrets": ["evil.*"], "folders": { "dump": "${setting:evil.dir}|data:evil" }, "agents": true, "clipboard": false, "weird<x>": "\"y\"" } }] },
                { "id": "../../escape", "name": "Escape", "versions": [{ "version": "1.0.0" }] },
                { "id": "index.html", "name": "Index", "versions": [{ "version": "1.0.0" }] },
                { "id": "acme.empty", "name": "No releases", "versions": [] }
            ],
            "bundles": [
                { "id": "acme.pack", "name": "Pack", "description": "Notes and the tool", "categories": ["tools"], "repository": format!("{HOME}/"),
                  "versions": [{ "version": "1.0.0", "fenix": ">=1.0, <2", "runtime": "bundle", "dependencies": { "acme.notes": "^1", "evil.tool": "*" }, "bundle": ["acme.notes", "evil.tool"] }] }
            ] });
        (root, index)
    }

    #[test]
    fn the_site_is_whole_safe_and_the_same_each_time() {
        let t = Temp::new("whole");
        let (root, index) = fixture(&t);
        let out = t.0.join("out");
        let pages = generate(&root, &index, &out, HOME, Some(date(1_778_544_000))).unwrap();
        assert_eq!(pages, 2 + 3, "index, 404, and notes, tool, pack");
        for f in [".nojekyll", "404.html", "assets/style.css", "assets/filter.js", "assets/favicon.svg", "acme.notes/index.html", "evil.tool/index.html", "acme.pack/index.html", "acme.notes/screenshots/01-board.png", "acme.notes/media/img/plot.png"] {
            assert!(out.join(f).is_file(), "{f}");
        }
        assert!(!out.join("acme.notes/screenshots/notes.txt").exists() && !t.0.join("escape").exists() && !out.join("acme.notes/media/secret.png").exists());
        assert_eq!(broken_links(&out), Vec::<String>::new());

        let list = std::fs::read_to_string(out.join("index.html")).unwrap();
        assert!(list.contains("Index as of <time datetime=\"2026-05-12\">2026-05-12</time>"));
        let notes = list.find("acme.notes/index.html").unwrap();
        assert!(list.find("evil.tool/index.html").unwrap() < notes, "by name: < before N");
        assert!(list.contains("<img class=\"shot\" src=\"acme.notes/screenshots/01-board.png\""), "the first screenshot on its card");
        assert!(list.contains("<span>v1.10.0</span>"), "the newest by semver, not by text");
        assert!(list.contains("data-categories=\"tools\""), "an unknown category is left out");
        assert!(list.contains("<input type=\"radio\" name=\"category\" id=\"cat-web\""));

        let evil = std::fs::read_to_string(out.join("evil.tool/index.html")).unwrap();
        for bad in ["<script>alert", "<img src=x", "<b>", "=\"javascript:", "<i>", "<x>"] {
            assert!(!evil.contains(bad) && !list.contains(bad), "{bad}");
        }
        assert!(evil.contains("&lt;script&gt;alert(&#39;x&#39;)&lt;/script&gt;"));
        assert!(evil.contains("Built elsewhere"));
        assert!(evil.contains("Runs these programs on your computer") && evil.contains("the program the setting evil.tool names"));
        assert!(evil.contains("Makes web (HTTP) requests to any host on the internet"));
        assert!(evil.contains("Asks your coding agents"));
        assert!(!evil.contains("clipboard"));
        assert!(evil.contains("the folder the setting <code>evil.dir</code> names, else <code>evil</code> in Fenix's data folder"));
        assert!(evil.contains("<code>weird&lt;x&gt;</code>"));
        assert!(evil.contains("<a href=\"../acme.notes/index.html\">Notes</a>") && evil.contains("<code>acme.gone</code> <span class=\"gloss\">(not in the index)</span>"));
        assert!(evil.contains("<a href=\"../acme.pack/index.html\">Pack</a>"), "the bundles it's in");
        assert!(!evil.contains("id=\"readme\""), "nothing from a folder for one built elsewhere");

        let page = std::fs::read_to_string(out.join("acme.notes/index.html")).unwrap();
        assert!(page.contains("<figure class=\"hero-shot\"><img src=\"screenshots/01-board.png\" alt=\"Notes: board\">"));
        assert!(page.contains("<img src=\"screenshots/02-the-list.png\" alt=\"Notes: the list\""));
        assert!(page.contains("<a href=\"https://github.com/tpedneault/fenix-extensions/blob/main/extensions/notes/docs/how.md\">kept</a>"));
        assert!(page.contains("<a href=\"#readme-keys\">here</a>") && page.contains("id=\"readme-keys\""));
        assert!(page.contains("<img src=\"media/img/plot.png\" alt=\"a plot\"") && !page.contains("secret.png\""));
        assert!(page.contains("&lt;b&gt;Bold&lt;/b&gt; claims.") && page.contains("id=\"changelog-120\""));
        assert!(!page.contains(">Notes</h2>") && !page.contains(">Changelog</h2>\n<h2"), "the documents' own titles left out");
        assert!(page.contains("Nothing: it's data"));
        assert!(page.contains("1.5 or later, before 2"));
        assert!(page.contains("<a href=\"https://example.com/n-1.10.0.fenixext\">Download") && page.matches("javascript").count() == 0);
        assert!(page.find(">1.10.0</th>").unwrap() < page.find(">1.2.0</th>").unwrap(), "newest first");

        let pack = std::fs::read_to_string(out.join("acme.pack/index.html")).unwrap();
        assert!(pack.contains("What it installs") && pack.contains("May use: runs programs, web requests, folders of yours, secrets, coding agents"));
        assert!(!pack.contains("Built elsewhere"), "its repository is this one, with a slash");

        // The same again.
        let again = t.0.join("again");
        generate(&root, &index, &again, HOME, Some(date(1_778_544_000))).unwrap();
        for f in crate::files(&out) {
            assert_eq!(std::fs::read(out.join(&f)).unwrap(), std::fs::read(again.join(&f)).unwrap(), "{}", f.display());
        }
        assert_eq!(crate::files(&out), crate::files(&again));
    }

    /// This repository's own extensions, as an index of their manifests
    /// would list them: every README renders and every link resolves.
    #[test]
    fn the_repositorys_own_extensions_make_a_whole_site() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf();
        let mut extensions = Vec::new();
        let mut bundles = Vec::new();
        for (id, (dir, _)) in folders(&root) {
            let m = crate::manifest(&dir).unwrap();
            let bundle = m.extension.runtime == "bundle";
            let release = serde_json::json!({ "version": m.extension.version, "fenix": m.engine.fenix, "runtime": m.extension.runtime, "api": m.engine.api,
                "dependencies": m.dependencies, "bundle": if bundle { m.dependencies.keys().cloned().collect::<Vec<_>>() } else { Vec::new() },
                "permissions": m.permissions.map(|p| serde_json::to_value(p).unwrap()), "url": format!("{HOME}/releases/x.fenixext"), "size": 1 });
            let listing = serde_json::json!({ "id": id, "name": m.extension.name, "description": m.extension.description, "categories": m.extension.categories, "repository": m.extension.repository, "versions": [release] });
            if bundle { bundles.push(listing) } else { extensions.push(listing) }
        }
        let count = extensions.len() + bundles.len();
        let index = serde_json::json!({ "format": 1, "extensions": extensions, "bundles": bundles });
        let t = Temp::new("repo");
        let pages = generate(&root, &index, &t.0, HOME, None).unwrap();
        assert_eq!(pages, count + 2);
        assert_eq!(broken_links(&t.0), Vec::<String>::new());
        let list = std::fs::read_to_string(t.0.join("index.html")).unwrap();
        assert!(!list.contains("Index as of"), "no date without SOURCE_DATE_EPOCH");
    }

    #[test]
    fn words_for_ranges_dates_and_sizes() {
        assert_eq!(fenix_long(">=1.0, <2"), "1.0 or later, before 2");
        assert_eq!(fenix_long("^1.2"), "^1.2");
        assert_eq!(fenix_short(">=1.5, <2"), "1.5+");
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(1_772_323_200), "2026-03-01");
        assert_eq!(date(951_782_400), "2000-02-29");
        assert_eq!((size(994), size(381_654), size(1_250_000)), ("994 bytes".into(), "382 KB".into(), "1.2 MB".into()));
        assert_eq!(caption("01-the_board.png"), "the board");
        assert!(same_repository("https://GitHub.com/tpedneault/fenix-extensions.git", HOME));
        assert!(!same_repository("https://github.com/someone/fenix-extensions", HOME));
    }
}
