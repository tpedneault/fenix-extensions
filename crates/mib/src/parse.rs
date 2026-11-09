use std::path::{Path, PathBuf};

use crate::row::{Row, RowShape, RowSource};

/// Something wrong with a MIB's files or rows, and where: shown on the
/// MIB page, in the project's MIB settings and by the project doctor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub root: usize,
    pub file: PathBuf,
    /// 1-based, when it's about one row.
    pub line: Option<usize>,
    pub message: String,
}

/// Parses `table`'s `.dat` file under one MIB root into `Row`s, laid
/// out as the newest ICD issue has it. A missing file is normal (a MIB
/// root has only some of the known tables) and yields nothing. A file
/// that exists but can't be read, or isn't UTF-8, yields no rows and a
/// problem.
#[cfg(test)]
pub fn parse_table_file(root_index: usize, root_label: &str, table: &str, file: &Path) -> (Vec<Row>, Vec<Problem>) {
    let Some(columns) = crate::schema::columns(table) else { return (Vec::new(), Vec::new()) };
    let columns: Vec<String> = columns.iter().map(|c| c.to_string()).collect();
    match read_table_file(root_index, file) {
        Ok(Some((text, warning))) => {
            let (rows, mut problems) = parse_table_text(root_index, root_label, table, file, &columns, &format!("ICD {}", crate::schema::Icd::LATEST), &text);
            problems.extend(warning);
            (rows, problems)
        }
        Ok(None) => (Vec::new(), Vec::new()),
        Err(problem) => (Vec::new(), vec![problem]),
    }
}

/// A table file's text: `None` when there's no such file, a problem
/// when it can't be read. A file that isn't UTF-8 is read as Latin-1
/// (ISO 8859-1, what MIB exports often are: a degree sign or a micro in
/// a description), with a problem saying so and where.
pub fn read_table_file(root_index: usize, file: &Path) -> Result<Option<(String, Option<Problem>)>, Problem> {
    let problem = |line: Option<usize>, message: String| Problem { root: root_index, file: file.to_path_buf(), line, message };
    match std::fs::read(file) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(text) => Ok(Some((text, None))),
            Err(err) => {
                let bytes = err.into_bytes();
                let first = bytes.iter().position(|b| *b >= 0x80).unwrap_or(0);
                let line = bytes[..first].iter().filter(|b| **b == b'\n').count() + 1;
                let text: String = bytes.iter().map(|&b| b as char).collect();
                Ok(Some((text, Some(problem(Some(line), "isn't UTF-8 text -- read as Latin-1 (ISO 8859-1)".to_string())))))
            }
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(problem(None, format!("couldn't be read ({err})"))),
    }
}

