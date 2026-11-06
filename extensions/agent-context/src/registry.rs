//! Where each ecosystem keeps a package's documentation for one version,
//! and what's read from there:
//!
//! - **Cargo**: the crate's README as crates.io renders it for that
//!   version (`/api/v1/crates/<name>/<version>/readme`); an item's page of
//!   docs.rs's rustdoc for that version, found in its `all.html`; the
//!   crate's front page there when it has no README.
//! - **npm**: the registry's README for that version, else the one in its
//!   published package (jsDelivr), else the package's latest README,
//!   saying so.
//! - **Python**: PyPI's description of that release.
//! - **Go**: pkg.go.dev's page for the module (or a package of it) at that
//!   version; the module proxy's `.info` and `go.mod` when there's none.
//!
//! The fetching goes through a [`Get`], so the whole of it runs in tests
//! against canned answers.

use std::future::Future;
use std::pin::Pin;

use crate::deps::Ecosystem;
use crate::text;

/// An answer: its status and body, or why there's none.
pub type Body = Result<(u16, String), String>;

/// What fetches a URL.
pub type Get<'a> = &'a dyn Fn(String) -> Pin<Box<dyn Future<Output = Body>>>;

/// What was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Docs {
    /// Where it came from.
    pub source: String,
    pub text: String,
    /// It's about the item asked for, not the package as a whole.
    pub about_item: bool,
    /// Something to know: it's the latest README, not this version's.
    pub note: Option<String>,
}

/// The hosts it reaches, for its manifest's `permissions.network`.
pub const HOSTS: &[&str] = &["crates.io", "static.crates.io", "docs.rs", "registry.npmjs.org", "cdn.jsdelivr.net", "pypi.org", "pkg.go.dev", "proxy.golang.org"];

async fn get_ok(get: Get<'_>, url: String) -> Option<String> {
    match get(url).await {
        Ok((200, body)) => Some(body),
        _ => None,
    }
}

/// The documentation of `name` at `version`, about `item` when given.
pub async fn fetch(get: Get<'_>, ecosystem: Ecosystem, name: &str, version: &str, item: Option<&str>) -> Result<Docs, String> {
    match ecosystem {
        Ecosystem::Cargo => cargo(get, name, version, item).await,
        Ecosystem::Npm => npm(get, name, version).await,
        Ecosystem::Python => python(get, name, version).await,
        Ecosystem::Go => go(get, name, version, item).await,
    }
}

// -- Cargo --------------------------------------------------------------------

/// A page's main part: rustdoc's and pkg.go.dev's `#main-content`, else
/// all of it.
pub fn main_content(html: &str) -> &str {
    html.find("id=\"main-content\"").and_then(|at| html[..at].rfind('<')).map_or(html, |start| &html[start..])
}

/// The `href`s of a page's links.
fn hrefs(html: &str) -> Vec<&str> {
    html.match_indices("href=\"").filter_map(|(at, m)| html[at + m.len()..].split('"').next()).collect()
}

/// The page rustdoc's `all.html` links for `path` (`sync::Mutex`, its
/// crate's name left off): the item of that name in that module, else
/// the first of that name anywhere, else one whose name differs only in
/// case.
pub fn rustdoc_item(all: &str, path: &[&str]) -> Option<String> {
    let (name, module) = path.split_last()?;
    // Each link: its href, its folders, and the item's name.
    let links: Vec<(&str, Vec<&str>, &str)> = hrefs(all)
        .into_iter()
        .filter(|h| !h.contains("://") && !h.starts_with('#'))
        .filter_map(|h| {
            let mut dirs: Vec<&str> = h.split('/').collect();
            let file = dirs.pop()?;
            let (_kind, item) = file.strip_suffix(".html")?.split_once('.')?;
            Some((h, dirs, item))
        })
        .collect();
    links
        .iter()
        .find(|(_, d, i)| d.as_slice() == module && i == name)
        .or_else(|| links.iter().find(|(_, _, i)| i == name))
        .or_else(|| links.iter().find(|(_, _, i)| i.eq_ignore_ascii_case(name)))
        .map(|(h, _, _)| h.to_string())
}

