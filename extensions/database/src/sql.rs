//! SQL as text: statements split where each engine splits them -- past
//! quotes, comments, PostgreSQL's dollar quoting, a trigger's body,
//! MySQL's `DELIMITER` and SQL Server's `GO` -- the statement under the
//! cursor, and what kind of statement one is, for the safety rules.

use std::ops::Range;

/// Whose SQL it is: how it quotes, comments and splits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Dialect {
    Sqlite,
    Postgres,
    Mysql,
    Mssql,
}

impl Dialect {
    /// `name` quoted as an identifier.
    pub fn ident(self, name: &str) -> String {
        match self {
            Dialect::Mysql => format!("`{}`", name.replace('`', "``")),
            Dialect::Mssql => format!("[{}]", name.replace(']', "]]")),
            Dialect::Sqlite | Dialect::Postgres => format!("\"{}\"", name.replace('"', "\"\"")),
        }
    }

    /// `text` as a string literal.
    pub fn string(self, text: &str) -> String {
        match self {
            // A backslash escapes in MySQL's strings (unless the server's
            // NO_BACKSLASH_ESCAPES is on): doubled, it's itself.
            Dialect::Mysql => format!("'{}'", text.replace('\\', "\\\\").replace('\'', "''")),
            Dialect::Mssql => format!("N'{}'", text.replace('\'', "''")),
            Dialect::Sqlite | Dialect::Postgres => format!("'{}'", text.replace('\'', "''")),
        }
    }

    /// `schema`.`table`, or the table alone.
    pub fn table(self, schema: Option<&str>, table: &str) -> String {
        match schema.filter(|s| !s.is_empty()) {
            Some(s) => format!("{}.{}", self.ident(s), self.ident(table)),
            None => self.ident(table),
        }
    }
}

/// One statement of a text: where it is (its bytes, trimmed, without the
/// `;` that ends it) and what it says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement {
    pub range: Range<usize>,
    /// Up to and with the `;`, `GO` or delimiter that ends it, or the
    /// text's end: what the cursor may be on to mean it.
    pub reach: Range<usize>,
    pub text: String,
}

/// What's open at a byte while scanning.
#[derive(Debug, Clone, PartialEq, Eq)]
enum In {
    Code,
    /// A quote, closed by the same character; `backslash` escapes in it.
    Quoted {
        close: u8,
        backslash: bool,
    },
    LineComment,
    /// `/* */`, PostgreSQL's nested.
    BlockComment(usize),
    /// `$tag$ ... $tag$`.
    Dollar(String),
}

