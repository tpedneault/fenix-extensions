//! A note as the notebook page shows the selected one: `blocks` laid out
//! on a [`Sheet`] -- wrapped paragraphs with their emphasis, links and
//! tags coloured, headings with a rule, lists with their markers and
//! checkboxes, code on a panel, tables with a grid, callouts and quotes
//! with a bar, and room left for pictures and diagrams. What it can't
//! know (how big a picture is, what an embed is) comes from `Ctx`. Fenix's
//! own reading view (`SPC m p`) lays a note out the same way.

use std::ops::Range;
use std::path::PathBuf;

use fenix_extension_api::grid::{fit, Grid};
use fenix_extension_api::Role;

use crate::blocks::{self, Blk, Block, LinkTo, Marker, Span};
use crate::meta::Link;

/// What an `![[embed]]` turned out to be.
pub enum Embedded {
    Diagram { name: String, source: String },
    Note { name: String, text: String },
    Missing(String),
}

/// A picture a sheet leaves room for.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Picture {
    /// A diagram, drawn by the diagrams extension from its source.
    Diagram(String),
    /// A picture file, under the notebook or the project.
    File(PathBuf),
}

/// What the layout asks of the world.
pub struct Ctx<'a> {
    /// A picture's size in pixels, once it's known.
    pub image_size: &'a dyn Fn(&Picture) -> Option<(u32, u32)>,
    /// Why a diagram's source doesn't draw, if it doesn't.
    pub diagram_error: &'a dyn Fn(&str) -> Option<String>,
    /// Where an image's `src` is, when it's somewhere it can be read.
    pub image_path: &'a dyn Fn(&str) -> Option<PathBuf>,
    pub embed: &'a dyn Fn(&Link) -> Embedded,
    /// Whether a `[[link]]` goes somewhere that exists.
    pub link_exists: &'a dyn Fn(&Link) -> bool,
    /// One cell's size in pixels, for pictures' proportions.
    pub cell: (f32, f32),
}

impl Ctx<'_> {
    /// A plain context: no pictures known, every link found.
    #[cfg(test)]
    pub fn plain() -> Ctx<'static> {
        Ctx { image_size: &|_| None, diagram_error: &|_| None, image_path: &|s| Some(PathBuf::from(s)), embed: &|l| Embedded::Missing(l.target.clone()), link_exists: &|_| true, cell: (8.0, 17.0) }
    }
}

/// A page being laid out that can be put inside another: lines of cells,
/// coloured runs, hairlines, panels and pictures.
#[derive(Debug, Clone, Default)]
pub struct Sheet {
    lines: Vec<Vec<char>>,
    pub spans: Vec<(usize, Range<usize>, Role)>,
    pub rules: Vec<(usize, Range<usize>)>,
    pub panels: Vec<(usize, Range<usize>)>,
    /// (line, column, rows, columns, what).
    pub pictures: Vec<(usize, usize, usize, usize, Picture)>,
}

impl Sheet {
    pub fn height(&self) -> usize {
        self.lines.len()
    }

    fn grow(&mut self, line: usize) {
        if self.lines.len() <= line {
            self.lines.resize(line + 1, Vec::new());
        }
    }

    /// Writes `text` at (`line`, `col`); the column after it.
    pub fn put(&mut self, line: usize, col: usize, text: &str, role: Role) -> usize {
        self.grow(line);
        let len = text.chars().count();
        if len == 0 {
            return col;
        }
        let row = &mut self.lines[line];
        if row.len() < col + len {
            row.resize(col + len, ' ');
        }
        for (i, c) in text.chars().enumerate() {
            row[col + i] = c;
        }
        self.spans.push((line, col..col + len, role));
        col + len
    }

    pub fn rule(&mut self, line: usize, cols: Range<usize>) {
        self.grow(line);
        self.rules.push((line, cols));
    }

    pub fn panel(&mut self, line: usize, cols: Range<usize>) {
        self.grow(line);
        self.panels.push((line, cols));
    }

    pub fn picture(&mut self, line: usize, col: usize, rows: usize, cols: usize, what: Picture) {
        self.grow(line + rows.saturating_sub(1));
        self.pictures.push((line, col, rows, cols, what));
    }

    /// Its text, a line each.
    pub fn text(&self) -> String {
        self.lines.iter().map(|row| row.iter().collect::<String>().trim_end().to_string()).collect::<Vec<_>>().join("\n")
    }

    /// Line `line`'s text.
    pub fn line(&self, line: usize) -> String {
        self.lines.get(line).map(|row| row.iter().collect::<String>().trim_end().to_string()).unwrap_or_default()
    }

