//! What separates a file's fields, what quotes them, and whether its
//! first record names the columns: guessed from its first lines, and
//! overridden per file.

use serde::{Deserialize, Serialize};

use crate::table::{parse_records, split_record};

/// The delimiters it guesses between, and `;` cycles through.
pub const DELIMITERS: [u8; 4] = *b",;\t|";

/// The quotes `"` cycles through: double, single, none.
pub const QUOTES: [Option<u8>; 3] = [Some(b'"'), Some(b'\''), None];

/// How much of a file the guess reads.
const SAMPLE: usize = 64 * 1024;
/// And how many records of it.
const SAMPLE_RECORDS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dialect {
    pub delimiter: u8,
    pub quote: Option<u8>,
    pub header: bool,
}

impl Default for Dialect {
    fn default() -> Dialect {
        Dialect { delimiter: b',', quote: Some(b'"'), header: true }
    }
}

impl Dialect {
    /// `,` `;` `tab` `|`, as the title line says it.
    pub fn delimiter_name(&self) -> String {
        match self.delimiter {
            b'\t' => "tab".into(),
            d => (d as char).to_string(),
        }
    }

    pub fn quote_name(&self) -> &'static str {
        match self.quote {
            Some(b'"') => "\"",
            Some(b'\'') => "'",
            Some(_) => "quoted",
            None => "no quotes",
        }
    }

    /// The next delimiter of [`DELIMITERS`].
    pub fn next_delimiter(mut self) -> Dialect {
        let i = DELIMITERS.iter().position(|&d| d == self.delimiter).map_or(0, |i| (i + 1) % DELIMITERS.len());
        self.delimiter = DELIMITERS[i];
        self
    }

    pub fn next_quote(mut self) -> Dialect {
        let i = QUOTES.iter().position(|&q| q == self.quote).map_or(0, |i| (i + 1) % QUOTES.len());
        self.quote = QUOTES[i];
        self
    }
}

/// The start of `text` the guess reads: whole lines of it.
fn sample(text: &str) -> &str {
    if text.len() <= SAMPLE {
        return text;
    }
    let mut end = SAMPLE;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let cut = &text[..end];
    match cut.rfind('\n') {
        Some(i) if i > 0 => &cut[..i],
        _ => cut,
    }
}

/// Guesses `text`'s dialect. `tab_first`: a `.tsv`, where a tab wins a
/// tie.
pub fn detect(text: &str, tab_first: bool) -> Dialect {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let sample = sample(text);
    let quote = Some(b'"');
    let mut order = DELIMITERS.to_vec();
    if tab_first {
        order.sort_by_key(|&d| d != b'\t');
    }
    // The delimiter that splits the most lines into the same number of
    // fields, more than one; then the one with more fields.
    let mut best: Option<(u8, (usize, usize))> = None;
    for &d in &order {
        let dialect = Dialect { delimiter: d, quote, header: false };
        let counts: Vec<usize> = parse_records(sample.as_bytes(), &dialect).into_iter().take(SAMPLE_RECORDS).map(|(_, n)| n).collect();
        let score = consistency(&counts);
        if score.1 > 1 && best.is_none_or(|(_, b)| score > b) {
            best = Some((d, score));
        }
    }
    let delimiter = best.map_or(if tab_first { b'\t' } else { b',' }, |(d, _)| d);
    let mut dialect = Dialect { delimiter, quote, header: false };
    dialect.header = has_header(sample, &dialect);
    dialect
}

/// How many of `counts` share the commonest count, and that count.
fn consistency(counts: &[usize]) -> (usize, usize) {
    let mut seen: Vec<(usize, usize)> = Vec::new();
    for &n in counts {
        match seen.iter_mut().find(|(c, _)| *c == n) {
            Some((_, k)) => *k += 1,
            None => seen.push((n, 1)),
        }
    }
    seen.into_iter().map(|(c, k)| (k, c)).max().unwrap_or((0, 0))
}

/// A number, as a cell holds one: `12`, `-3.5`, `1e6`, ` 7 `. Not
/// `inf` or `NaN`, which are words.
pub fn number(s: &str) -> Option<f64> {
    let s = s.trim();
    let first = s.bytes().next()?;
    if !(first.is_ascii_digit() || matches!(first, b'-' | b'+' | b'.')) {
        return None;
    }
    if !s.bytes().any(|b| b.is_ascii_digit()) || s.bytes().any(|b| b.is_ascii_alphabetic() && b != b'e' && b != b'E') {
        return None;
    }
    s.parse::<f64>().ok().filter(|n| n.is_finite())
}