/// `text`'s statements, in order. Empty ones -- only blanks, comments --
/// are left out.
pub fn split(text: &str, dialect: Dialect) -> Vec<Statement> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut state = In::Code;
    let mut start = 0;
    let mut i = 0;
    let mut delimiter = String::from(";");
    // BEGIN ... END in a trigger's (SQLite's) body: `;` inside doesn't end it.
    let mut depth = 0usize;
    let mut words: Vec<String> = Vec::new();
    let line_start = |i: usize| i == 0 || b[i - 1] == b'\n';
    let push = |out: &mut Vec<Statement>, from: usize, to: usize, reach_end: usize| {
        let piece = &text[from..to];
        let lead = piece.len() - piece.trim_start().len();
        let trimmed = piece.trim();
        if !is_blank(trimmed, dialect) {
            let s = from + lead;
            out.push(Statement { range: s..s + trimmed.len(), reach: from..reach_end, text: trimmed.to_string() });
        }
    };
    while i < b.len() {
        let c = b[i];
        match &mut state {
            In::Code => {
                // A line that's only `DELIMITER x` (MySQL's clients), or
                // `GO` (SQL Server's).
                if line_start(i) {
                    let line_end = text[i..].find('\n').map_or(b.len(), |n| i + n);
                    let line = text[i..line_end].trim();
                    if dialect == Dialect::Mysql && line.len() > 10 && line[..10].eq_ignore_ascii_case("delimiter ") {
                        push(&mut out, start, i, i);
                        delimiter = line[10..].trim().to_string();
                        i = line_end;
                        start = i;
                        words.clear();
                        continue;
                    }
                    if dialect == Dialect::Mssql && is_go(line) {
                        push(&mut out, start, i, line_end);
                        i = line_end;
                        start = i;
                        words.clear();
                        depth = 0;
                        continue;
                    }
                }
                if delimiter != ";" && text[i..].starts_with(delimiter.as_str()) {
                    let end = i + delimiter.len();
                    push(&mut out, start, i, end);
                    i = end;
                    start = i;
                    words.clear();
                    continue;
                }
                match c {
                    b';' if delimiter == ";" && depth == 0 => {
                        push(&mut out, start, i, i + 1);
                        start = i + 1;
                        words.clear();
                    }
                    b'\'' => {
                        state = In::Quoted {
                            close: b'\'',
                            backslash: dialect == Dialect::Mysql || (dialect == Dialect::Postgres && i > 0 && matches!(b[i - 1], b'e' | b'E') && (i < 2 || !is_word(b[i - 2]))),
                        }
                    }
                    b'"' => state = In::Quoted { close: b'"', backslash: dialect == Dialect::Mysql },
                    b'`' if matches!(dialect, Dialect::Mysql | Dialect::Sqlite) => state = In::Quoted { close: b'`', backslash: false },
                    b'[' if matches!(dialect, Dialect::Mssql | Dialect::Sqlite) => state = In::Quoted { close: b']', backslash: false },
                    b'-' if b.get(i + 1) == Some(&b'-') => state = In::LineComment,
                    b'#' if dialect == Dialect::Mysql => state = In::LineComment,
                    b'/' if b.get(i + 1) == Some(&b'*') => {
                        state = In::BlockComment(1);
                        i += 1;
                    }
                    b'$' if dialect == Dialect::Postgres && (i == 0 || !is_word(b[i - 1])) => {
                        if let Some(tag) = dollar_tag(&b[i..]) {
                            i += tag.len() - 1;
                            state = In::Dollar(tag);
                        }
                    }
                    c if is_word(c) && (i == 0 || !is_word(b[i - 1])) => {
                        let end = (i..b.len()).find(|&j| !is_word(b[j])).unwrap_or(b.len());
                        let word = text[i..end].to_ascii_uppercase();
                        if words.len() < 4 {
                            words.push(word.clone());
                        }
                        if trigger(&words, dialect) {
                            match word.as_str() {
                                "BEGIN" | "CASE" => depth += 1,
                                "END" => depth = depth.saturating_sub(1),
                                _ => {}
                            }
                        }
                        i = end;
                        continue;
                    }
                    _ => {}
                }
            }
            In::Quoted { close, backslash } => {
                if *backslash && c == b'\\' {
                    i += 1;
                } else if c == *close {
                    // Doubled, it's itself.
                    if b.get(i + 1) == Some(close) {
                        i += 1;
                    } else {
                        state = In::Code;
                    }
                }
            }
            In::LineComment => {
                if c == b'\n' {
                    state = In::Code;
                    // The next line may be a GO or a DELIMITER: look at it
                    // from its start.
                    i += 1;
                    continue;
                }
            }
            In::BlockComment(n) => {
                if c == b'*' && b.get(i + 1) == Some(&b'/') {
                    *n -= 1;
                    i += 1;
                    if *n == 0 {
                        state = In::Code;
                    }
                } else if dialect == Dialect::Postgres && c == b'/' && b.get(i + 1) == Some(&b'*') {
                    *n += 1;
                    i += 1;
                }
            }
            In::Dollar(tag) => {
                if c == b'$' && text[i..].starts_with(tag.as_str()) {
                    i += tag.len();
                    state = In::Code;
                    continue;
                }
            }
        }
        i += 1;
    }
    push(&mut out, start, b.len(), b.len());
    out
}