    /// Its first `max` lines put on `g` from (`y`, `x`): each picture as
    /// `image` has it loaded, or the room left blank until it is.
    pub fn draw(&self, g: &mut Grid, y: usize, x: usize, max: usize, image: &dyn Fn(&Picture) -> Option<u32>) {
        let mut runs: Vec<&(usize, Range<usize>, Role)> = self.spans.iter().filter(|(l, _, _)| *l < max).collect();
        runs.sort_by_key(|(l, cols, _)| (*l, cols.start));
        for (line, cols, role) in runs {
            let text: String = self.lines[*line].iter().skip(cols.start).take(cols.len()).collect();
            g.put(y + line, x + cols.start, &text, role.clone());
        }
        for (line, cols) in self.rules.iter().filter(|(l, _)| *l < max) {
            g.rule(y + line, x + cols.start..x + cols.end);
        }
        for (line, cols) in self.panels.iter().filter(|(l, _)| *l < max) {
            g.panel(y + line, x + cols.start..x + cols.end);
        }
        for (line, col, rows, cols, what) in self.pictures.iter().filter(|p| p.0 + p.2 <= max) {
            if let Some(id) = image(what) {
                g.picture(y + line, x + col, *rows, *cols, id, None);
            }
        }
    }
}

/// Lays `text` out `width` cells wide.
pub fn layout(text: &str, width: usize, ctx: &Ctx) -> Sheet {
    let blocks = blocks::parse(text);
    let mut w = Writer { g: Sheet::default(), y: 0, width: width.max(20) };
    let mut prev: Option<&Blk> = None;
    for b in &blocks {
        // A blank line between blocks, not between a list's items.
        if let Some(p) = prev {
            let both_items = matches!(p.block, Block::Item { .. }) && matches!(b.block, Block::Item { .. });
            let callout_body = matches!(p.block, Block::Callout { .. });
            if !both_items && !callout_body {
                w.blank(b.quote.min(p.quote));
            }
        }
        w.block(b, ctx);
        prev = Some(b);
    }
    w.g
}

struct Writer {
    g: Sheet,
    y: usize,
    width: usize,
}

fn syntax(name: &str) -> Role {
    Role::Syntax(name.to_string())
}

/// The colour a run of text gets.
fn role_of(span: &Span, ctx: &Ctx) -> Role {
    match &span.link {
        Some(LinkTo::Wiki(l)) if !(ctx.link_exists)(l) => Role::Warn,
        Some(_) => syntax("text.uri"),
        None if span.style.code => syntax("text.literal"),
        None if span.style.tag => syntax("text.reference"),
        None if span.style.strike => Role::Muted,
        None if span.style.bold => syntax("text.strong"),
        None if span.style.italic => syntax("text.emphasis"),
        None => Role::Text,
    }
}

impl Writer {
    /// The quote bars for depth `quote`, and the column after them.
    fn bars(&mut self, quote: usize) -> usize {
        for q in 0..quote {
            self.g.put(self.y, q * 2, "│", Role::Muted);
        }
        quote * 2
    }

    fn blank(&mut self, quote: usize) {
        self.bars(quote);
        self.g.put(self.y, 0, "", Role::Text);
        self.y += 1;
    }

    /// Writes `spans` wrapped into columns `x..width`, the first line
    /// starting at `first_x`.
    fn spans(&mut self, spans: &[Span], x: usize, first_x: usize, quote: usize, ctx: &Ctx, muted: bool) {
        let mut pieces: Vec<(String, Role)> = Vec::new();
        for s in spans {
            let role = if muted { Role::Muted } else { role_of(s, ctx) };
            if s.text == "\n" {
                pieces.push(("\n".into(), role));
                continue;
            }
            let mut word = String::new();
            for c in s.text.chars() {
                word.push(c);
                if c == ' ' {
                    pieces.push((std::mem::take(&mut word), role.clone()));
                }
            }
            if !word.is_empty() {
                pieces.push((word, role));
            }
        }
        let right = self.width;
        let mut col = first_x;
        self.bars(quote);
        for (piece, role) in pieces {
            if piece == "\n" {
                self.y += 1;
                self.bars(quote);
                col = x;
                continue;
            }
            let len = piece.trim_end().chars().count();
            if col > x && col + len > right {
                self.y += 1;
                self.bars(quote);
                col = x;
                if piece.trim().is_empty() {
                    continue;
                }
            }
            // A word longer than the line is cut to it.
            let shown: String = if len > right.saturating_sub(x) { piece.chars().take(right.saturating_sub(x).max(1)).collect() } else { piece.clone() };
            let start = col;
            self.g.put(self.y, col, shown.trim_end(), role);
            col = start + shown.chars().count();
        }
        self.y += 1;
    }

