use std::path::Path;
use std::sync::Arc;

/// Where a `Row` came from -- which configured root, which table, which
/// line of that table's `.dat` file. `root_index` (not just `root_label`)
/// is what lets cross-reference lookups (a telecommand's parameters, a
/// parameter's calibration curve, ...) stay scoped to the *same* MIB root
/// a row was found in, instead of accidentally resolving a name against
/// a different root that happens to define something by the same name.
///
/// The label, table and file are shared by every row of a file: a
/// mission's MIB has hundreds of thousands of rows, and a copy of each per
/// row ran an extension out of memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowSource {
    pub root_index: usize,
    pub root_label: Arc<str>,
    pub table: Arc<str>,
    pub file: Arc<Path>,
    /// 1-based line number within the table file.
    pub line: usize,
}

/// One parsed MIB row: a table's column names paired with this line's
/// tab-separated values, in schema order. The names are the table's,
/// shared by its rows; a row keeps only its values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub table: Arc<str>,
    columns: Arc<[String]>,
    values: Vec<Box<str>>,
    pub source: RowSource,
}

impl Row {
    /// A row of `values` under `columns` -- as many as there are values,
    /// the rest of the columns absent.
    pub fn new(table: Arc<str>, columns: Arc<[String]>, values: Vec<Box<str>>, source: RowSource) -> Self {
        debug_assert!(values.len() <= columns.len());
        Self { table, columns, values, source }
    }

    /// A row of its own `(name, value)` pairs (an XTCE file's, whose
    /// fields vary from row to row).
    pub fn from_pairs(table: Arc<str>, fields: Vec<(String, String)>, source: RowSource) -> Self {
        let (columns, values): (Vec<String>, Vec<Box<str>>) = fields.into_iter().map(|(k, v)| (k, v.into_boxed_str())).unzip();
        Self { table, columns: columns.into(), values, source }
    }

    /// The value of `field` (e.g. `"CCF_CNAME"`) on this row, or `None`
    /// if the row has no such field -- either an unknown name, or a
    /// line with fewer tab-separated values than the schema has columns
    /// (a malformed/truncated `.dat` line; see `parse::parse_table_file`).
    pub fn get(&self, field: &str) -> Option<&str> {
        self.columns.iter().zip(&self.values).find(|(name, _)| *name == field).map(|(_, value)| &**value)
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
        self.columns.iter().zip(&self.values).map(|(name, value)| (name.as_str(), &**value))
    }

    /// How many fields the line had (at most the table's columns).
    pub fn field_count(&self) -> usize {
        self.values.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(fields: &[(&str, &str)]) -> Row {
        let source = RowSource { root_index: 0, root_label: "TEST".into(), table: "ccf".into(), file: Path::new("ccf.dat").into(), line: 1 };
        Row::from_pairs("ccf".into(), fields.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(), source)
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
        let source = RowSource { root_index: 0, root_label: "TEST".into(), table: "ccf".into(), file: Path::new("ccf.dat").into(), line: 1 };
        let columns: Arc<[String]> = vec!["A".to_string(), "B".to_string(), "C".to_string()].into();
        let r = Row::new("ccf".into(), columns, vec!["1".into()], source);
        assert_eq!((r.get("A"), r.get("B"), r.field_count()), (Some("1"), None, 1));
        assert_eq!(r.fields().collect::<Vec<_>>(), vec![("A", "1")]);
    }
}