/// Whether a statement's first words make it one whose body has `;`s:
/// SQLite's `CREATE [TEMP] TRIGGER`.
fn trigger(words: &[String], dialect: Dialect) -> bool {
    if dialect != Dialect::Sqlite || words.first().map(String::as_str) != Some("CREATE") {
        return false;
    }
    words.iter().skip(1).take(2).any(|w| w == "TRIGGER")
}

fn is_word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80
}

/// `GO`, or `GO 5`, alone on a line.
fn is_go(line: &str) -> bool {
    let mut parts = line.split_whitespace();
    parts.next().is_some_and(|w| w.eq_ignore_ascii_case("go")) && parts.next().is_none_or(|n| n.parse::<u32>().is_ok()) && parts.next().is_none()
}

/// `$tag$` at the start of `b` -- `$$` or `$name$`, not `$1`.
fn dollar_tag(b: &[u8]) -> Option<String> {
    let close = b.iter().skip(1).position(|&c| c == b'$')? + 1;
    let tag = &b[1..close];
    let ok = tag.is_empty() || (!tag[0].is_ascii_digit() && tag.iter().all(|&c| is_word(c)));
    ok.then(|| String::from_utf8_lossy(&b[..=close]).into_owned())
}

/// Whether `text` holds nothing but blanks and comments.
fn is_blank(text: &str, dialect: Dialect) -> bool {
    strip_leading(text, dialect).is_empty()
}

/// `text` from its first word: blanks, comments and opening parentheses
/// before it left out.
pub fn strip_leading(text: &str, dialect: Dialect) -> &str {
    let mut s = text;
    loop {
        let t = s.trim_start().trim_start_matches('(').trim_start();
        if let Some(rest) = t.strip_prefix("--").or_else(|| if dialect == Dialect::Mysql { t.strip_prefix('#') } else { None }) {
            s = rest.find('\n').map_or("", |n| &rest[n..]);
        } else if let Some(rest) = t.strip_prefix("/*") {
            s = rest.find("*/").map_or("", |n| &rest[n + 2..]);
        } else {
            return t;
        }
    }
}

/// The statement the cursor's byte `at` is in: the one whose reach has it,
/// or, on a line after one, the last before it.
pub fn statement_at(text: &str, at: usize, dialect: Dialect) -> Option<Statement> {
    let all = split(text, dialect);
    let at = at.min(text.len());
    if let Some(s) = all.iter().find(|s| s.range.start <= at && at <= s.range.end) {
        return Some(s.clone());
    }
    // In what's between two: the one its reach ends in, if it's on the
    // cursor's line (just after its `;`); else the next one, else the last.
    let line_of = |i: usize| text[..i.min(text.len())].matches('\n').count();
    if let Some(s) = all.iter().rev().find(|s| s.range.end <= at && line_of(s.range.end) == line_of(at)) {
        return Some(s.clone());
    }
    all.iter().find(|s| s.range.start > at).or(all.last()).cloned()
}

/// A (line, column) place, in characters from 0, as a byte offset into
/// `text`.
pub fn offset(text: &str, line: u32, column: u32) -> usize {
    let mut at = 0;
    for (n, l) in text.split_inclusive('\n').enumerate() {
        if n as u32 == line {
            return at + l.char_indices().nth(column as usize).map_or(l.trim_end_matches(['\n', '\r']).len(), |(i, _)| i);
        }
        at += l.len();
    }
    text.len()
}

/// What a statement does, for the safety rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// It only reads: SELECT, SHOW, EXPLAIN, a PRAGMA that reads.
    Read,
    /// It changes rows: INSERT, UPDATE, DELETE, MERGE, TRUNCATE, COPY.
    Write,
    /// It changes the schema or rights: CREATE, ALTER, DROP, GRANT.
    Schema,
    /// The session's: BEGIN, COMMIT, SET, USE.
    Session,
    /// Can't tell -- a procedure called, a block run -- so it's taken as
    /// one that changes data.
    Unknown,
}

