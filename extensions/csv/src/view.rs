//! What's shown of a table: its rows filtered by a text or by
//! conditions on columns, sorted by a column -- in the view only, the
//! file as it was -- and the focused column's stats.

use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::HashSet;
use std::hash::{BuildHasher, BuildHasherDefault, DefaultHasher};

use crate::dialect::number;
use crate::table::Table;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    /// `~`: holds the text.
    Has,
    /// `!~`: doesn't.
    Lacks,
}

/// The operators, longest first so `>=` isn't read as `>`.
const OPS: [(&str, Op); 9] = [("!~", Op::Lacks), (">=", Op::Ge), ("<=", Op::Le), ("!=", Op::Ne), ("==", Op::Eq), ("~", Op::Has), (">", Op::Gt), ("<", Op::Lt), ("=", Op::Eq)];

#[derive(Debug, Clone, PartialEq)]
pub struct Cond {
    pub col: usize,
    pub op: Op,
    /// What it's compared with, lowercased.
    pub value: String,
    pub num: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Filter {
    /// Rows with a cell that holds it, any case.
    Text(String),
    /// Rows each of these holds for: `price > 3 & name ~ ada`.
    Conds(Vec<Cond>),
}

/// The column `name` means: a title (any case), or its number from 1
/// (`3`, `#3`).
pub fn column(name: &str, titles: &[String]) -> Option<usize> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    if let Some(c) = titles.iter().position(|t| t.trim().to_lowercase() == name.to_lowercase()) {
        return Some(c);
    }
    let n: usize = name.strip_prefix('#').unwrap_or(name).parse().ok()?;
    (1..=titles.len()).contains(&n).then(|| n - 1)
}

fn cond(part: &str, titles: &[String]) -> Option<Cond> {
    let (at, token, op) = OPS.iter().filter_map(|(t, op)| part.find(t).map(|i| (i, *t, *op))).min_by_key(|(i, t, _)| (*i, std::cmp::Reverse(t.len())))?;
    let col = column(&part[..at], titles)?;
    let value = part[at + token.len()..].trim();
    let value = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')).unwrap_or(value);
    Some(Cond { col, op, value: value.to_lowercase(), num: number(value) })
}

/// What `/` typed means: conditions when every part (split by `&`) is
/// one on a column it names, else a text to find.
pub fn parse_filter(input: &str, titles: &[String]) -> Option<Filter> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    let conds: Option<Vec<Cond>> = input.split('&').filter(|p| !p.trim().is_empty()).map(|p| cond(p, titles)).collect();
    Some(match conds {
        Some(c) if !c.is_empty() => Filter::Conds(c),
        _ => Filter::Text(input.to_lowercase()),
    })
}

/// Whether `hay` holds `needle` (lowercased), any case.
fn holds(hay: &str, needle: &str) -> bool {
    if needle.is_ascii() {
        let (h, n) = (hay.as_bytes(), needle.as_bytes());
        n.is_empty() || h.windows(n.len()).any(|w| w.eq_ignore_ascii_case(n))
    } else {
        hay.to_lowercase().contains(needle)
    }
}

/// A text's first sixteen bytes, without case, as a number: ordered as
/// [`text_cmp`] orders them, so most comparisons stop there.
fn prefix(s: &str) -> (u64, u64) {
    let mut k = [0u8; 16];
    for (i, b) in s.bytes().take(16).enumerate() {
        k[i] = b.to_ascii_lowercase();
    }
    let (hi, lo) = k.split_at(8);
    (u64::from_be_bytes(hi.try_into().unwrap_or_default()), u64::from_be_bytes(lo.try_into().unwrap_or_default()))
}

/// Ordered without case.
fn lower_cmp(a: &str, b: &str) -> Ordering {
    let (x, y) = (a.as_bytes(), b.as_bytes());
    for i in 0..x.len().min(y.len()) {
        let (p, q) = (x[i].to_ascii_lowercase(), y[i].to_ascii_lowercase());
        if p != q {
            return p.cmp(&q);
        }
    }
    x.len().cmp(&y.len())
}