async fn cargo(get: Get<'_>, name: &str, version: &str, item: Option<&str>) -> Result<Docs, String> {
    let lib = name.replace('-', "_");
    let docs = format!("https://docs.rs/{name}/{version}/{lib}");
    let mut note = None;
    if let Some(item) = item.map(str::trim).filter(|i| !i.is_empty()) {
        let mut path: Vec<&str> = item.split("::").filter(|s| !s.is_empty()).collect();
        if path.first().is_some_and(|p| *p == lib || *p == name || *p == "crate") {
            path.remove(0);
        }
        if !path.is_empty() {
            let page = match get_ok(get, format!("{docs}/all.html")).await {
                Some(all) => rustdoc_item(&all, &path).map(|href| format!("{docs}/{href}")),
                None => None,
            };
            // Not an item: a module, perhaps.
            let page = page.unwrap_or_else(|| format!("{docs}/{}/index.html", path.join("/")));
            if let Some(html) = get_ok(get, page.clone()).await {
                return Ok(Docs { source: page, text: text::html_to_text(main_content(&html)), about_item: true, note: None });
            }
            note = Some(format!("docs.rs has no page for {item} in {name} {version}: the crate's README instead"));
        }
    }
    let readme = format!("https://crates.io/api/v1/crates/{name}/{version}/readme");
    match get(readme.clone()).await {
        Ok((200, html)) if !html.trim().is_empty() => return Ok(Docs { source: readme, text: text::html_to_text(&html), about_item: false, note }),
        Ok((404, _)) | Ok((200, _)) => {}
        Ok((status, _)) => note = Some(format!("crates.io said {status} for the README")),
        Err(e) => note = Some(format!("crates.io: {e}")),
    }
    // No README: the crate's front page on docs.rs.
    let front = format!("{docs}/index.html");
    match get(front.clone()).await {
        Ok((200, html)) => Ok(Docs { source: front, text: text::html_to_text(main_content(&html)), about_item: false, note }),
        Ok((404, _)) => Err(format!("neither crates.io nor docs.rs has documentation for {name} {version}")),
        Ok((status, _)) => Err(format!("docs.rs said {status} for {name} {version}")),
        Err(e) => Err(format!("docs.rs: {e}")),
    }
}

// -- npm ----------------------------------------------------------------------

/// A package's name in a registry URL: `@scope/name` as `@scope%2fname`.
fn npm_path(name: &str) -> String {
    name.replace('/', "%2f")
}

/// The lines that say what a package is: its description, homepage and
/// repository.
fn npm_header(meta: &serde_json::Value) -> String {
    let mut lines = Vec::new();
    if let Some(d) = meta.get("description").and_then(|v| v.as_str()).filter(|d| !d.trim().is_empty()) {
        lines.push(d.trim().to_string());
    }
    if let Some(h) = meta.get("homepage").and_then(|v| v.as_str()) {
        lines.push(format!("Homepage: {h}"));
    }
    let repo = meta.get("repository").and_then(|r| r.as_str().map(str::to_string).or_else(|| r.get("url").and_then(|u| u.as_str()).map(str::to_string)));
    if let Some(r) = repo {
        lines.push(format!("Repository: {r}"));
    }
    lines.join("\n")
}

fn readme_text(text: &str) -> String {
    if text::looks_like_html(text) {
        text::html_to_text(text)
    } else {
        text::tidy_markdown(text)
    }
}

