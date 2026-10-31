//! Cells for the clipboard: as the file writes them, or as a Markdown
//! table.

use crate::dialect::Dialect;
use crate::table::encode;

/// `rows` as CSV in the file's dialect, a line each. A file without
/// quotes is copied with `"` where a cell needs them.
pub fn as_csv(d: &Dialect, rows: &[Vec<String>]) -> String {
    let d = Dialect { quote: d.quote.or(Some(b'"')), ..*d };
    let sep = (d.delimiter as char).to_string();
    let mut out = String::new();
    for row in rows {
        let cells: Vec<String> = row.iter().map(|c| encode(&d, c).unwrap_or_else(|_| c.clone())).collect();
        out.push_str(&cells.join(&sep));
        out.push('\n');
    }
    out
}

/// A cell as Markdown table text: `|` escaped, line breaks as `<br>`.
fn md_cell(s: &str) -> String {
    s.replace('\\', "\\\\").replace('|', "\\|").replace("\r\n", "<br>").replace(['\n', '\r'], "<br>")
}

/// `rows` under `titles` as a Markdown table, numbers lined up on the
/// right.
pub fn as_markdown(titles: &[String], rows: &[Vec<String>]) -> String {
    let n = titles.len().max(rows.iter().map(Vec::len).max().unwrap_or(0));
    let cell = |r: &[String], c: usize| md_cell(r.get(c).map(String::as_str).unwrap_or(""));
    let right: Vec<bool> = (0..n)
        .map(|c| {
            let vals: Vec<&str> = rows.iter().filter_map(|r| r.get(c)).map(|s| s.as_str()).filter(|s| !s.trim().is_empty()).collect();
            !vals.is_empty() && vals.iter().all(|v| crate::dialect::number(v).is_some())
        })
        .collect();
    let widths: Vec<usize> = (0..n).map(|c| std::iter::once(cell(titles, c)).chain(rows.iter().map(|r| cell(r, c))).map(|s| fenix_ui_kit::text::width(&s)).max().unwrap_or(0).max(3)).collect();
    let line = |cells: Vec<String>| {
        let padded: Vec<String> = cells
            .iter()
            .enumerate()
            .map(|(c, s)| {
                let pad = " ".repeat(widths[c] - fenix_ui_kit::text::width(s));
                if right[c] {
                    format!("{pad}{s}")
                } else {
                    format!("{s}{pad}")
                }
            })
            .collect();
        format!("| {} |\n", padded.join(" | "))
    };
    let mut out = line((0..n).map(|c| cell(titles, c)).collect());
    let rule: Vec<String> = (0..n).map(|c| if right[c] { format!("{}:", "-".repeat(widths[c] - 1)) } else { "-".repeat(widths[c]) }).collect();
    out.push_str(&format!("| {} |\n", rule.join(" | ")));
    for r in rows {
        out.push_str(&line((0..n).map(|c| cell(r, c)).collect()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows() -> Vec<Vec<String>> {
        vec![vec!["ada".into(), "36".into()], vec!["a|b, c".into(), "7".into()]]
    }

    #[test]
    fn as_csv_in_the_files_dialect() {
        assert_eq!(as_csv(&Dialect::default(), &rows()), "ada,36\n\"a|b, c\",7\n");
        let tsv = Dialect { delimiter: b'\t', quote: None, header: true };
        assert_eq!(as_csv(&tsv, &rows()), "ada\t36\na|b, c\t7\n");
        assert_eq!(as_csv(&tsv, &[vec!["x\ty".into()]]), "\"x\ty\"\n", "quoted on the clipboard though the file has none");
    }

    #[test]
    fn as_a_markdown_table() {
        let titles = vec!["name".to_string(), "age".to_string()];
        assert_eq!(as_markdown(&titles, &rows()), "| name    | age |\n| ------- | --: |\n| ada     |  36 |\n| a\\|b, c |   7 |\n");
        assert_eq!(as_markdown(&titles, &[vec!["two\nlines".into()]]), "| name         | age |\n| ------------ | --- |\n| two<br>lines |     |\n");
    }
}