/// A table file's text as rows of `columns`. Each non-empty line is
/// split on tabs and zipped against the column names: a line with fewer
/// values leaves the trailing columns absent (`Row::get` reports
/// `None`), and one with more drops the extras, with a problem saying
/// so -- `layout` names what has that many columns (`ICD 7.x`).
pub fn parse_table_text(root_index: usize, root_label: &str, table: &str, file: &Path, columns: &[String], layout: &str, contents: &str) -> (Vec<Row>, Vec<Problem>) {
    let problem = |line: Option<usize>, message: String| Problem { root: root_index, file: file.to_path_buf(), line, message };
    let mut rows = Vec::new();
    let mut problems = Vec::new();
    // What every row of the file shares: where it's from, its columns,
    // and its text -- each row is where its line is in it.
    let source = RowSource { root_index, root_label: root_label.into(), table: table.into(), file: file.into() };
    let shape = RowShape::new(source, columns.into());
    let text: std::sync::Arc<str> = contents.into();
    let mut at = 0;
    for (i, raw) in text.split_inclusive('\n').enumerate() {
        let start = at;
        at += raw.len();
        let line = raw.strip_suffix('\n').unwrap_or(raw);
        // A file written on Windows.
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        let count = line.split('\t').count();
        if count > columns.len() {
            problems.push(problem(Some(i + 1), format!("{count} columns, {layout} has {}; the extra ones were dropped", columns.len())));
        }
        rows.push(Row::in_text(&shape, &text, start, line.len(), i + 1));
    }
    (rows, problems)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_dir(name: &str) -> std::path::PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("fenix-mib-parse-test-{name}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn parses_one_row_per_non_empty_line() {
        let dir = temp_dir("basic");
        let file = dir.join("prv.dat");
        std::fs::write(&file, "1\t0\t100\n2\t-10\t10\n").unwrap();

        let rows = parse_table_file(0, "TEST", "prv", &file).0;

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get("PRV_NUMBR"), Some("1"));
        assert_eq!(rows[0].get("PRV_MINVAL"), Some("0"));
        assert_eq!(rows[0].get("PRV_MAXVAL"), Some("100"));
        assert_eq!(rows[1].get("PRV_NUMBR"), Some("2"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn tracks_source_file_and_1_based_line_number() {
        let dir = temp_dir("source");
        let file = dir.join("prv.dat");
        std::fs::write(&file, "1\t0\t1\n2\t0\t1\n").unwrap();

        let rows = parse_table_file(3, "MIB-D", "prv", &file).0;

        assert_eq!(rows[0].source().root_index, 3);
        assert_eq!(&*rows[0].source().root_label, "MIB-D");
        assert_eq!(&*rows[0].source().file, file.as_path());
        assert_eq!(rows[0].line(), 1);
        assert_eq!(rows[1].line(), 2);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_line_with_fewer_values_than_schema_columns_leaves_the_rest_absent() {
        let dir = temp_dir("short-line");
        let file = dir.join("prv.dat");
        std::fs::write(&file, "1\t0\n").unwrap(); // PRV_MAXVAL never written

        let rows = parse_table_file(0, "TEST", "prv", &file).0;

        assert_eq!(rows[0].get("PRV_MINVAL"), Some("0"));
        assert_eq!(rows[0].get("PRV_MAXVAL"), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_line_with_more_values_than_schema_columns_drops_the_extras() {
        let dir = temp_dir("long-line");
        let file = dir.join("prv.dat");
        std::fs::write(&file, "1\t0\t100\textra\tstuff\n").unwrap();

        let rows = parse_table_file(0, "TEST", "prv", &file).0;

        assert_eq!(rows[0].field_count(), 3); // schema has exactly 3 columns
        let (_, problems) = parse_table_file(0, "TEST", "prv", &file);
        assert_eq!(problems[0].line, Some(1));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn windows_line_endings_are_not_part_of_the_last_value() {
        let dir = temp_dir("crlf");
        let file = dir.join("prv.dat");
        std::fs::write(&file, "1\t0\t100\r\n").unwrap();
        assert_eq!(parse_table_file(0, "TEST", "prv", &file).0[0].get("PRV_MAXVAL"), Some("100"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn blank_lines_are_skipped() {
        let dir = temp_dir("blank-lines");
        let file = dir.join("prv.dat");
        std::fs::write(&file, "1\t0\t1\n\n2\t0\t1\n").unwrap();

        assert_eq!(parse_table_file(0, "TEST", "prv", &file).0.len(), 2);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_file_yields_no_rows_not_an_error() {
        let dir = temp_dir("missing");
        let file = dir.join("prv.dat");
        assert!(parse_table_file(0, "TEST", "prv", &file).0.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_latin1_file_is_read_with_a_problem_saying_so() {
        // A whole table lost to one degree sign read the same as "this
        // root has no rows in this table".
        let dir = temp_dir("bad-utf8");
        let file = dir.join("prv.dat");
        std::fs::write(&file, b"1\t0\t100\n2\t-10\t10 \xb0C\n").unwrap();
        let (rows, problems) = parse_table_file(0, "TEST", "prv", &file);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].get("PRV_MAXVAL"), Some("10 \u{b0}C"));
        assert_eq!(problems.len(), 1);
        assert!(problems[0].message.contains("Latin-1") && problems[0].line == Some(2), "{problems:?}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_unknown_table_yields_no_rows() {
        let dir = temp_dir("unknown-table");
        let file = dir.join("nope.dat");
        std::fs::write(&file, "a\tb\tc\n").unwrap();
        assert!(parse_table_file(0, "TEST", "nope", &file).0.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }
}
