//! The statements you ran: where, when, how they went -- kept in the
//! extension's folder, a JSON line each, and searched by their words.

use serde::{Deserialize, Serialize};

/// The most kept; the oldest go past it.
const MOST: usize = 2000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// When, in seconds since 1970 (UTC).
    pub at: i64,
    pub conn: String,
    pub sql: String,
    pub ok: bool,
    /// How it went: `3 rows`, `UPDATE 1`, its error.
    pub summary: String,
    pub millis: u64,
}

#[derive(Debug, Default)]
pub struct History {
    pub entries: Vec<Entry>,
    path: Option<String>,
}

impl History {
    /// What's kept at `path`; nothing if there's none.
    pub fn load(path: &str) -> History {
        let mut h = History { entries: Vec::new(), path: None };
        for e in std::fs::read_to_string(path).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()) {
            h.push(e);
        }
        h.path = Some(path.to_string());
        h
    }

    /// One more, kept: added to the file, which is written again, shorter,
    /// once it's twice as long as what's kept.
    pub fn push(&mut self, entry: Entry) {
        // The same statement again, just after: once.
        if self.entries.last().is_some_and(|e| e.sql == entry.sql && e.conn == entry.conn) {
            self.entries.pop();
        }
        self.entries.push(entry);
        let Some(path) = &self.path else { return };
        if self.entries.len() > MOST * 2 {
            self.entries.drain(..self.entries.len() - MOST);
            let text: String = self.entries.iter().filter_map(|e| serde_json::to_string(e).ok()).map(|l| l + "\n").collect();
            let _ = std::fs::write(path, text);
        } else if let Some(line) = self.entries.last().and_then(|e| serde_json::to_string(e).ok()) {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                let _ = writeln!(f, "{line}");
            }
        }
    }

    /// The newest first, those with every word of `query` (any case) in
    /// their statement or connection's name.
    pub fn search(&self, query: &str) -> Vec<&Entry> {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        self.entries
            .iter()
            .rev()
            .filter(|e| {
                let hay = format!("{} {}", e.conn, e.sql).to_lowercase();
                words.iter().all(|w| hay.contains(w.as_str()))
            })
            .collect()
    }
}

/// A statement on one line, for a picker: blanks run together, cut at
/// `most` characters.
pub fn one_line(sql: &str, most: usize) -> String {
    let s = sql.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() > most {
        format!("{}…", s.chars().take(most.saturating_sub(1)).collect::<String>())
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(conn: &str, sql: &str) -> Entry {
        Entry { at: 0, conn: conn.into(), sql: sql.into(), ok: true, summary: "1 row".into(), millis: 3 }
    }

    #[test]
    fn statements_are_kept_and_searched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.jsonl");
        let path = path.to_str().unwrap();
        let mut h = History::load(path);
        h.push(entry("prod", "select * from users"));
        h.push(entry("dev", "SELECT count(*) FROM orders"));
        h.push(entry("dev", "SELECT count(*) FROM orders"));
        h.push(entry("prod", "delete from sessions where at < now()"));
        assert_eq!(h.entries.len(), 3, "the same one twice running is kept once");
        let again = History::load(path);
        assert_eq!(again.entries, h.entries);
        assert_eq!(h.search("FROM").len(), 3);
        assert_eq!(h.search("prod from").iter().map(|e| e.sql.as_str()).collect::<Vec<_>>(), ["delete from sessions where at < now()", "select * from users"], "newest first, every word");
        assert!(h.search("nothing").is_empty());
        assert_eq!(one_line("select *\n  from   t", 100), "select * from t");
        assert_eq!(one_line("abcdef", 4), "abc…");
    }

    #[test]
    fn it_stays_short() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("h.jsonl");
        let mut h = History::load(path.to_str().unwrap());
        for i in 0..MOST * 2 + 1 {
            h.push(entry("x", &format!("select {i}")));
        }
        assert_eq!(h.entries.len(), MOST);
        assert_eq!(History::load(path.to_str().unwrap()).entries.len(), MOST);
    }
}
