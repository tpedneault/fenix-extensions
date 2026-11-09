use std::path::Path;
use std::sync::Arc;

/// Where a `Row` came from -- which configured root, which table, which
/// `.dat` file (the line is the row's own, `Row::line`). `root_index`
/// (not just `root_label`) is what lets cross-reference lookups (a
/// telecommand's parameters, a parameter's calibration curve, ...) stay
/// scoped to the *same* MIB root a row was found in, instead of
/// accidentally resolving a name against a different root that happens
/// to define something by the same name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowSource {
    pub root_index: usize,
    pub root_label: Arc<str>,
    pub table: Arc<str>,
    pub file: Arc<Path>,
}

/// What every row of a table file shares: where they're from, and the
/// table's column names.
#[derive(Debug, PartialEq, Eq)]
pub struct RowShape {
    pub source: RowSource,
    columns: Arc<[String]>,
}

impl RowShape {
    pub fn new(source: RowSource, columns: Arc<[String]>) -> Arc<RowShape> {
        Arc::new(RowShape { source, columns })
    }
}

/// One parsed MIB row: a table's column names paired with this line's
/// tab-separated values, in schema order.
///
/// A row is where its line is in its file's text, kept once for every
/// row of the file -- not a string of its own per field: a mission's MIB
/// has hundreds of thousands of rows and millions of fields, and a copy
/// of each ran the extension out of memory.
#[derive(Clone)]
pub struct Row {
    shape: Arc<RowShape>,
    text: Arc<str>,
    start: u32,
    len: u32,
    line: u32,
    /// How many fields the line has (at most the table's columns).
    count: u16,
}

/// A value as a row's text keeps it: one line, fields between tabs.
fn flat(v: &str) -> std::borrow::Cow<'_, str> {
    if v.contains(['\t', '\n', '\r']) {
        v.replace(['\t', '\n', '\r'], " ").into()
    } else {
        v.into()
    }
}

impl Row {
    /// The row on `text[start..start + len]`, line `line` of the file.
    pub fn in_text(shape: &Arc<RowShape>, text: &Arc<str>, start: usize, len: usize, line: usize) -> Self {
        let count = text[start..start + len].split('\t').count().min(shape.columns.len());
        Self { shape: shape.clone(), text: text.clone(), start: start as u32, len: len as u32, line: line as u32, count: count as u16 }
    }

    /// A row of `values` (the first of the shape's columns), as a line of
    /// its own.
    pub fn with_values<'a>(shape: &Arc<RowShape>, values: impl IntoIterator<Item = &'a str>, line: usize) -> Self {
        let values: Vec<std::borrow::Cow<str>> = values.into_iter().take(shape.columns.len()).map(flat).collect();
        let count = values.len();
        let text: Arc<str> = values.join("\t").into();
        let len = text.len();
        Self { shape: shape.clone(), text, start: 0, len: len as u32, line: line as u32, count: count as u16 }
    }

    /// A row of its own `(name, value)` pairs (an XTCE file's, whose
    /// fields vary from row to row).
    pub fn from_pairs(table: Arc<str>, fields: Vec<(String, String)>, source: RowSource, line: usize) -> Self {
        debug_assert_eq!(table, source.table);
        let (columns, values): (Vec<String>, Vec<String>) = fields.into_iter().unzip();
        let shape = RowShape::new(source, columns.into());
        Self::with_values(&shape, values.iter().map(String::as_str), line)
    }

    /// The table it's a row of (`"ccf"`).
    pub fn table(&self) -> &str {
        &self.shape.source.table
    }

    pub fn source(&self) -> &RowSource {
        &self.shape.source
    }

    /// 1-based line number within the table file.
    pub fn line(&self) -> usize {
        self.line as usize
    }

    fn text(&self) -> &str {
        &self.text[self.start as usize..(self.start + self.len) as usize]
    }

    fn values(&self) -> impl Iterator<Item = &str> {
        let text = self.text();
        let n = self.count as usize;
        // A row with no fields has no text, not one empty field.
        text.split('\t').take(n)
    }

    /// The value of `field` (e.g. `"CCF_CNAME"`) on this row, or `None`
    /// if the row has no such field -- either an unknown name, or a
    /// line with fewer tab-separated values than the schema has columns
    /// (a malformed/truncated `.dat` line; see `parse::parse_table_file`).
    pub fn get(&self, field: &str) -> Option<&str> {
        let i = self.shape.columns.iter().position(|name| name == field)?;
        if i >= self.count as usize {
            return None;
        }
        self.values().nth(i)
    }

    /// `get`, with surrounding whitespace trimmed and a missing field
    /// treated the same as an empty one -- MIB fields routinely carry
    /// presentation padding, and most callers want "is there a real
    /// value here" rather than "does this field exist."
    pub fn clean(&self, field: &str) -> &str {
        self.get(field).map(str::trim).unwrap_or("")
    }

    /// Its fields, `(name, value)`, in file order.
    pub fn fields(&self) -> impl Iterator<Item = (&str, &str)> {
        self.shape.columns.iter().map(String::as_str).zip(self.values())
    }

    /// How many fields the line had (at most the table's columns).
    pub fn field_count(&self) -> usize {
        self.count as usize
    }
}

impl PartialEq for Row {
    fn eq(&self, other: &Self) -> bool {
        self.line == other.line && self.shape.source == other.shape.source && self.fields().eq(other.fields())
    }
}

impl Eq for Row {}

impl std::fmt::Debug for Row {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Row").field("table", &self.table()).field("line", &self.line).field("fields", &self.fields().collect::<Vec<_>>()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> RowSource {
        RowSource { root_index: 0, root_label: "TEST".into(), table: "ccf".into(), file: Path::new("ccf.dat").into() }
    }

    fn row(fields: &[(&str, &str)]) -> Row {
        Row::from_pairs("ccf".into(), fields.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(), source(), 1)
    }

    #[test]
    fn get_returns_the_matching_field_value() {
        let r = row(&[("CCF_CNAME", "AAA001"), ("CCF_APID", "100")]);
        assert_eq!(r.get("CCF_APID"), Some("100"));
    }

    #[test]
    fn get_is_none_for_an_unknown_field() {
        let r = row(&[("CCF_CNAME", "AAA001")]);
        assert_eq!(r.get("CCF_NOPE"), None);
    }

    #[test]
    fn clean_trims_whitespace_and_treats_missing_as_empty() {
        let r = row(&[("CCF_DESCR", "  padded  ")]);
        assert_eq!(r.clean("CCF_DESCR"), "padded");
        assert_eq!(r.clean("CCF_NOPE"), "");
    }

    #[test]
    fn a_short_line_has_its_first_columns_only() {
        let columns: Arc<[String]> = vec!["A".to_string(), "B".to_string(), "C".to_string()].into();
        let shape = RowShape::new(source(), columns);
        let text: Arc<str> = "x\n1\ny".into();
        let r = Row::in_text(&shape, &text, 2, 1, 2);
        assert_eq!((r.get("A"), r.get("B"), r.field_count(), r.line()), (Some("1"), None, 1, 2));
        assert_eq!(r.fields().collect::<Vec<_>>(), vec![("A", "1")]);
        // Tabs and newlines in a value don't split it.
        let r = Row::with_values(&shape, ["a\tb", "c\nd", ""], 1);
        assert_eq!(r.fields().collect::<Vec<_>>(), vec![("A", "a b"), ("B", "c d"), ("C", "")]);
        assert_eq!(Row::from_pairs("ccf".into(), Vec::new(), source(), 1).field_count(), 0);
    }
}