/// Whether the first record names the columns: a column of numbers under
/// a name says so, a number at its head says not; text of one length
/// under a name of another says so. With nothing to go on, a first row
/// of distinct names that aren't numbers, and isn't a row again below,
/// is taken as a header.
pub fn has_header(text: &str, dialect: &Dialect) -> bool {
    let bytes = text.as_bytes();
    let records: Vec<Vec<String>> =
        parse_records(bytes, dialect).into_iter().take(SAMPLE_RECORDS).map(|((s, e), _)| split_record(&text[s..e], dialect).into_iter().map(|f| f.1.into_owned()).collect()).collect();
    let Some((first, rest)) = records.split_first() else { return false };
    if rest.is_empty() {
        return first.iter().all(|c| !c.trim().is_empty() && number(c).is_none());
    }
    let mut votes = 0i32;
    for (c, head) in first.iter().enumerate() {
        let below: Vec<&str> = rest.iter().filter_map(|r| r.get(c)).map(|s| s.as_str()).filter(|s| !s.trim().is_empty()).collect();
        if below.is_empty() {
            continue;
        }
        if below.iter().all(|s| number(s).is_some()) {
            votes += if number(head).is_some() { -1 } else { 1 };
            continue;
        }
        let len = below[0].chars().count();
        if below.iter().all(|s| s.chars().count() == len) && head.chars().count() != len {
            votes += 1;
        }
    }
    if votes != 0 {
        return votes > 0;
    }
    if rest.contains(first) {
        return false;
    }
    let mut names: Vec<&str> = first.iter().map(|s| s.trim()).collect();
    if names.iter().any(|n| n.is_empty() || number(n).is_some()) {
        return false;
    }
    names.sort_unstable();
    names.windows(2).all(|w| w[0] != w[1])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_delimiter_is_the_one_that_splits_lines_alike() {
        assert_eq!(detect("a,b,c\n1,2,3\n4,5,6\n", false).delimiter, b',');
        assert_eq!(detect("a;b;c\n1,5;2;3\n4;5;6\n", false).delimiter, b';', "decimal commas");
        assert_eq!(detect("a\tb\n1\t2\n", false).delimiter, b'\t');
        assert_eq!(detect("a|b|c\nx, y|2|3\n", false).delimiter, b'|');
        assert_eq!(detect("name,note\n\"Smith; John\",\"a; b; c\"\nx,y\n", false).delimiter, b',', "quoted ; don't count");
        assert_eq!(detect("one\ntwo\n", true).delimiter, b'\t', "nothing to go on: a .tsv's tab");
        assert_eq!(detect("one\ntwo\n", false).delimiter, b',');
        assert_eq!(detect("\u{feff}a;b\n1;2\n", false), Dialect { delimiter: b';', quote: Some(b'"'), header: true });
    }

    #[test]
    fn a_header_is_names_over_values() {
        let d = Dialect::default();
        assert!(has_header("id,price\n1,2.5\n2,3\n", &d));
        assert!(!has_header("1,2.5\n2,3\n", &d), "numbers all the way");
        assert!(has_header("code,name\nAB12,x\nCD34,yy\n", &d), "codes of one length under a name");
        assert!(has_header("name,city\nada,london\ngrace,arlington\n", &d), "distinct names, nothing against");
        assert!(!has_header("ada,london\nada,london\n", &d), "the same twice");
    }

    #[test]
    fn numbers_are_what_cells_hold() {
        assert_eq!(number(" 12 "), Some(12.0));
        assert_eq!(number("-3.5"), Some(-3.5));
        assert_eq!(number("1e3"), Some(1000.0));
        assert_eq!(number(".5"), Some(0.5));
        for s in ["", "inf", "NaN", "12a", "-", "x1", "1,5"] {
            assert_eq!(number(s), None, "{s}");
        }
    }

    #[test]
    fn delimiters_and_quotes_cycle() {
        let d = Dialect::default();
        assert_eq!(d.next_delimiter().delimiter, b';');
        assert_eq!(d.next_delimiter().next_delimiter().delimiter_name(), "tab");
        assert_eq!(Dialect { delimiter: b'|', ..d }.next_delimiter().delimiter, b',');
        assert_eq!(d.next_quote().quote, Some(b'\''));
        assert_eq!(d.next_quote().next_quote().quote_name(), "no quotes");
    }
}