async fn npm(get: Get<'_>, name: &str, version: &str) -> Result<Docs, String> {
    let at = format!("https://registry.npmjs.org/{}/{version}", npm_path(name));
    let meta: serde_json::Value = match get(at.clone()).await {
        Ok((200, body)) => serde_json::from_str(&body).map_err(|e| format!("the npm registry's answer: {e}"))?,
        Ok((404, _)) => return Err(format!("the npm registry has no {name} {version}")),
        Ok((status, _)) => return Err(format!("the npm registry said {status} for {name} {version}")),
        Err(e) => return Err(format!("the npm registry: {e}")),
    };
    let header = npm_header(&meta);
    let with_header = |readme: &str| if header.is_empty() { readme_text(readme) } else { format!("{header}\n\n{}", readme_text(readme)) };
    if let Some(readme) = meta.get("readme").and_then(|v| v.as_str()).filter(|r| !r.trim().is_empty() && !r.starts_with("ERROR: No README")) {
        return Ok(Docs { source: at, text: with_header(readme), about_item: false, note: None });
    }
    // The README in the package as it was published.
    for file in ["README.md", "readme.md", "Readme.md", "README"] {
        let url = format!("https://cdn.jsdelivr.net/npm/{name}@{version}/{file}");
        if let Some(readme) = get_ok(get, url.clone()).await.filter(|r| !r.trim().is_empty()) {
            return Ok(Docs { source: url, text: with_header(&readme), about_item: false, note: None });
        }
    }
    // The package's README: its latest version's.
    let all = format!("https://registry.npmjs.org/{}", npm_path(name));
    if let Some(body) = get_ok(get, all.clone()).await {
        let doc: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
        if let Some(readme) = doc.get("readme").and_then(|v| v.as_str()).filter(|r| !r.trim().is_empty()) {
            let latest = doc.get("dist-tags").and_then(|t| t.get("latest")).and_then(|v| v.as_str()).unwrap_or("?");
            let note = (latest != version).then(|| format!("the README of the latest version, {latest}: {version}'s isn't published on its own"));
            return Ok(Docs { source: all, text: with_header(readme), about_item: false, note });
        }
    }
    if header.is_empty() {
        return Err(format!("{name} {version} has no README on the npm registry"));
    }
    Ok(Docs { source: at, text: header, about_item: false, note: Some("no README: only the package's description".into()) })
}

// -- Python -------------------------------------------------------------------

async fn python(get: Get<'_>, name: &str, version: &str) -> Result<Docs, String> {
    let url = format!("https://pypi.org/pypi/{name}/{version}/json");
    let json: serde_json::Value = match get(url.clone()).await {
        Ok((200, body)) => serde_json::from_str(&body).map_err(|e| format!("PyPI's answer: {e}"))?,
        Ok((404, _)) => return Err(format!("PyPI has no {name} {version}")),
        Ok((status, _)) => return Err(format!("PyPI said {status} for {name} {version}")),
        Err(e) => return Err(format!("PyPI: {e}")),
    };
    let info = &json["info"];
    let field = |k: &str| info.get(k).and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty() && *s != "UNKNOWN");
    let mut lines = Vec::new();
    if let Some(s) = field("summary") {
        lines.push(s.to_string());
    }
    if let Some(urls) = info.get("project_urls").and_then(|u| u.as_object()) {
        for (label, u) in urls {
            if let Some(u) = u.as_str() {
                lines.push(format!("{label}: {u}"));
            }
        }
    } else if let Some(h) = field("home_page") {
        lines.push(format!("Homepage: {h}"));
    }
    if let Some(r) = field("requires_python") {
        lines.push(format!("Requires Python {r}"));
    }
    let description = field("description").map(|d| match field("description_content_type") {
        Some(t) if t.contains("html") => text::html_to_text(d),
        Some(t) if t.contains("markdown") => text::tidy_markdown(d),
        _ => d.to_string(),
    });
    let note = description.is_none().then(|| "no description on PyPI for this release".to_string());
    if let Some(d) = description {
        lines.push(String::new());
        lines.push(d);
    }
    if lines.is_empty() {
        return Err(format!("PyPI has nothing to say about {name} {version}"));
    }
    Ok(Docs { source: url, text: lines.join("\n"), about_item: false, note })
}

// -- Go -----------------------------------------------------------------------