/// Ordered without case, then with it.
fn text_cmp(a: &str, b: &str) -> Ordering {
    lower_cmp(a, b).then_with(|| a.cmp(b))
}

impl Cond {
    pub fn holds(&self, cell: &str) -> bool {
        if let (Some(want), Some(have)) = (self.num, number(cell)) {
            if !matches!(self.op, Op::Has | Op::Lacks) {
                let o = have.partial_cmp(&want).unwrap_or(Ordering::Equal);
                return self.ordered(o);
            }
        }
        let cell = cell.trim();
        match self.op {
            Op::Has => holds(cell, &self.value),
            Op::Lacks => !holds(cell, &self.value),
            Op::Eq => cell.to_lowercase() == self.value,
            Op::Ne => cell.to_lowercase() != self.value,
            // A number compared with a cell that isn't one: no row.
            _ if self.num.is_some() => false,
            _ => self.ordered(text_cmp(&cell.to_lowercase(), &self.value)),
        }
    }

    fn ordered(&self, o: Ordering) -> bool {
        match self.op {
            Op::Lt => o == Ordering::Less,
            Op::Le => o != Ordering::Greater,
            Op::Gt => o == Ordering::Greater,
            Op::Ge => o != Ordering::Less,
            Op::Eq => o == Ordering::Equal,
            Op::Ne => o != Ordering::Equal,
            Op::Has | Op::Lacks => false,
        }
    }
}