impl Kind {
    /// Whether it may change what's stored.
    pub fn changes(self) -> bool {
        matches!(self, Kind::Write | Kind::Schema | Kind::Unknown)
    }
}

/// The words of `text` outside quotes and comments, upper-case, with
/// their depth in parentheses.
fn words(text: &str, dialect: Dialect) -> Vec<(String, usize)> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let mut depth = 0usize;
    while i < b.len() {
        let c = b[i];
        match c {
            b'\'' | b'"' | b'`' | b'[' => {
                if c == b'[' && !matches!(dialect, Dialect::Mssql | Dialect::Sqlite) || c == b'`' && !matches!(dialect, Dialect::Mysql | Dialect::Sqlite) {
                    i += 1;
                    continue;
                }
                let close = if c == b'[' { b']' } else { c };
                let backslash = dialect == Dialect::Mysql && c != b'`';
                i += 1;
                while i < b.len() {
                    if backslash && b[i] == b'\\' {
                        i += 2;
                        continue;
                    }
                    if b[i] == close {
                        if b.get(i + 1) == Some(&close) {
                            i += 2;
                            continue;
                        }
                        break;
                    }
                    i += 1;
                }
                i += 1;
            }
            b'-' if b.get(i + 1) == Some(&b'-') => i = text[i..].find('\n').map_or(b.len(), |n| i + n),
            b'#' if dialect == Dialect::Mysql => i = text[i..].find('\n').map_or(b.len(), |n| i + n),
            b'/' if b.get(i + 1) == Some(&b'*') => i = text[i + 2..].find("*/").map_or(b.len(), |n| i + 2 + n + 2),
            b'$' if dialect == Dialect::Postgres && (i == 0 || !is_word(b[i - 1])) => match dollar_tag(&b[i..]) {
                Some(tag) => {
                    let body = i + tag.len();
                    i = text[body..].find(tag.as_str()).map_or(b.len(), |n| body + n + tag.len());
                }
                None => i += 1,
            },
            b'(' => {
                depth += 1;
                i += 1;
            }
            b')' => {
                depth = depth.saturating_sub(1);
                i += 1;
            }
            c if is_word(c) => {
                let end = (i..b.len()).find(|&j| !is_word(b[j])).unwrap_or(b.len());
                out.push((text[i..end].to_ascii_uppercase(), depth));
                i = end;
            }
            _ => i += 1,
        }
    }
    out
}

