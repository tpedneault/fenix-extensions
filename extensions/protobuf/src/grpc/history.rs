//! What was called, kept per method in the extension's folder
//! (`/data/history.json`), the newest first: the server, the body and
//! metadata sent, and how it went -- its status, how long it took, how
//! many messages came, and the first of them. A body from the history is
//! a body to send again.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The most of a response that's kept.
pub const KEEP_BYTES: usize = 16 << 10;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Seconds since 1970.
    pub at: i64,
    pub target: String,
    pub body: String,
    #[serde(default)]
    pub metadata: Vec<(String, String)>,
    /// Its status code's name, or `ERROR`.
    pub code: String,
    /// What the status or failure said.
    #[serde(default)]
    pub message: String,
    pub ms: u64,
    pub messages: usize,
    /// The first message, cut at [`KEEP_BYTES`].
    #[serde(default)]
    pub first: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct History {
    /// By method, `orbit.v1.OrbitService/GetSatellite`.
    pub methods: BTreeMap<String, Vec<Entry>>,
}

/// `text` cut at `max` bytes, on a character's edge.
fn cut(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

impl History {
    pub fn parse(text: &str) -> History {
        serde_json::from_str(text).unwrap_or_default()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// A call kept, first; `keep` a method at most.
    pub fn push(&mut self, method: &str, mut entry: Entry, keep: usize) {
        entry.first = cut(&entry.first, KEEP_BYTES);
        let list = self.methods.entry(method.to_string()).or_default();
        list.insert(0, entry);
        list.truncate(keep.max(1));
    }

    pub fn of(&self, method: &str) -> &[Entry] {
        self.methods.get(method).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Read from `path`; empty when it isn't there or doesn't read.
    pub fn load(path: &str) -> History {
        std::fs::read_to_string(path).map(|t| History::parse(&t)).unwrap_or_default()
    }

    pub fn save(&self, path: &str) -> Result<(), String> {
        if let Some(dir) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, self.to_json()).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(at: i64, code: &str) -> Entry {
        Entry { at, target: "localhost:50051".into(), body: "{}".into(), metadata: vec![("x-team".into(), "ops".into())], code: code.into(), message: String::new(), ms: 12, messages: 1, first: "{\"id\": \"1\"}".into() }
    }

    #[test]
    fn each_method_keeps_its_newest_calls() {
        let mut h = History::default();
        for i in 0..5 {
            h.push("a.S/Get", entry(i, "OK"), 3);
        }
        h.push("a.S/Watch", entry(9, "NotFound"), 3);
        assert_eq!(h.of("a.S/Get").iter().map(|e| e.at).collect::<Vec<_>>(), [4, 3, 2], "newest first, three kept");
        assert_eq!(h.of("a.S/Watch")[0].code, "NotFound");
        assert!(h.of("a.S/Nothing").is_empty());
        assert_eq!(History::parse(&h.to_json()), h);
        assert_eq!(History::parse("not json"), History::default());

        let mut big = entry(1, "OK");
        big.first = "é".repeat(KEEP_BYTES);
        h.push("a.S/Big", big, 3);
        let kept = &h.of("a.S/Big")[0].first;
        assert!(kept.len() <= KEEP_BYTES + "…".len() && kept.ends_with('…'));

        let dir = crate::project::tests::temp("history");
        let path = dir.join("sub/history.json").to_string_lossy().into_owned();
        h.save(&path).unwrap();
        assert_eq!(History::load(&path), h);
        assert_eq!(History::load("/nowhere/history.json"), History::default());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