impl Filter {
    pub fn keeps(&self, t: &Table, r: usize) -> bool {
        match self {
            // On the row's text: a quote's doubling aside, what a cell
            // holds is there.
            Filter::Text(s) => holds(t.raw_row(r), s),
            Filter::Conds(cs) => cs.iter().all(|c| c.holds(&t.cell(r, c.col))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sort {
    pub col: usize,
    pub desc: bool,
}

/// Whether the values are numbers: all those that aren't blank, and
/// some are.
fn numeric<S: AsRef<str>>(values: &[S]) -> bool {
    let mut any = false;
    for v in values {
        let v = v.as_ref();
        if v.trim().is_empty() {
            continue;
        }
        if number(v).is_none() {
            return false;
        }
        any = true;
    }
    any
}

/// The rows shown, in order, as data row indices: those `filter` keeps,
/// sorted by `sort` (a stable sort; blanks last either way). None when
/// there's neither: every row, as in the file.
pub fn rows(t: &Table, filter: Option<&Filter>, sort: Option<Sort>) -> Option<Vec<u32>> {
    if filter.is_none() && sort.is_none() {
        return None;
    }
    let mut order: Vec<u32> = match filter {
        Some(f) => (0..t.rows()).filter(|&r| f.keeps(t, r)).map(|r| r as u32).collect(),
        None => (0..t.rows() as u32).collect(),
    };
    let Some(sort) = sort else { return Some(order) };
    let values: Vec<Cow<'_, str>> = order.iter().map(|&r| t.cell(r as usize, sort.col)).collect();
    let dir = |o: Ordering| if sort.desc { o.reverse() } else { o };
    // The rows with a value sorted, those without after them as they
    // came; each sorted by a key kept beside it, so a million of them
    // aren't each looked up in the text again and again.
    let idx: Vec<u32> = if numeric(&values) {
        let mut keyed: Vec<(f64, u32)> = Vec::with_capacity(values.len());
        let mut blank = Vec::new();
        for (i, v) in values.iter().enumerate() {
            match number(v) {
                Some(n) => keyed.push((n, i as u32)),
                None => blank.push(i as u32),
            }
        }
        drop(values);
        keyed.sort_unstable_by(|a, b| dir(a.0.partial_cmp(&b.0).unwrap_or(Ordering::Equal)).then(a.1.cmp(&b.1)));
        keyed.into_iter().map(|k| k.1).chain(blank).collect()
    } else {
        let mut keyed: Vec<((u64, u64), u32, u32)> = Vec::with_capacity(values.len());
        let mut blank = Vec::new();
        for (i, v) in values.iter().enumerate() {
            let k = v.trim();
            if k.is_empty() {
                blank.push(i as u32);
            } else {
                keyed.push((prefix(k), k.len().min(u32::MAX as usize) as u32, i as u32));
            }
        }
        let text = |i: u32| values[i as usize].trim();
        // Without case; the same either way, as the rows came (an
        // unstable sort, but ties go by where the rows were). Texts that
        // fit their keys are never looked up.
        keyed.sort_unstable_by(|a, b| {
            let by = a.0.cmp(&b.0).then_with(|| if a.1 <= 16 && b.1 <= 16 { a.1.cmp(&b.1) } else { lower_cmp(text(a.2), text(b.2)) });
            dir(by).then(a.2.cmp(&b.2))
        });
        keyed.into_iter().map(|k| k.2).chain(blank).collect()
    };
    order = idx.into_iter().map(|i| order[i as usize]).collect();
    Some(order)
}

/// A column's values, summed up.
#[derive(Debug, Clone, PartialEq)]
pub struct Stats {
    /// Cells that aren't blank.
    pub count: usize,
    pub blank: usize,
    pub distinct: usize,
    pub numeric: bool,
    pub min: Option<String>,
    pub max: Option<String>,
    pub mean: Option<f64>,
}

/// Column `col`'s stats over `rows` (data row indices).
pub fn stats(t: &Table, rows: impl Iterator<Item = usize>, col: usize) -> Stats {
    let hasher = BuildHasherDefault::<DefaultHasher>::default();
    let mut seen = HashSet::new();
    let (mut count, mut blank) = (0, 0);
    let mut numeric = true;
    let mut sum = 0.0;
    let mut nmin: Option<(f64, String)> = None;
    let mut nmax: Option<(f64, String)> = None;
    let (mut tmin, mut tmax): (Option<String>, Option<String>) = (None, None);
    for r in rows {
        let v = t.cell(r, col);
        let v = v.trim();
        if v.is_empty() {
            blank += 1;
            continue;
        }
        count += 1;
        seen.insert(hasher.hash_one(v));
        if tmin.as_deref().is_none_or(|m| text_cmp(v, m) == Ordering::Less) {
            tmin = Some(v.to_string());
        }
        if tmax.as_deref().is_none_or(|m| text_cmp(v, m) == Ordering::Greater) {
            tmax = Some(v.to_string());
        }
        if numeric {
            match number(v) {
                Some(n) => {
                    sum += n;
                    if nmin.as_ref().is_none_or(|m| n < m.0) {
                        nmin = Some((n, v.to_string()));
                    }
                    if nmax.as_ref().is_none_or(|m| n > m.0) {
                        nmax = Some((n, v.to_string()));
                    }
                }
                None => numeric = false,
            }
        }
    }
    let numeric = numeric && count > 0;
    Stats {
        count,
        blank,
        distinct: seen.len(),
        numeric,
        min: if numeric { nmin.map(|m| m.1) } else { tmin },
        max: if numeric { nmax.map(|m| m.1) } else { tmax },
        mean: numeric.then(|| sum / count as f64),
    }
}

/// A mean as the stats line shows it: to six significant places, no
/// trailing zeros.
pub fn show_number(n: f64) -> String {
    if n == n.trunc() && n.abs() < 1e15 {
        return format!("{}", n as i64);
    }
    let digits = (6 - n.abs().log10().floor() as i32 - 1).clamp(0, 12) as usize;
    let s = format!("{n:.digits$}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialect::Dialect;

    fn people() -> Table {
        Table::new("name,age,city\nada,36,London\ngrace,85,Arlington\nalan,41,london\nkatherine,,Hampton\nBob,7,\n".into(), Dialect::default())
    }

    fn names(t: &Table, rows: &[u32]) -> Vec<String> {
        rows.iter().map(|&r| t.cell(r as usize, 0).into_owned()).collect()
    }

    #[test]
    fn a_filter_is_conditions_or_a_text() {
        let t = people();
        let titles = t.titles();
        assert_eq!(parse_filter("age > 40", &titles), Some(Filter::Conds(vec![Cond { col: 1, op: Op::Gt, value: "40".into(), num: Some(40.0) }])));
        assert_eq!(parse_filter("#3 ~ lon", &titles), Some(Filter::Conds(vec![Cond { col: 2, op: Op::Has, value: "lon".into(), num: None }])));
        assert_eq!(parse_filter("AGE>=7 & city != london", &titles).map(|f| matches!(f, Filter::Conds(c) if c.len() == 2 && c[0].op == Op::Ge && c[1].op == Op::Ne)), Some(true));
        assert_eq!(parse_filter("London", &titles), Some(Filter::Text("london".into())));
        assert_eq!(parse_filter("height > 3", &titles), Some(Filter::Text("height > 3".into())), "no such column: a text");
        assert_eq!(parse_filter("  ", &titles), None);
    }

    #[test]
    fn rows_are_filtered() {
        let t = people();
        let titles = t.titles();
        let shown = |q: &str| names(&t, &rows(&t, parse_filter(q, &titles).as_ref(), None).unwrap());
        assert_eq!(shown("age > 40"), ["grace", "alan"]);
        assert_eq!(shown("age <= 36"), ["ada", "Bob"]);
        assert_eq!(shown("city = london"), ["ada", "alan"], "any case");
        assert_eq!(shown("city ~ ON"), ["ada", "grace", "alan", "katherine"]);
        assert_eq!(shown("city !~ on"), ["Bob"]);
        assert_eq!(shown("age != 36 & city ~ l"), ["grace", "alan"]);
        assert_eq!(shown("ham"), ["katherine"]);
        assert_eq!(shown("name > b"), ["grace", "katherine", "Bob"], "text in order without case");
        assert!(rows(&t, None, None).is_none());
    }

    #[test]
    fn rows_are_sorted_numbers_as_numbers_blanks_last() {
        let t = people();
        let sorted = |col, desc| names(&t, &rows(&t, None, Some(Sort { col, desc })).unwrap());
        assert_eq!(sorted(1, false), ["Bob", "ada", "alan", "grace", "katherine"]);
        assert_eq!(sorted(1, true), ["grace", "alan", "ada", "Bob", "katherine"]);
        assert_eq!(sorted(0, false), ["ada", "alan", "Bob", "grace", "katherine"]);
        assert_eq!(sorted(2, false), ["grace", "katherine", "ada", "alan", "Bob"], "stable: London before london as they come");
        let filter = parse_filter("city ~ lon", &t.titles());
        assert_eq!(names(&t, &rows(&t, filter.as_ref(), Some(Sort { col: 1, desc: true })).unwrap()), ["alan", "ada"]);
    }

    #[test]
    fn a_columns_stats() {
        let t = people();
        let s = stats(&t, 0..t.rows(), 1);
        assert_eq!(s, Stats { count: 4, blank: 1, distinct: 4, numeric: true, min: Some("7".into()), max: Some("85".into()), mean: Some(42.25) });
        let s = stats(&t, 0..t.rows(), 2);
        assert_eq!((s.count, s.distinct, s.numeric, s.min.as_deref(), s.max.as_deref(), s.mean), (4, 4, false, Some("Arlington"), Some("london"), None));
        let s = stats(&t, [0, 2].into_iter(), 2);
        assert_eq!(s.distinct, 2, "London and london are two");
        assert_eq!(show_number(42.25), "42.25");
        assert_eq!(show_number(1.0 / 3.0), "0.333333");
        assert_eq!(show_number(12345.678), "12345.7");
        assert_eq!(show_number(-3.0), "-3");
    }
}
