//! A CSV file's text, and where its records are in it: found once, as
//! the csv crate reads them, so a row's cells are read only when it's
//! shown, sorted on or filtered.
//!
//! Changes are [`Splice`]s of the text -- what the file's buffer is
//! given -- and the records they touch are found again, the others
//! moved along.

use std::borrow::Cow;
use std::cell::RefCell;
use std::ops::Range;

use crate::dialect::Dialect;

/// Bytes `start..end` of the text replaced with `text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Splice {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

impl Splice {
    pub fn new(range: Range<usize>, text: impl Into<String>) -> Splice {
        Splice { start: range.start, end: range.end, text: text.into() }
    }

    pub fn insert(at: usize, text: impl Into<String>) -> Splice {
        Splice::new(at..at, text)
    }
}

fn reader(d: &Dialect) -> csv_core::Reader {
    let mut b = csv_core::ReaderBuilder::new();
    b.delimiter(d.delimiter);
    match d.quote {
        Some(q) => b.quote(q),
        None => b.quoting(false),
    };
    b.build()
}

/// `bytes`' records: each one's content (from its first byte to before
/// its line break, blank lines left out) and how many fields it has.
pub fn parse_records(bytes: &[u8], d: &Dialect) -> Vec<((usize, usize), usize)> {
    let mut out = Vec::new();
    parse_into(bytes, 0, bytes.len(), d, &mut out);
    out
}

/// The records of `bytes[from..to]`, `from` the start of one.
fn parse_into(bytes: &[u8], from: usize, to: usize, d: &Dialect, out: &mut Vec<((usize, usize), usize)>) {
    let mut rdr = reader(d);
    let mut buf = vec![0u8; 16 * 1024];
    let mut ends = vec![0usize; 64];
    let (mut pos, mut start, mut nbuf, mut nends) = (from, from, 0, 0);
    loop {
        let input = &bytes[pos..to];
        let (res, nin, nout, nend) = rdr.read_record(input, &mut buf[nbuf..], &mut ends[nends..]);
        pos += nin;
        nbuf += nout;
        nends += nend;
        use csv_core::ReadRecordResult as R;
        match res {
            R::InputEmpty => {
                if input.is_empty() {
                    // Fed nothing at the end already: done.
                    break;
                }
            }
            R::OutputFull => buf.resize(buf.len() * 2, 0),
            R::OutputEndsFull => ends.resize(ends.len() * 2, 0),
            R::Record => {
                let (mut s, mut e) = (start, pos);
                while s < e && matches!(bytes[s], b'\r' | b'\n') {
                    s += 1;
                }
                while e > s && matches!(bytes[e - 1], b'\r' | b'\n') {
                    e -= 1;
                }
                if s < e {
                    out.push(((s, e), nends));
                }
                start = pos;
                nbuf = 0;
                nends = 0;
            }
            R::End => break,
        }
    }
}

/// A record's fields: each one's bytes in `rec` (quotes and all) and its
/// value.
pub fn split_record<'a>(rec: &'a str, d: &Dialect) -> Vec<(Range<usize>, Cow<'a, str>)> {
    split_with(&mut reader(d), rec, d, usize::MAX)
}

/// The first `limit` of them, read with `rdr` (made for `d`, which
/// takes a while: a table keeps one).
fn split_with<'a>(rdr: &mut csv_core::Reader, rec: &'a str, d: &Dialect, limit: usize) -> Vec<(Range<usize>, Cow<'a, str>)> {
    let bytes = rec.as_bytes();
    // Nothing quoted: its fields are what's between the delimiters.
    if d.quote.is_none_or(|q| !bytes.contains(&q)) {
        let mut out = Vec::new();
        let mut start = 0;
        loop {
            let end = bytes[start..].iter().position(|&b| b == d.delimiter).map_or(bytes.len(), |i| start + i);
            out.push((start..end, Cow::Borrowed(&rec[start..end])));
            if end == bytes.len() || out.len() >= limit {
                return out;
            }
            start = end + 1;
        }
    }
    rdr.reset();
    let mut buf = vec![0u8; bytes.len() + 1];
    let mut out = Vec::new();
    let (mut pos, mut start, mut nbuf) = (0, 0, 0);
    while out.len() < limit {
        let input = &bytes[pos..];
        let (res, nin, nout) = rdr.read_field(input, &mut buf[nbuf..]);
        pos += nin;
        nbuf += nout;
        use csv_core::ReadFieldResult as F;
        match res {
            F::InputEmpty if input.is_empty() => break,
            F::InputEmpty => {}
            F::OutputFull => buf.resize(buf.len() * 2, 0),
            F::Field { record_end } => {
                // Its bytes, up to the delimiter it ended on; a record has
                // no line break here, so it ends only where the bytes do.
                let end = if record_end { pos } else { pos - 1 };
                let raw = &rec[start..end];
                let value = if raw.as_bytes() == &buf[..nbuf] { Cow::Borrowed(raw) } else { Cow::Owned(String::from_utf8_lossy(&buf[..nbuf]).into_owned()) };
                out.push((start..end, value));
                start = pos;
                nbuf = 0;
                if record_end {
                    break;
                }
            }
            F::End => break,
        }
    }
    out
}