/// What statement `text` (one statement) does.
pub fn classify(text: &str, dialect: Dialect) -> Kind {
    let all = words(text, dialect);
    let Some((first, _)) = all.first() else { return Kind::Read };
    let has = |w: &str| all.iter().any(|(x, _)| x == w);
    let top = |w: &str| all.iter().any(|(x, d)| x == w && *d == 0);
    match first.as_str() {
        "SELECT" => {
            // SELECT ... INTO makes a table (PostgreSQL, SQL Server) or a
            // file (MySQL's OUTFILE); into @variables only sets them.
            let into = all.iter().position(|(x, d)| x == "INTO" && *d == 0);
            match into {
                Some(_) if dialect == Dialect::Mysql && text_after_word(text, "INTO").trim_start().starts_with('@') => Kind::Read,
                Some(i) if all.get(i + 1).is_some_and(|(w, _)| w == "OUTFILE" || w == "DUMPFILE") => Kind::Write,
                Some(_) => Kind::Write,
                None => Kind::Read,
            }
        }
        "VALUES" | "TABLE" | "SHOW" | "DESCRIBE" | "DESC" | "HELP" | "LIST" => Kind::Read,
        "EXPLAIN" => {
            // EXPLAIN ANALYZE runs it: it's what it explains.
            if has("ANALYZE") || has("ANALYSE") {
                let inner = all.iter().skip(1).find(|(w, _)| {
                    !matches!(
                        w.as_str(),
                        "ANALYZE"
                            | "ANALYSE"
                            | "VERBOSE"
                            | "BUFFERS"
                            | "COSTS"
                            | "FORMAT"
                            | "JSON"
                            | "TEXT"
                            | "TRUE"
                            | "FALSE"
                            | "ON"
                            | "OFF"
                            | "TIMING"
                            | "SETTINGS"
                            | "WAL"
                            | "SUMMARY"
                            | "QUERY"
                            | "PLAN"
                    )
                });
                match inner.map(|(w, _)| w.as_str()) {
                    Some("SELECT" | "VALUES" | "TABLE") | None => Kind::Read,
                    Some("WITH") => with_kind(&all),
                    Some(_) => Kind::Write,
                }
            } else {
                Kind::Read
            }
        }
        "WITH" => with_kind(&all),
        "PRAGMA" => {
            // `PRAGMA x = y` and `PRAGMA x(y)` set; a few that read change
            // the file all the same.
            let name = all.get(1).map(|(w, _)| w.as_str()).unwrap_or("");
            let name = if all.get(2).is_some_and(|(_, d)| *d == 0) && text.contains('.') { all.get(2).map(|(w, _)| w.as_str()).unwrap_or(name) } else { name };
            let reads_with_argument = matches!(
                name,
                "TABLE_INFO" | "TABLE_XINFO" | "INDEX_LIST" | "INDEX_INFO" | "INDEX_XINFO" | "FOREIGN_KEY_LIST" | "FOREIGN_KEY_CHECK" | "INTEGRITY_CHECK" | "QUICK_CHECK" | "TABLE_LIST"
            );
            if text.contains('=') || matches!(name, "OPTIMIZE" | "INCREMENTAL_VACUUM" | "WAL_CHECKPOINT" | "SHRINK_MEMORY") || (text.contains('(') && !reads_with_argument) {
                Kind::Schema
            } else {
                Kind::Read
            }
        }
        "INSERT" | "UPDATE" | "DELETE" | "MERGE" | "REPLACE" | "UPSERT" | "TRUNCATE" | "COPY" | "LOAD" | "IMPORT" | "BULK" | "WRITETEXT" | "UPDATETEXT" => Kind::Write,
        "CREATE" | "ALTER" | "DROP" | "RENAME" | "GRANT" | "REVOKE" | "DENY" | "COMMENT" | "VACUUM" | "REINDEX" | "CLUSTER" | "REFRESH" | "ATTACH" | "DETACH" | "OPTIMIZE" | "REPAIR" | "FLUSH"
        | "PURGE" | "SECURITY" | "DBCC" | "BACKUP" | "RESTORE" | "KILL" | "SHUTDOWN" => Kind::Schema,
        // SQLite's ANALYZE writes its statistics; PostgreSQL's too.
        "ANALYZE" | "ANALYSE" => Kind::Schema,
        "SET" if dialect == Dialect::Mysql && (has("GLOBAL") || has("PERSIST")) => Kind::Schema,
        "DECLARE" if dialect == Dialect::Mssql && ["INSERT", "UPDATE", "DELETE", "MERGE", "EXEC", "EXECUTE"].iter().any(|w| top(w)) => Kind::Unknown,
        "BEGIN" if dialect == Dialect::Mssql && top("TRY") => Kind::Unknown,
        "BEGIN" | "START" | "COMMIT" | "END" | "ROLLBACK" | "SAVEPOINT" | "RELEASE" | "SET" | "RESET" | "USE" | "LISTEN" | "UNLISTEN" | "NOTIFY" | "DISCARD" | "LOCK" | "UNLOCK" | "PREPARE"
        | "DEALLOCATE" | "FETCH" | "CLOSE" | "DECLARE" | "PRINT" | "CHECKPOINT" => Kind::Session,
        _ => Kind::Unknown,
    }
}