    fn block(&mut self, b: &Blk, ctx: &Ctx) {
        let x0 = b.quote * 2;
        match &b.block {
            Block::Meta { tags, about } => {
                self.bars(b.quote);
                let mut col = x0;
                for t in tags {
                    col = self.g.put(self.y, col, &format!("#{t}"), syntax("text.reference")) + 1;
                }
                if let Some(a) = about {
                    let text = if tags.is_empty() { format!("about {a}") } else { format!("· about {a}") };
                    self.g.put(self.y, col, &text, Role::Muted);
                }
                self.y += 1;
            }
            Block::Heading { level, spans } => {
                let title: String = blocks::plain(spans);
                let role = if *level <= 2 { syntax("text.title") } else { Role::Title };
                let prefix = if *level >= 3 { "#".repeat(*level - 2) + " " } else { String::new() };
                self.bars(b.quote);
                let col = if prefix.is_empty() { x0 } else { self.g.put(self.y, x0, &prefix, Role::Muted) };
                let shown = if *level == 1 { title.to_uppercase() } else { title };
                let end = self.g.put(self.y, col, &fit(&shown, self.width.saturating_sub(col)), role);
                // A hairline after the title: to the edge for a chapter,
                // a short one for a section.
                if *level <= 2 {
                    let stop = if *level == 1 { self.width } else { (end + 12).min(self.width) };
                    if end + 2 < stop {
                        self.g.rule(self.y, end + 1..stop);
                    }
                }
                self.y += 1;
            }
            Block::Paragraph { spans } => self.spans(spans, x0, x0, b.quote, ctx, false),
            Block::Item { depth, marker, spans } => {
                let indent = x0 + depth * 3;
                self.bars(b.quote);
                let (mark, role) = match marker {
                    Marker::Bullet => ("•".to_string(), Role::Muted),
                    Marker::Number(n) => (format!("{n}."), Role::Muted),
                    Marker::Task(true) => ("[x]".to_string(), Role::Good),
                    Marker::Task(false) => ("[ ]".to_string(), Role::Accent),
                };
                let end = self.g.put(self.y, indent, &mark, role);
                let text_x = end + 1;
                let done = matches!(marker, Marker::Task(true));
                self.spans(spans, text_x, text_x, b.quote, ctx, done);
            }
            Block::Code { lang, text } => self.code(lang, text, b.quote),
            Block::Mermaid { source } => self.diagram(source, None, b.quote, ctx),
            Block::Callout { kind, title } => {
                let role = match kind.as_str() {
                    "warning" | "caution" | "attention" => Role::Warn,
                    "danger" | "error" | "bug" | "failure" => Role::Bad,
                    "tip" | "success" | "check" | "done" => Role::Good,
                    _ => Role::Accent,
                };
                let bar_x = x0.saturating_sub(2);
                self.g.put(self.y, bar_x, "┃", role.clone());
                self.g.put(self.y, x0, &fit(title, self.width.saturating_sub(x0)), role);
                self.y += 1;
            }
            Block::Table { align, header, rows } => self.table(align, header, rows, b.quote, ctx),
            Block::Rule => {
                self.bars(b.quote);
                self.g.rule(self.y, x0..self.width);
                self.y += 1;
            }
            Block::Image { src, alt } => match (ctx.image_path)(src) {
                Some(path) => {
                    self.picture(Picture::File(path), x0, b.quote, ctx);
                    if !alt.is_empty() {
                        self.bars(b.quote);
                        self.g.put(self.y, x0, &fit(alt, self.width.saturating_sub(x0)), Role::Muted);
                        self.y += 1;
                    }
                }
                None => {
                    self.bars(b.quote);
                    self.g.put(self.y, x0, &fit(&format!("[image {src} isn't there]"), self.width.saturating_sub(x0)), Role::Warn);
                    self.y += 1;
                }
            },
            Block::Embed { link } => match (ctx.embed)(link) {
                Embedded::Diagram { name, source } => self.diagram(&source, Some(name), b.quote, ctx),
                Embedded::Note { name, text } => {
                    self.bars(b.quote);
                    let end = self.g.put(self.y, x0, "↳ ", Role::Muted);
                    self.g.put(self.y, end, &name, syntax("text.uri"));
                    self.y += 1;
                    // Its first lines, quoted.
                    let inner = layout(&text, self.width.saturating_sub(x0 + 2), ctx);
                    for i in 0..inner.height().min(8) {
                        self.bars(b.quote + 1);
                        let plain = inner.line(i);
                        let row: Vec<&(usize, Range<usize>, Role)> = inner.spans.iter().filter(|s| s.0 == i).collect();
                        if row.is_empty() {
                            self.g.put(self.y, x0 + 2, &plain, Role::Muted);
                        }
                        for (_, cols, role) in row {
                            let piece: String = plain.chars().skip(cols.start).take(cols.len()).collect();
                            self.g.put(self.y, x0 + 2 + cols.start, &piece, role.clone());
                        }
                        self.y += 1;
                    }
                }
                Embedded::Missing(name) => {
                    self.bars(b.quote);
                    self.g.put(self.y, x0, &fit(&format!("![[{name}]] -- nothing by that name yet"), self.width.saturating_sub(x0)), Role::Warn);
                    self.y += 1;
                }
            },
            Block::Footnote { label, spans } => {
                self.bars(b.quote);
                let end = self.g.put(self.y, x0, &format!("[{label}]"), Role::Muted) + 1;
                self.spans(spans, end, end, b.quote, ctx, true);
            }
        }
    }