/// A field's value written as the file writes them: quoted when it must
/// be. Err when it can't be (a delimiter in a file without quotes).
pub fn encode(d: &Dialect, value: &str) -> Result<String, String> {
    let Some(q) = d.quote else {
        if value.bytes().any(|b| b == d.delimiter || b == b'\n' || b == b'\r') {
            return Err(format!(
                "this file has no quotes, so a cell can't hold {}",
                if value.contains(['\n', '\r']) { "a line break".to_string() } else { format!("its delimiter ({})", d.delimiter_name()) }
            ));
        }
        return Ok(value.to_string());
    };
    if value.is_empty() {
        return Ok(String::new());
    }
    let mut w = csv::WriterBuilder::new().delimiter(d.delimiter).quote(q).quote_style(csv::QuoteStyle::Necessary).terminator(csv::Terminator::Any(b'\n')).from_writer(Vec::new());
    w.write_record([value]).map_err(|e| e.to_string())?;
    let bytes = w.into_inner().map_err(|e| e.to_string())?;
    let mut s = String::from_utf8(bytes).map_err(|e| e.to_string())?;
    s.pop();
    // A value that starts with a space or the quote stays as typed; one
    // the csv crate leaves bare but a reader would take as quoted is
    // quoted.
    if !s.starts_with(q as char) && value.starts_with(q as char) {
        s = format!("{0}{1}{0}", q as char, value.replace(q as char, &format!("{0}{0}", q as char)));
    }
    Ok(s)
}

/// A file's text, its dialect, and where its records are.
#[derive(Debug, Clone)]
pub struct Table {
    text: String,
    pub dialect: Dialect,
    /// Each record's content, a blank line not one.
    records: Vec<(u32, u32)>,
    /// The most fields a record has.
    width: usize,
    /// A reader for the dialect, made once.
    reader: RefCell<csv_core::Reader>,
}

impl Table {
    pub fn new(text: String, dialect: Dialect) -> Table {
        let mut t = Table { text, dialect, records: Vec::new(), width: 0, reader: RefCell::new(reader(&dialect)) };
        t.reindex();
        t
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Read again with another dialect.
    pub fn set_dialect(&mut self, dialect: Dialect) {
        let delims = (dialect.delimiter, dialect.quote) != (self.dialect.delimiter, self.dialect.quote);
        self.dialect = dialect;
        if delims {
            self.reader = RefCell::new(reader(&dialect));
            self.reindex();
        }
    }

    fn reindex(&mut self) {
        let mut recs = Vec::new();
        let from = if self.text.starts_with('\u{feff}') { 3 } else { 0 };
        parse_into(self.text.as_bytes(), from, self.text.len(), &self.dialect, &mut recs);
        self.width = recs.iter().map(|r| r.1).max().unwrap_or(0);
        self.records = recs.into_iter().map(|((s, e), _)| (s as u32, e as u32)).collect();
    }

    /// Record bytes `rec`'s first `limit` fields.
    fn split<'a>(&self, rec: &'a str, limit: usize) -> Vec<(Range<usize>, Cow<'a, str>)> {
        split_with(&mut self.reader.borrow_mut(), rec, &self.dialect, limit)
    }

    fn head(&self) -> usize {
        usize::from(self.dialect.header && !self.records.is_empty())
    }

    /// Rows of data: the header isn't one.
    pub fn rows(&self) -> usize {
        self.records.len() - self.head()
    }

    pub fn cols(&self) -> usize {
        self.width
    }

    /// The record row `r` is.
    fn record(&self, r: usize) -> Range<usize> {
        let (s, e) = self.records[r + self.head()];
        s as usize..e as usize
    }

    /// Row `r`'s bytes, as they are in the file.
    pub fn raw_row(&self, r: usize) -> &str {
        &self.text[self.record(r)]
    }