/// A `WITH`'s: what its main statement does, or any of its parts.
fn with_kind(all: &[(String, usize)]) -> Kind {
    if all.iter().any(|(w, _)| matches!(w.as_str(), "INSERT" | "UPDATE" | "DELETE" | "MERGE")) {
        Kind::Write
    } else {
        Kind::Read
    }
}

/// What follows the first `word` (any case) in `text`.
fn text_after_word<'a>(text: &'a str, word: &str) -> &'a str {
    let upper = text.to_ascii_uppercase();
    upper.find(&format!("{word} ")).map_or("", |i| &text[i + word.len()..])
}

/// Whether an UPDATE or DELETE has no WHERE: every row.
pub fn every_row(text: &str, dialect: Dialect) -> bool {
    let all = words(text, dialect);
    match all.first().map(|(w, _)| w.as_str()) {
        Some("UPDATE" | "DELETE") => !all.iter().any(|(w, d)| w == "WHERE" && *d == 0),
        Some("TRUNCATE") => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(text: &str, d: Dialect) -> Vec<String> {
        split(text, d).into_iter().map(|s| s.text).collect()
    }

    #[test]
    fn statements_split_at_semicolons_outside_quotes_and_comments() {
        let text = "select 'a;b', \"c;d\" from t; -- x; y\nselect 2 /* ; */;\n\n  -- only a comment;\n";
        assert_eq!(texts(text, Dialect::Postgres), ["select 'a;b', \"c;d\" from t", "-- x; y\nselect 2 /* ; */"]);
        assert_eq!(texts("select 'it''s; fine'; select 3", Dialect::Sqlite), ["select 'it''s; fine'", "select 3"]);
        let s = &split("  select 1 ;", Dialect::Sqlite)[0];
        assert_eq!((s.range.clone(), s.reach.clone()), (2..10, 0..12));
    }

    #[test]
    fn dollar_quoting_keeps_a_function_body_whole() {
        let text = "create function f() returns int as $body$ begin; return 1; end; $body$ language plpgsql;\nselect $1, $$a;b$$;";
        assert_eq!(texts(text, Dialect::Postgres), ["create function f() returns int as $body$ begin; return 1; end; $body$ language plpgsql", "select $1, $$a;b$$"]);
        // A nested comment, PostgreSQL's.
        assert_eq!(texts("select /* a /* b; */ c; */ 1; select 2", Dialect::Postgres), ["select /* a /* b; */ c; */ 1", "select 2"]);
        // E'' strings escape with backslashes; plain ones don't.
        assert_eq!(texts(r"select E'a\';b'; select 'c\'; select 3", Dialect::Postgres), [r"select E'a\';b'", r"select 'c\'", "select 3"]);
    }

    #[test]
    fn mysql_backslashes_hashes_backticks_and_delimiter() {
        let text = "select 'a\\';b' # c; d\n, `x;y` from t;\nDELIMITER $$\ncreate procedure p() begin select 1; select 2; end$$\nDELIMITER ;\nselect 4;";
        assert_eq!(texts(text, Dialect::Mysql), ["select 'a\\';b' # c; d\n, `x;y` from t", "create procedure p() begin select 1; select 2; end", "select 4"]);
    }

    #[test]
    fn sql_server_splits_at_go_and_brackets_hold_semicolons() {
        let text = "select [a;b] from t\nGO\ncreate procedure p as begin select 1 end\ngo 2\nselect 3";
        assert_eq!(texts(text, Dialect::Mssql), ["select [a;b] from t", "create procedure p as begin select 1 end", "select 3"]);
        assert!(!is_go("good"), "a word that starts with go");
    }

    #[test]
    fn a_sqlite_trigger_keeps_its_body() {
        let text = "create trigger t after insert on a begin update b set n = n + 1; insert into c values (case when 1 then 2 end); end; select 1;";
        assert_eq!(texts(text, Dialect::Sqlite), ["create trigger t after insert on a begin update b set n = n + 1; insert into c values (case when 1 then 2 end); end", "select 1"]);
    }

    #[test]
    fn the_statement_under_the_cursor() {
        let text = "select 1;\nselect 2;\n\nselect\n  3";
        let at = |line, col| statement_at(text, offset(text, line, col), Dialect::Postgres).map(|s| s.text);
        assert_eq!(at(0, 3).as_deref(), Some("select 1"));
        assert_eq!(at(0, 9).as_deref(), Some("select 1"), "just after its ;");
        assert_eq!(at(1, 0).as_deref(), Some("select 2"));
        assert_eq!(at(2, 0).as_deref(), Some("select\n  3"), "a blank line: the next");
        assert_eq!(at(4, 3).as_deref(), Some("select\n  3"));
        assert_eq!(statement_at("", 0, Dialect::Sqlite), None);
        assert_eq!(offset("aé\nb", 0, 2), 3, "in bytes, past the é");
        assert_eq!(offset("aé\nb", 1, 1), 5);
    }

    #[test]
    fn kinds_of_statements() {
        let k = |s: &str| classify(s, Dialect::Postgres);
        assert_eq!(k("  -- note\n(select 1)"), Kind::Read);
        assert_eq!(k("with x as (select 1) select * from x"), Kind::Read);
        assert_eq!(k("with x as (delete from t returning *) select * from x"), Kind::Write);
        assert_eq!(k("explain select 1"), Kind::Read);
        assert_eq!(k("explain analyze delete from t"), Kind::Write);
        assert_eq!(k("explain (analyze, buffers) select 1"), Kind::Read);
        assert_eq!(k("select * into t2 from t"), Kind::Write);
        assert_eq!(k("select 'insert' from t"), Kind::Read, "a word in a string");
        assert_eq!(k("update t set a = 1"), Kind::Write);
        assert_eq!(k("drop table t"), Kind::Schema);
        assert_eq!(k("begin"), Kind::Session);
        assert_eq!(k("set search_path = x"), Kind::Session);
        assert_eq!(k("call p()"), Kind::Unknown);
        assert_eq!(k("do $$ begin delete from t; end $$"), Kind::Unknown);
        assert_eq!(classify("select @a := 1 into @b", Dialect::Mysql), Kind::Read);
        assert_eq!(classify("select * from t into outfile '/tmp/x'", Dialect::Mysql), Kind::Write);
        assert_eq!(classify("set global max_connections = 10", Dialect::Mysql), Kind::Schema);
        assert_eq!(classify("pragma table_info(t)", Dialect::Sqlite), Kind::Read);
        assert_eq!(classify("pragma journal_mode = wal", Dialect::Sqlite), Kind::Schema);
        assert_eq!(classify("pragma user_version", Dialect::Sqlite), Kind::Read);
        assert_eq!(classify("exec sp_who", Dialect::Mssql), Kind::Unknown);
        assert!(Kind::Unknown.changes() && !Kind::Session.changes());
    }

    #[test]
    fn every_row_is_an_update_or_delete_without_where() {
        assert!(every_row("delete from t", Dialect::Postgres));
        assert!(!every_row("delete from t where id = 1", Dialect::Postgres));
        assert!(every_row("update t set a = (select b from u where u.id = 1)", Dialect::Postgres), "the WHERE is a subquery's");
        assert!(!every_row("select * from t", Dialect::Postgres));
    }

    #[test]
    fn quoting() {
        assert_eq!(Dialect::Postgres.ident("a\"b"), "\"a\"\"b\"");
        assert_eq!(Dialect::Mysql.ident("a`b"), "`a``b`");
        assert_eq!(Dialect::Mssql.ident("a]b"), "[a]]b]");
        assert_eq!(Dialect::Mysql.string("it's \\"), "'it''s \\\\'");
        assert_eq!(Dialect::Mssql.string("it's"), "N'it''s'");
        assert_eq!(Dialect::Postgres.table(Some("public"), "t"), "\"public\".\"t\"");
        assert_eq!(Dialect::Sqlite.table(None, "t"), "\"t\"");
    }
}
