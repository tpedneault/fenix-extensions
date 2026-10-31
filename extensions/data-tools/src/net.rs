//! Schemas and SchemaStore's catalog: fetched once, kept under
//! `/data/schemas/`, fetched again after a week. What waits on one is
//! done again when it comes (`Again`).

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use fenix_extension_api::{run, HttpRequest};
use serde_json::Value;

use crate::schema::{Catalog, Store};

pub const CATALOG: &str = "https://www.schemastore.org/api/json/catalog.json";
const KEPT: &str = "/data/schemas";
const WEEK: Duration = Duration::from_secs(7 * 24 * 3600);

/// What to do once a fetch is over.
#[derive(Clone, Debug, PartialEq)]
pub enum Again {
    /// Check this file again (and say how it went).
    Check(String, bool),
    /// Offer the catalog's schemas for this file.
    Pick(String),
    Nothing,
}

#[derive(Default)]
struct Net {
    store: Store,
    catalog: Option<Rc<Catalog>>,
    asked: HashSet<String>,
    failed: HashMap<String, String>,
    /// What waits on each URL being fetched.
    waiting: HashMap<String, Vec<Again>>,
    /// What's waited and can be done now.
    again: Vec<Again>,
}

thread_local! {
    static NET: RefCell<Net> = RefCell::new(Net::default());
}

pub fn store() -> Store {
    NET.with(|n| n.borrow().store.clone())
}

/// What's waiting to be done again, now that its fetch is over.
pub fn again() -> Vec<Again> {
    NET.with(|n| std::mem::take(&mut n.borrow_mut().again))
}

/// Why `url` couldn't be fetched, if it couldn't.
pub fn failed(url: &str) -> Option<String> {
    NET.with(|n| n.borrow().failed.get(url).cloned())
}

/// A URL as a file name under `/data/schemas`.
pub fn file_for(url: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in url.bytes() {
        hash = (hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    let tail: String = url.rsplit('/').next().unwrap_or("").chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')).take(60).collect();
    format!("{KEPT}/{hash:016x}-{tail}")
}

/// What's kept for `url`, and whether it's old enough to fetch again.
fn kept(url: &str) -> Option<(String, bool)> {
    let path = file_for(url);
    let text = std::fs::read_to_string(&path).ok()?;
    let old = std::fs::metadata(&path).and_then(|m| m.modified()).ok().and_then(|t| SystemTime::now().duration_since(t).ok()).is_some_and(|age| age > WEEK);
    Some((text, old))
}

/// The schema at `url`: in memory, or kept; else it's fetched, and `then`
/// done when it's come.
pub fn schema(url: &str, then: Again) -> Option<Value> {
    if let Some(v) = NET.with(|n| n.borrow().store.0.lock().ok().and_then(|s| s.get(url).cloned())) {
        return Some(v);
    }
    if let Some((text, old)) = kept(url) {
        if let Ok(v) = serde_json::from_str::<Value>(&text) {
            NET.with(|n| n.borrow().store.0.lock().map(|mut s| s.insert(url.to_string(), v.clone())).ok());
            if old {
                fetch(url, Again::Nothing);
            }
            return Some(v);
        }
    }
    fetch(url, then);
    None
}

/// SchemaStore's catalog, the same way.
pub fn catalog(then: Again) -> Option<Rc<Catalog>> {
    if let Some(c) = NET.with(|n| n.borrow().catalog.clone()) {
        return Some(c);
    }
    if let Some((text, old)) = kept(CATALOG) {
        if let Ok(c) = Catalog::parse(&text) {
            let c = Rc::new(c);
            NET.with(|n| n.borrow_mut().catalog = Some(c.clone()));
            if old {
                fetch(CATALOG, Again::Nothing);
            }
            return Some(c);
        }
    }
    fetch(CATALOG, then);
    None
}

fn fetch(url: &str, then: Again) {
    let first = NET.with(|n| {
        let mut n = n.borrow_mut();
        let waiting = n.waiting.entry(url.to_string()).or_default();
        if then != Again::Nothing && !waiting.contains(&then) {
            waiting.push(then);
        }
        n.asked.insert(url.to_string())
    });
    if !first {
        return;
    }
    let url = url.to_string();
    run::spawn(async move {
        let got = run::fetch(HttpRequest::get(&url).header("Accept", "application/json")).await;
        let body = match got {
            Ok(r) if (200..300).contains(&r.status) => Ok(r.text()),
            Ok(r) => Err(format!("HTTP {}", r.status)),
            Err(e) => Err(e),
        };
        let parsed = body.and_then(|text| serde_json::from_str::<Value>(&text).map(|v| (text, v)).map_err(|e| format!("not JSON: {e}")));
        NET.with(|n| {
            let mut n = n.borrow_mut();
            n.asked.remove(&url);
            n.failed.remove(&url);
            match parsed {
                Ok((text, v)) => {
                    let _ = std::fs::create_dir_all(KEPT);
                    let _ = std::fs::write(file_for(&url), &text);
                    if url == CATALOG {
                        match Catalog::parse(&text) {
                            Ok(c) => n.catalog = Some(Rc::new(c)),
                            Err(e) => {
                                n.failed.insert(url.clone(), e);
                            }
                        }
                    } else if let Ok(mut s) = n.store.0.lock() {
                        s.insert(url.clone(), v);
                    }
                }
                Err(e) => {
                    fenix_extension_api::warn!("{url}: {e}");
                    n.failed.insert(url.clone(), e);
                }
            }
            let done = n.waiting.remove(&url).unwrap_or_default();
            n.again.extend(done);
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_kept_by_a_name_of_their_own() {
        let a = file_for("https://www.schemastore.org/package.json");
        assert!(a.starts_with("/data/schemas/") && a.ends_with("-package.json"), "{a}");
        assert_ne!(a, file_for("https://example.org/package.json"));
        assert_eq!(a, file_for("https://www.schemastore.org/package.json"));
    }
}