    fn code(&mut self, lang: &str, text: &str, quote: usize) {
        let x0 = quote * 2;
        let lines: Vec<&str> = if text.is_empty() { vec![""] } else { text.lines().collect() };
        for (i, line) in lines.iter().enumerate() {
            self.bars(quote);
            let shown: String = line.chars().take(self.width.saturating_sub(x0 + 2)).collect();
            self.g.panel(self.y, x0..self.width);
            self.g.put(self.y, x0 + 1, &shown, Role::Text);
            if i == 0 && !lang.is_empty() && shown.chars().count() + lang.len() + 3 < self.width - x0 {
                self.g.put(self.y, self.width - lang.len(), lang, Role::Muted);
            }
            self.y += 1;
        }
    }

    /// Room for a picture: as wide as it is (in cells) up to the line,
    /// as tall as its proportions make that.
    fn picture(&mut self, what: Picture, x0: usize, quote: usize, ctx: &Ctx) {
        let avail = self.width.saturating_sub(x0).max(10);
        let (cw, ch) = ctx.cell;
        let (cols, rows) = match (ctx.image_size)(&what) {
            Some((w, h)) if w > 0 && h > 0 => {
                let natural = (w as f32 / cw).ceil() as usize;
                let cols = natural.clamp(8, avail);
                let rows = ((cols as f32 * cw) * h as f32 / w as f32 / ch).ceil() as usize;
                (cols, rows.clamp(2, 60))
            }
            _ => (avail.min(60), 10),
        };
        let start = self.y;
        for r in 0..rows {
            self.bars(quote);
            self.g.put(start + r, x0, "", Role::Text);
        }
        self.g.picture(start, x0, rows, cols, what);
        self.y = start + rows;
    }

    fn diagram(&mut self, source: &str, embed: Option<String>, quote: usize, ctx: &Ctx) {
        let x0 = quote * 2;
        if let Some(err) = (ctx.diagram_error)(source) {
            self.bars(quote);
            self.g.put(self.y, x0, &fit(&format!("diagram: {err}"), self.width.saturating_sub(x0)), Role::Bad);
            self.y += 1;
            self.code("mermaid", source, quote);
            return;
        }
        self.picture(Picture::Diagram(source.to_string()), x0, quote, ctx);
        self.bars(quote);
        let caption = match &embed {
            Some(name) => format!("{name} · diagram"),
            None => "diagram".to_string(),
        };
        self.g.put(self.y, x0, &fit(&caption, self.width.saturating_sub(x0)), Role::Muted);
        self.y += 1;
    }