    /// The line row `r` starts on, from 1, for the detail line.
    pub fn line_of(&self, r: usize) -> usize {
        crate::edit::positions(&self.text, &[self.record(r).start])[0].0 as usize + 1
    }

    /// The columns' titles: the header's names, or their numbers.
    pub fn titles(&self) -> Vec<String> {
        let mut names: Vec<String> = if self.head() == 1 {
            let (s, e) = self.records[0];
            self.split(&self.text[s as usize..e as usize], usize::MAX).into_iter().map(|f| f.1.into_owned()).collect()
        } else {
            Vec::new()
        };
        let n = self.width.max(names.len());
        for c in names.len()..n {
            names.push((c + 1).to_string());
        }
        names
    }

    pub fn row(&self, r: usize) -> Vec<Cow<'_, str>> {
        self.split(self.raw_row(r), usize::MAX).into_iter().map(|f| f.1).collect()
    }

    /// Cell (`r`, `c`); blank past the row's end.
    pub fn cell(&self, r: usize, c: usize) -> Cow<'_, str> {
        self.split(self.raw_row(r), c + 1).into_iter().nth(c).map(|f| f.1).unwrap_or(Cow::Borrowed(""))
    }

    /// Cell (`r`, `c`)'s bytes in the text, and its value: none past the
    /// row's end.
    pub fn cell_span(&self, r: usize, c: usize) -> Option<(Range<usize>, Cow<'_, str>)> {
        let base = self.record(r).start;
        self.split(self.raw_row(r), c + 1).into_iter().nth(c).map(|(raw, v)| (raw.start + base..raw.end + base, v))
    }

    /// The fields of a record, by its index among all records.
    fn fields_of(&self, rec: usize) -> Vec<(Range<usize>, Cow<'_, str>)> {
        let (s, e) = self.records[rec];
        let base = s as usize;
        self.split(&self.text[s as usize..e as usize], usize::MAX).into_iter().map(|(raw, v)| (raw.start + base..raw.end + base, v)).collect()
    }

    /// The line break the file uses.
    pub fn newline(&self) -> &'static str {
        let end = self.records.first().map_or(0, |r| r.1 as usize);
        let rest = &self.text.as_bytes()[end..];
        if rest.starts_with(b"\r\n") {
            "\r\n"
        } else if rest.starts_with(b"\r") {
            "\r"
        } else {
            "\n"
        }
    }

    /// An empty row of `n` cells, as the file writes one: a lone empty
    /// cell quoted, or it's a blank line, which isn't a row.
    fn empty_row(&self, n: usize) -> String {
        if n <= 1 {
            return self.dialect.quote.map(|q| format!("{0}{0}", q as char)).unwrap_or_default();
        }
        (self.dialect.delimiter as char).to_string().repeat(n - 1)
    }

    // What a change is, as splices of the text.

    /// Cell (`r`, `c`) set to `value`; a row too short for it grows.
    pub fn set_cell(&self, r: usize, c: usize, value: &str) -> Result<Vec<Splice>, String> {
        let mut enc = encode(&self.dialect, value)?;
        let rec = self.record(r);
        let fields = self.fields_of(r + self.head());
        if enc.is_empty() && fields.len() <= 1 && c == 0 {
            enc = self.empty_row(1);
        }
        Ok(vec![match fields.get(c) {
            Some((raw, _)) => Splice::new(raw.clone(), enc),
            None => {
                let pad = (self.dialect.delimiter as char).to_string().repeat(c + 1 - fields.len());
                Splice::insert(rec.end, pad + &enc)
            }
        }])
    }

    /// A header's name set, as a cell is.
    pub fn set_title(&self, c: usize, value: &str) -> Result<Vec<Splice>, String> {
        if self.head() == 0 {
            return Err("this file has no header (H takes its first row as one)".into());
        }
        let enc = encode(&self.dialect, value)?;
        let fields = self.fields_of(0);
        Ok(vec![match fields.get(c) {
            Some((raw, _)) => Splice::new(raw.clone(), enc),
            None => {
                let pad = (self.dialect.delimiter as char).to_string().repeat(c + 1 - fields.len());
                Splice::insert(self.records[0].1 as usize, pad + &enc)
            }
        }])
    }

    /// An empty row put in as row `at` (`rows()` for after the last).
    pub fn insert_row(&self, at: usize) -> Vec<Splice> {
        let row = self.empty_row(self.width.max(1));
        let nl = self.newline();
        if at < self.rows() {
            return vec![Splice::insert(self.record(at).start, row + nl)];
        }
        // After the last record: on a line of its own.
        let end = self.text.len();
        let after = self.records.last().map_or(0, |r| r.1 as usize);
        let ends_in_break = self.text[after..].contains(['\n', '\r']);
        if self.records.is_empty() || ends_in_break {
            vec![Splice::insert(end, row + nl)]
        } else {
            vec![Splice::insert(end, nl.to_string() + &row)]
        }
    }

    /// Row `r` taken out, with its line break.
    pub fn delete_row(&self, r: usize) -> Vec<Splice> {
        let i = r + self.head();
        let (s, _) = self.records[i];
        match self.records.get(i + 1) {
            Some(&(next, _)) => vec![Splice::new(s as usize..next as usize, "")],
            // The last: from the end of the one before, so no blank line
            // is left; the file's last line break stays.
            None => {
                let end = self.records[i].1 as usize;
                let tail = &self.text[end..];
                let from = if i > 0 { self.records[i - 1].1 as usize } else { s as usize };
                let keep = if i > 0 && tail.contains(['\n', '\r']) { self.newline() } else { "" };
                vec![Splice::new(from..self.text.len(), keep)]
            }
        }
    }

    /// An empty column put in as column `at` (`cols()` for after the
    /// last), named `name` in the header.
    pub fn insert_column(&self, at: usize, name: &str) -> Result<Vec<Splice>, String> {
        let name = encode(&self.dialect, name)?;
        let d = self.dialect.delimiter as char;
        let mut out = Vec::new();
        for rec in 0..self.records.len() {
            let fields = self.fields_of(rec);
            let value = if rec == 0 && self.head() == 1 { name.as_str() } else { "" };
            match fields.get(at) {
                Some((raw, _)) => out.push(Splice::insert(raw.start, format!("{value}{d}"))),
                None => {
                    let end = self.records[rec].1 as usize;
                    let pad = d.to_string().repeat(at + 1 - fields.len());
                    out.push(Splice::insert(end, format!("{pad}{value}")));
                }
            }
        }
        Ok(out)
    }

    /// Column `c` taken out of every row that has it.
    pub fn delete_column(&self, c: usize) -> Vec<Splice> {
        let mut out = Vec::new();
        for rec in 0..self.records.len() {
            let fields = self.fields_of(rec);
            let Some((raw, _)) = fields.get(c) else { continue };
            if fields.len() == 1 {
                // The record's only field: it's left empty, quoted.
                out.push(Splice::new(raw.clone(), self.empty_row(1)));
            } else if c + 1 < fields.len() {
                out.push(Splice::new(raw.start..fields[c + 1].0.start, ""));
            } else {
                out.push(Splice::new(fields[c - 1].0.end..raw.end, ""));
            }
        }
        out
    }

    /// The rows written in `order` (data rows, every one once), the
    /// header staying first: a sort saved.
    pub fn reorder(&self, order: &[u32]) -> Vec<Splice> {
        let h = self.head();
        if self.rows() < 2 {
            return Vec::new();
        }
        let first = self.records[h].0 as usize;
        let last = self.records[self.records.len() - 1].1 as usize;
        let nl = self.newline();
        let mut body = String::with_capacity(last - first);
        for (i, &r) in order.iter().enumerate() {
            if i > 0 {
                body.push_str(nl);
            }
            body.push_str(self.raw_row(r as usize));
        }
        vec![Splice::new(first..last, body)]
    }

    /// `splices` (in order, apart) made to the text, the records found
    /// again; what undoes them comes back.
    pub fn apply(&mut self, splices: &[Splice]) -> Vec<Splice> {
        if splices.is_empty() {
            return Vec::new();
        }
        let mut inverse = Vec::with_capacity(splices.len());
        let mut shift: isize = 0;
        for s in splices {
            let start = (s.start as isize + shift) as usize;
            inverse.push(Splice::new(start..start + s.text.len(), &self.text[s.start..s.end]));
            shift += s.text.len() as isize - (s.end - s.start) as isize;
        }
        if splices.len() == 1 {
            let s = &splices[0];
            let delta = s.text.len() as isize - (s.end - s.start) as isize;
            self.text.replace_range(s.start..s.end, &s.text);
            self.reindex_around(s.start, s.end, delta);
        } else {
            let mut text = String::with_capacity((self.text.len() as isize + shift).max(0) as usize);
            let mut at = 0;
            for s in splices {
                text.push_str(&self.text[at..s.start]);
                text.push_str(&s.text);
                at = s.end;
            }
            text.push_str(&self.text[at..]);
            self.text = text;
            self.reindex();
        }
        inverse
    }

    /// The records about bytes `start..end`, which became `delta` longer,
    /// found again; those after it moved along.
    fn reindex_around(&mut self, start: usize, end: usize, delta: isize) {
        let a = self.records.partition_point(|r| (r.1 as usize) < start);
        let b = self.records.partition_point(|r| (r.0 as usize) <= end).max(a);
        let from = self.records.get(a).map_or(start, |r| (r.0 as usize).min(start));
        let from = if a > 0 { from.max(self.records[a - 1].1 as usize) } else { from };
        let to = match self.records.get(b) {
            Some(r) => (r.0 as isize + delta) as usize,
            None => self.text.len(),
        };
        let mut found = Vec::new();
        parse_into(self.text.as_bytes(), from, to, &self.dialect, &mut found);
        let width = found.iter().map(|r| r.1).max().unwrap_or(0);
        let shifted: Vec<(u32, u32)> = found.into_iter().map(|((s, e), _)| (s as u32, e as u32)).collect();
        for r in &mut self.records[b..] {
            r.0 = (r.0 as isize + delta) as u32;
            r.1 = (r.1 as isize + delta) as u32;
        }
        self.records.splice(a..b, shifted);
        self.width = self.width.max(width);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(text: &str) -> Table {
        Table::new(text.into(), Dialect::default())
    }

    fn cells(t: &Table) -> Vec<Vec<String>> {
        (0..t.rows()).map(|r| t.row(r).into_iter().map(|c| c.into_owned()).collect()).collect()
    }

    #[test]
    fn records_and_fields_are_read_as_the_csv_crate_reads_them() {
        let t = table("id,name,note\r\n1,ada,\"says \"\"hi\"\"\"\r\n\r\n2,grace,\"two\nlines\"\r\n3,,x\r\n");
        assert_eq!(t.titles(), ["id", "name", "note"]);
        assert_eq!(t.rows(), 3, "the blank line isn't a row");
        assert_eq!(cells(&t), [vec!["1", "ada", "says \"hi\""], vec!["2", "grace", "two\nlines"], vec!["3", "", "x"]]);
        assert_eq!(t.cell(1, 2), "two\nlines");
        assert_eq!(t.cell(1, 7), "", "past the row's end");
        assert_eq!(t.newline(), "\r\n");
        let (span, v) = t.cell_span(0, 2).unwrap();
        assert_eq!(&t.text()[span], "\"says \"\"hi\"\"\"");
        assert_eq!(v, "says \"hi\"");
        assert_eq!(t.line_of(2), 6);
    }

    #[test]
    fn ragged_rows_unquoted_files_and_a_bom() {
        let t = Table::new("\u{feff}a\tb\n1\n2\t3\t4\n".into(), Dialect { delimiter: b'\t', quote: None, header: true });
        assert_eq!(t.cols(), 3);
        assert_eq!(t.titles(), ["a", "b", "3"], "a column the header doesn't name gets its number");
        assert_eq!(cells(&t), [vec!["1"], vec!["2", "3", "4"]]);
        let t = Table::new("x,\"y\nz\n".into(), Dialect { quote: None, header: false, ..Dialect::default() });
        assert_eq!(t.rows(), 2, "without quotes a quote is a character");
        assert_eq!(t.cell(0, 1), "\"y");
        let t = Table::new("a,b\n,\n".into(), Dialect::default());
        assert_eq!(cells(&t), [vec!["", ""]], "a row of empty cells");
    }

    #[test]
    fn cells_are_quoted_when_they_must_be() {
        let d = Dialect::default();
        assert_eq!(encode(&d, "plain").unwrap(), "plain");
        assert_eq!(encode(&d, "a,b").unwrap(), "\"a,b\"");
        assert_eq!(encode(&d, "say \"x\"").unwrap(), "\"say \"\"x\"\"\"");
        assert_eq!(encode(&d, "two\nlines").unwrap(), "\"two\nlines\"");
        assert_eq!(encode(&d, "\"lead").unwrap(), "\"\"\"lead\"");
        let semi = Dialect { delimiter: b';', ..d };
        assert_eq!(encode(&semi, "a,b").unwrap(), "a,b");
        assert_eq!(encode(&semi, "1;2").unwrap(), "\"1;2\"");
        let bare = Dialect { quote: None, ..d };
        assert!(encode(&bare, "a,b").is_err());
        assert!(encode(&bare, "a\nb").is_err());
    }

    /// Applied and undone, the text and its records each time as reading
    /// it afresh finds them.
    fn check(t: &mut Table, splices: Vec<Splice>, expect: &str) {
        let before = t.text().to_string();
        let undo = t.apply(&splices);
        assert_eq!(t.text(), expect);
        let fresh = Table::new(t.text().into(), t.dialect);
        assert_eq!(t.records, fresh.records, "records found again as afresh");
        t.apply(&undo);
        assert_eq!(t.text(), before, "undone");
        assert_eq!(t.records, Table::new(before.clone(), t.dialect).records);
        t.apply(&splices);
    }

    #[test]
    fn cells_rows_and_columns_change_as_text() {
        let mut t = table("id,name\n1,ada\n2,grace\n");
        let s = t.set_cell(0, 1, "Ada, Countess").unwrap();
        check(&mut t, s, "id,name\n1,\"Ada, Countess\"\n2,grace\n");
        let s = t.set_cell(1, 3, "x").unwrap();
        check(&mut t, s, "id,name\n1,\"Ada, Countess\"\n2,grace,,x\n");
        let s = t.set_title(1, "who").unwrap();
        check(&mut t, s, "id,who\n1,\"Ada, Countess\"\n2,grace,,x\n");
        let s = t.insert_row(1);
        check(&mut t, s, "id,who\n1,\"Ada, Countess\"\n,,,\n2,grace,,x\n");
        let s = t.delete_row(1);
        check(&mut t, s, "id,who\n1,\"Ada, Countess\"\n2,grace,,x\n");
        let s = t.insert_row(t.rows());
        check(&mut t, s, "id,who\n1,\"Ada, Countess\"\n2,grace,,x\n,,,\n");
        let s = t.delete_row(2);
        check(&mut t, s, "id,who\n1,\"Ada, Countess\"\n2,grace,,x\n");
        let s = t.delete_column(0);
        check(&mut t, s, "who\n\"Ada, Countess\"\ngrace,,x\n");
        let s = t.insert_column(0, "n").unwrap();
        check(&mut t, s, "n,who\n,\"Ada, Countess\"\n,grace,,x\n");
        let s = t.insert_column(2, "end").unwrap();
        check(&mut t, s, "n,who,end\n,\"Ada, Countess\",\n,grace,,,x\n");
        let s = t.delete_column(3);
        check(&mut t, s, "n,who,end\n,\"Ada, Countess\",\n,grace,,x\n");
    }

    #[test]
    fn the_last_row_and_a_file_without_a_last_line_break() {
        let mut t = table("a,b\n1,2\n3,4");
        let s = t.delete_row(1);
        check(&mut t, s, "a,b\n1,2");
        let s = t.insert_row(1);
        check(&mut t, s, "a,b\n1,2\n,");
        let mut t = table("a,b\r\n1,2\r\n");
        let s = t.insert_row(0);
        check(&mut t, s, "a,b\r\n,\r\n1,2\r\n");
        let s = t.delete_row(1);
        check(&mut t, s, "a,b\r\n,\r\n");
        let mut one = table("name\nx\ny\n");
        let s = one.set_cell(0, 0, "").unwrap();
        check(&mut one, s, "name\n\"\"\ny\n");
        assert_eq!(one.rows(), 2, "an empty cell alone is still a row");
        let s = one.insert_row(2);
        check(&mut one, s, "name\n\"\"\ny\n\"\"\n");
    }

    #[test]
    fn a_sort_is_written_as_the_rows_in_its_order() {
        let mut t = table("n\n3\n1\n2\n");
        let s = t.reorder(&[1, 2, 0]);
        check(&mut t, s, "n\n1\n2\n3\n");
    }

    #[test]
    fn a_change_that_moves_records_finds_the_rest_again() {
        let mut t = table("a,b\n1,\"x\ny\"\n2,3\n4,5\n");
        // A quoted line break taken out: the records after it move up.
        let s = t.set_cell(0, 1, "xy").unwrap();
        check(&mut t, s, "a,b\n1,xy\n2,3\n4,5\n");
        let s = t.set_cell(2, 0, "four\nlines\nof\ntext").unwrap();
        check(&mut t, s, "a,b\n1,xy\n2,3\n\"four\nlines\nof\ntext\",5\n");
        assert_eq!(t.cell(2, 1), "5");
    }
}