/// A module path as the module proxy spells it: capitals as `!` and the
/// letter lower case.
pub fn proxy_escape(module: &str) -> String {
    let mut out = String::new();
    for c in module.chars() {
        if c.is_ascii_uppercase() {
            out.push('!');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// What an item names in a Go module: a package under it, and a symbol
/// in that package -- `Command`, `cobra.Command`, `doc`,
/// `doc.GenMarkdown`, `github.com/spf13/cobra/doc.GenMarkdown`.
pub fn go_item(module: &str, item: &str) -> (Option<String>, Option<String>) {
    let item = item.trim().trim_start_matches(module).trim_start_matches('/');
    if item.is_empty() {
        return (None, None);
    }
    let (dir, last) = match item.rsplit_once('/') {
        Some((d, l)) => (Some(d), l),
        None => (None, item),
    };
    // The module's own package's name: its path's last part (`cobra`), or
    // the one before a major version (`yaml` of `gopkg.in/yaml/v3`).
    let mut parts = module.rsplit('/');
    let own = match parts.next() {
        Some(v) if v.len() > 1 && v.starts_with('v') && v[1..].chars().all(|c| c.is_ascii_digit()) => parts.next().unwrap_or(v),
        Some(p) => p.split('.').next().unwrap_or(p),
        None => module,
    };
    let (package, symbol) = match last.split_once('.') {
        // `cobra.Command`: the module's own package.
        Some((pkg, symbol)) if dir.is_none() && pkg == own => (None, Some(symbol)),
        Some((pkg, symbol)) => (Some(format!("{}/{pkg}", dir.unwrap_or(""))), Some(symbol)),
        None if dir.is_none() && last.starts_with(|c: char| c.is_ascii_uppercase()) => (None, Some(last)),
        None => (Some(dir.map_or(last.to_string(), |d| format!("{d}/{last}"))), None),
    };
    (package.map(|p| p.trim_start_matches('/').to_string()), symbol.map(str::to_string))
}

async fn go(get: Get<'_>, module: &str, version: &str, item: Option<&str>) -> Result<Docs, String> {
    let (package, symbol) = item.map(|i| go_item(module, i)).unwrap_or_default();
    let path = match &package {
        Some(p) => format!("{module}/{p}"),
        None => module.to_string(),
    };
    let url = format!("https://pkg.go.dev/{path}@{version}");
    if let Some(html) = get_ok(get, url.clone()).await {
        let page = text::html_to_text(main_content(&html));
        let focused = symbol.as_deref().and_then(|s| text::focus(&page, &format!("type {s}")).or_else(|| text::focus(&page, &format!("func {s}"))).or_else(|| text::focus(&page, s)));
        let about_item = focused.is_some() || (package.is_some() && symbol.is_none());
        return Ok(Docs { source: url, text: focused.unwrap_or(page), about_item, note: None });
    }
    // A module pkg.go.dev hasn't got (a private one): what the proxy knows.
    let proxy = format!("https://proxy.golang.org/{}/@v/{version}", proxy_escape(module));
    let info = get_ok(get, format!("{proxy}.info")).await;
    let gomod = get_ok(get, format!("{proxy}.mod")).await;
    if info.is_none() && gomod.is_none() {
        return Err(format!("neither pkg.go.dev nor the module proxy has {module} {version}"));
    }
    let mut lines = vec![format!("{module} {version}")];
    if let Some(time) = info.as_deref().and_then(|i| serde_json::from_str::<serde_json::Value>(i).ok()).and_then(|j| j.get("Time").and_then(|t| t.as_str()).map(str::to_string)) {
        lines.push(format!("Published {time}"));
    }
    if let Some(m) = gomod {
        lines.push(String::new());
        lines.push(format!("Its go.mod:\n```\n{}\n```", m.trim()));
    }
    Ok(Docs { source: format!("{proxy}.info"), text: lines.join("\n"), about_item: false, note: Some("pkg.go.dev has no page for it: only what the module proxy knows".into()) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::rc::Rc;

    fn testdata(name: &str) -> String {
        std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/testdata").join(name)).unwrap()
    }

    /// A future that's ready, run.
    pub(crate) fn block_on<T>(f: impl Future<Output = T>) -> T {
        use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
        fn clone(_: *const ()) -> RawWaker {
            RawWaker::new(std::ptr::null(), &VTABLE)
        }
        fn noop(_: *const ()) {}
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);
        // SAFETY: every function in the table ignores its (null) data.
        let waker = unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) };
        let mut f = std::pin::pin!(f);
        match f.as_mut().poll(&mut Context::from_waker(&waker)) {
            Poll::Ready(v) => v,
            Poll::Pending => panic!("a canned answer is always ready"),
        }
    }

    /// Canned answers by URL, and the URLs asked for, in order.
    type Asked = Rc<std::cell::RefCell<Vec<String>>>;

    fn web(answers: &[(&str, u16, String)]) -> (impl Fn(String) -> Pin<Box<dyn Future<Output = Body>>>, Asked) {
        let map: HashMap<String, (u16, String)> = answers.iter().map(|(u, s, b)| (u.to_string(), (*s, b.clone()))).collect();
        let asked = Rc::new(std::cell::RefCell::new(Vec::new()));
        let log = asked.clone();
        let get = move |url: String| -> Pin<Box<dyn Future<Output = Body>>> {
            log.borrow_mut().push(url.clone());
            let answer = map.get(&url).cloned().unwrap_or((404, String::new()));
            Box::pin(async move { Ok(answer) })
        };
        (get, asked)
    }

    #[test]
    fn a_crates_readme_for_its_version() {
        let (get, asked) = web(&[("https://crates.io/api/v1/crates/anyhow/1.0.79/readme", 200, testdata("crates-readme.html"))]);
        let docs = block_on(fetch(&get, Ecosystem::Cargo, "anyhow", "1.0.79", None)).unwrap();
        assert_eq!(docs.source, "https://crates.io/api/v1/crates/anyhow/1.0.79/readme");
        assert!(docs.text.starts_with("# Anyhow"), "{}", docs.text);
        assert_eq!(asked.borrow().len(), 1);
    }

    #[test]
    fn an_item_of_a_crate_from_docs_rs() {
        let all = "<ul class=\"all-items\"><li><a href=\"struct.Runtime.html\">Runtime</a></li><li><a href=\"sync/struct.Mutex.html\">sync::Mutex</a></li><li><a href=\"sync/mpsc/fn.channel.html\">sync::mpsc::channel</a></li><li><a href=\"macro.select.html\">select</a></li></ul>";
        assert_eq!(rustdoc_item(all, &["sync", "Mutex"]).as_deref(), Some("sync/struct.Mutex.html"));
        assert_eq!(rustdoc_item(all, &["Mutex"]).as_deref(), Some("sync/struct.Mutex.html"), "by its name anywhere");
        assert_eq!(rustdoc_item(all, &["sync", "mpsc", "channel"]).as_deref(), Some("sync/mpsc/fn.channel.html"));
        assert_eq!(rustdoc_item(all, &["select"]).as_deref(), Some("macro.select.html"));
        assert_eq!(rustdoc_item(all, &["runtime"]).as_deref(), Some("struct.Runtime.html"), "any case, last");
        assert_eq!(rustdoc_item(all, &["nothing"]), None);

        let (get, asked) = web(&[
            ("https://docs.rs/tokio/1.35.1/tokio/all.html", 200, all.to_string()),
            ("https://docs.rs/tokio/1.35.1/tokio/sync/struct.Mutex.html", 200, testdata("docs-rs-item.html")),
        ]);
        let docs = block_on(fetch(&get, Ecosystem::Cargo, "tokio", "1.35.1", Some("tokio::sync::Mutex"))).unwrap();
        assert!(docs.about_item);
        assert!(docs.text.contains("pub fn new(t: T) -> Mutex<T>"), "{}", docs.text);
        assert_eq!(asked.borrow().len(), 2);

        // A module: its index page.
        let (get, _) = web(&[("https://docs.rs/tokio/1.35.1/tokio/sync/index.html", 200, "<main id=\"main-content\"><h1>Module tokio::sync</h1></main>".into())]);
        let docs = block_on(fetch(&get, Ecosystem::Cargo, "tokio", "1.35.1", Some("sync"))).unwrap();
        assert_eq!(docs.text, "# Module tokio::sync");

        // Nothing of that name: the README, saying so.
        let (get, _) = web(&[("https://crates.io/api/v1/crates/tokio/1.35.1/readme", 200, "<h1>Tokio</h1>".into())]);
        let docs = block_on(fetch(&get, Ecosystem::Cargo, "tokio", "1.35.1", Some("Nope"))).unwrap();
        assert!(!docs.about_item && docs.text == "# Tokio" && docs.note.unwrap().contains("no page for Nope"));
    }

    #[test]
    fn a_crate_without_a_readme_has_its_front_page() {
        let (get, _) = web(&[("https://docs.rs/my-crate/0.2.0/my_crate/index.html", 200, "<nav>x</nav><section id=\"main-content\"><h1>Crate my_crate</h1><p>Does things.</p></section>".into())]);
        let docs = block_on(fetch(&get, Ecosystem::Cargo, "my-crate", "0.2.0", None)).unwrap();
        assert_eq!(docs.text, "# Crate my_crate\n\nDoes things.");
        let (get, _) = web(&[]);
        assert!(block_on(fetch(&get, Ecosystem::Cargo, "my-crate", "0.2.0", None)).unwrap_err().contains("neither crates.io nor docs.rs"));
    }

    #[test]
    fn an_npm_readme_for_its_version() {
        let meta = r##"{"name":"@babel/core","version":"7.23.7","description":"Babel compiler core.","homepage":"https://babel.dev","readme":"# @babel/core\n\n[![npm](https://x.svg)](https://y)\n\nUse `transform`."}"##;
        let (get, asked) = web(&[("https://registry.npmjs.org/@babel%2fcore/7.23.7", 200, meta.into())]);
        let docs = block_on(fetch(&get, Ecosystem::Npm, "@babel/core", "7.23.7", None)).unwrap();
        assert_eq!(docs.text, "Babel compiler core.\nHomepage: https://babel.dev\n\n# @babel/core\n\nUse `transform`.");
        assert_eq!(asked.borrow().len(), 1);

        // No README in the version's metadata: the published package's.
        let meta = r#"{"name":"react","version":"18.2.0","description":"React is a JavaScript library.","repository":{"type":"git","url":"https://github.com/facebook/react.git"}}"#;
        let (get, _) = web(&[("https://registry.npmjs.org/react/18.2.0", 200, meta.into()), ("https://cdn.jsdelivr.net/npm/react@18.2.0/README.md", 200, "# react\n\nThe library.".into())]);
        let docs = block_on(fetch(&get, Ecosystem::Npm, "react", "18.2.0", None)).unwrap();
        assert_eq!(docs.source, "https://cdn.jsdelivr.net/npm/react@18.2.0/README.md");
        assert_eq!(docs.text, "React is a JavaScript library.\nRepository: https://github.com/facebook/react.git\n\n# react\n\nThe library.");

        // Nor there: the latest README, saying whose.
        let (get, _) = web(&[("https://registry.npmjs.org/react/18.2.0", 200, meta.into()), ("https://registry.npmjs.org/react", 200, r##"{"dist-tags":{"latest":"19.0.0"},"readme":"# react 19"}"##.into())]);
        let docs = block_on(fetch(&get, Ecosystem::Npm, "react", "18.2.0", None)).unwrap();
        assert!(docs.note.unwrap().contains("latest version, 19.0.0"));
        let (get, _) = web(&[]);
        assert_eq!(block_on(fetch(&get, Ecosystem::Npm, "react", "99.0.0", None)).unwrap_err(), "the npm registry has no react 99.0.0");
    }

    #[test]
    fn a_pypi_description_for_its_release() {
        let (get, _) = web(&[("https://pypi.org/pypi/requests/2.31.0/json", 200, testdata("pypi-requests.json"))]);
        let docs = block_on(fetch(&get, Ecosystem::Python, "requests", "2.31.0", None)).unwrap();
        assert!(docs.text.starts_with("Python HTTP for Humans.\nDocumentation: https://requests.readthedocs.io\nSource: https://github.com/psf/requests\nRequires Python >=3.7\n\n# Requests"), "{}", docs.text);
        assert!(!docs.text.contains("img.shields.io"), "badges out: {}", docs.text);
        let (get, _) = web(&[]);
        assert_eq!(block_on(fetch(&get, Ecosystem::Python, "requests", "0.0.1", None)).unwrap_err(), "PyPI has no requests 0.0.1");
    }

    #[test]
    fn a_go_module_on_pkg_go_dev_or_the_proxy() {
        assert_eq!(go_item("github.com/spf13/cobra", "Command"), (None, Some("Command".into())));
        assert_eq!(go_item("github.com/spf13/cobra", "cobra.Command"), (None, Some("Command".into())));
        assert_eq!(go_item("github.com/spf13/cobra", "doc"), (Some("doc".into()), None));
        assert_eq!(go_item("github.com/spf13/cobra", "doc.GenMarkdown"), (Some("doc".into()), Some("GenMarkdown".into())), "a package of the module");
        assert_eq!(go_item("gopkg.in/yaml.v3", "yaml.Node"), (None, Some("Node".into())));
        assert_eq!(go_item("github.com/jackc/pgx/v5", "pgx.Conn"), (None, Some("Conn".into())));
        assert_eq!(go_item("github.com/spf13/cobra", "github.com/spf13/cobra/doc.GenMarkdown"), (Some("doc".into()), Some("GenMarkdown".into())));
        assert_eq!(go_item("golang.org/x/sync", "x/errgroup.Group"), (Some("x/errgroup".into()), Some("Group".into())));
        assert_eq!(proxy_escape("github.com/BurntSushi/toml"), "github.com/!burnt!sushi/toml");

        let page = "<header>Go</header><main id=\"main-content\"><h2>Overview</h2><p>Package cobra is a commander.</p><h3>func CheckErr</h3><pre>func CheckErr(msg interface{})</pre><h3>type Command</h3><pre>type Command struct {\n\tUse string\n}</pre></main>";
        let (get, _) = web(&[("https://pkg.go.dev/github.com/spf13/cobra@v1.8.0", 200, page.into())]);
        let docs = block_on(fetch(&get, Ecosystem::Go, "github.com/spf13/cobra", "v1.8.0", Some("Command"))).unwrap();
        assert!(docs.about_item);
        assert!(docs.text.starts_with("### type Command\n\n```\ntype Command struct {"), "{}", docs.text);
        let docs = block_on(fetch(&get, Ecosystem::Go, "github.com/spf13/cobra", "v1.8.0", None)).unwrap();
        assert!(docs.text.starts_with("## Overview\n\nPackage cobra is a commander."), "{}", docs.text);

        let (get, _) = web(&[
            ("https://proxy.golang.org/example.com/!private/@v/v0.1.0.info", 200, r#"{"Version":"v0.1.0","Time":"2024-01-02T03:04:05Z"}"#.into()),
            ("https://proxy.golang.org/example.com/!private/@v/v0.1.0.mod", 200, "module example.com/Private\n".into()),
        ]);
        let docs = block_on(fetch(&get, Ecosystem::Go, "example.com/Private", "v0.1.0", None)).unwrap();
        assert_eq!(docs.text, "example.com/Private v0.1.0\nPublished 2024-01-02T03:04:05Z\n\nIts go.mod:\n```\nmodule example.com/Private\n```");
        assert!(docs.note.is_some());
    }
}