    fn table(&mut self, align: &[pulldown_cmark::Alignment], header: &[Vec<Span>], rows: &[Vec<Vec<Span>>], quote: usize, ctx: &Ctx) {
        let x0 = quote * 2;
        let ncols = header.len().max(rows.iter().map(Vec::len).max().unwrap_or(0));
        if ncols == 0 {
            return;
        }
        let text = |cell: &Vec<Span>| blocks::plain(cell);
        let mut widths = vec![1usize; ncols];
        for (i, cell) in header.iter().enumerate() {
            widths[i] = widths[i].max(text(cell).chars().count());
        }
        for row in rows {
            for (i, cell) in row.iter().enumerate() {
                widths[i] = widths[i].max(text(cell).chars().count());
            }
        }
        // Squeezed, widest first, to fit.
        let avail = self.width.saturating_sub(x0 + 3 * (ncols - 1));
        while widths.iter().sum::<usize>() > avail {
            let (i, &w) = widths.iter().enumerate().max_by_key(|(_, w)| **w).unwrap();
            if w <= 4 {
                break;
            }
            widths[i] = w - 1;
        }
        let total: usize = widths.iter().sum::<usize>() + 3 * (ncols - 1);
        let write_row = |me: &mut Writer, cells: &[Vec<Span>], head: bool| {
            me.bars(quote);
            let mut x = x0;
            for (i, w) in widths.iter().enumerate() {
                let cell = cells.get(i).cloned().unwrap_or_default();
                let t = fit(&text(&cell), *w);
                let pad = w.saturating_sub(t.chars().count());
                let start = match align.get(i) {
                    Some(pulldown_cmark::Alignment::Right) => x + pad,
                    Some(pulldown_cmark::Alignment::Center) => x + pad / 2,
                    _ => x,
                };
                let role = if head { Role::Title } else { cell.first().map(|s| role_of(s, ctx)).unwrap_or(Role::Text) };
                me.g.put(me.y, start, &t, role);
                x += w;
                if i + 1 < ncols {
                    me.g.put(me.y, x + 1, "│", Role::Muted);
                    x += 3;
                }
            }
            me.y += 1;
        };
        write_row(self, header, true);
        // The rule under the header gets a row of its own.
        self.bars(quote);
        self.g.rule(self.y, x0..x0 + total);
        self.y += 1;
        for row in rows {
            write_row(self, row, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_note_lays_out_with_headings_lists_and_links() {
        let text = "# Title\n\nSome **bold** text and a [[Link]] that wraps when the line is narrow.\n\n- [ ] open\n- [x] done\n";
        let s = layout(text, 30, &Ctx::plain());
        let t = s.text();
        let lines: Vec<&str> = t.lines().collect();
        assert_eq!(lines[0], "TITLE");
        assert_eq!(lines[2], "Some bold text and a Link that");
        assert_eq!(lines[3], "wraps when the line is narrow.");
        assert_eq!(lines[5], "[ ] open");
        assert_eq!(lines[6], "[x] done");
        assert!(s.rules.iter().any(|(l, _)| *l == 0));
        assert!(s.spans.iter().any(|(l, cols, role)| *l == 2 && *cols == (21..25) && *role == syntax("text.uri")));
    }

    #[test]
    fn tables_code_and_diagrams() {
        let text = "| A | Long header |\n|---|---:|\n| 1 | 2 |\n\n```c\nint x;\n```\n\n```mermaid\nflowchart TD\n a-->b\n```\n";
        let s = layout(text, 40, &Ctx::plain());
        let t = s.text();
        let lines: Vec<&str> = t.lines().collect();
        assert_eq!(lines[0], "A │ Long header");
        assert_eq!(lines[2], "1 │           2");
        assert!(s.rules.iter().any(|(l, _)| *l == 1));
        assert_eq!(lines[4], " int x;                                c");
        assert!(s.panels.iter().any(|(l, _)| *l == 4));
        let (line, _, rows, _, what) = &s.pictures[0];
        assert_eq!((*line, *rows), (6, 10));
        assert_eq!(*what, Picture::Diagram("flowchart TD\n a-->b".into()));
        assert_eq!(lines[16], "diagram");
    }

    #[test]
    fn quotes_callouts_and_missing_links() {
        let ctx = Ctx { link_exists: &|l| l.target != "Nope", ..Ctx::plain() };
        let s = layout("> [!warning] Careful\n> body [[Nope]]\n", 40, &ctx);
        let t = s.text();
        let lines: Vec<&str> = t.lines().collect();
        assert_eq!(lines[0], "┃ Careful");
        assert_eq!(lines[1], "│ body Nope");
        assert!(s.spans.iter().any(|(l, _, role)| *l == 1 && *role == Role::Warn));
    }

    #[test]
    fn a_sheet_is_drawn_onto_a_page_cut_to_its_room() {
        let s = layout("# One\n\ntwo\n\nthree\n", 30, &Ctx::plain());
        let mut g = Grid::new();
        s.draw(&mut g, 2, 4, 3, &|_| None);
        let t = g.text();
        assert!(t.contains("    ONE") && t.contains("    two") && !t.contains("three"), "{t}");
    }
}
