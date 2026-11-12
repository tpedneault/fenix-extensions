//! A page as plain text, to read in a test and keep as a snapshot.
//!
//! [`text`] is what's on screen: a page's lines, or a canvas's cells, a
//! line each, with hairlines drawn (`─`, `│`), pictures as `▒`, a popup
//! under the line it's beside, and the keys at the foot. [`marked`] is
//! the same with what the colours and tints say: the focused line has a
//! `>` in the gutter, and a run of text in a role or on a background is
//! `«role bg=shade:text»`. Plain text is left as it is, and spaces at
//! lines' ends go, so the text doesn't change unless the page does.
//!
//! [`canvas`] and [`canvas_marked`] are the same for a `ui-kit`
//! [`Canvas`](fenix_ui_kit::Canvas) alone (feature `ui-kit`, on by
//! default).

use fenix_extension_api::{Canvas, Cells, Page, Role, Shade, Span};
use unicode_width::UnicodeWidthChar;

/// `page` as it reads on screen.
pub fn text(page: &Page) -> String {
    draw(page, false)
}

/// `page` with its focus, roles and backgrounds marked.
pub fn marked(page: &Page) -> String {
    draw(page, true)
}

/// A ui-kit canvas as it reads on screen, as the page `to_page` makes.
#[cfg(feature = "ui-kit")]
pub fn canvas(canvas: &fenix_ui_kit::Canvas) -> String {
    text(&canvas.to_page())
}

/// A ui-kit canvas with its focus, roles and backgrounds marked.
#[cfg(feature = "ui-kit")]
pub fn canvas_marked(canvas: &fenix_ui_kit::Canvas) -> String {
    marked(&canvas.to_page())
}

/// A role's name in a mark.
pub fn role_name(role: &Role) -> String {
    match role {
        Role::Title => "title".into(),
        Role::Text => "text".into(),
        Role::Muted => "muted".into(),
        Role::Accent => "accent".into(),
        Role::Good => "good".into(),
        Role::Warn => "warn".into(),
        Role::Bad => "bad".into(),
        Role::Syntax(name) => name.clone(),
    }
}

fn rgb((r, g, b): (u8, u8, u8)) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// A shade's name in a mark.
pub fn shade_name(shade: &Shade) -> String {
    match shade {
        Shade::Panel => "panel".into(),
        Shade::Focus => "focus".into(),
        Shade::Selection => "selection".into(),
        Shade::Accent => "accent".into(),
        Shade::Good => "good".into(),
        Shade::Warn => "warn".into(),
        Shade::Bad => "bad".into(),
        Shade::Rgb(c) => rgb(*c),
    }
}

fn width(s: &str) -> usize {
    s.chars().map(|c| c.width().unwrap_or(0)).sum()
}

fn spans(spans: &[Span], marks: bool) -> String {
    let mut out = String::new();
    for s in spans {
        if marks && s.role != Role::Text && !s.text.is_empty() {
            out.push_str(&format!("«{}:{}»", role_name(&s.role), s.text));
        } else {
            out.push_str(&s.text);
        }
    }
    out
}

fn trim(lines: Vec<String>) -> String {
    let mut lines: Vec<String> = lines.into_iter().map(|l| l.trim_end().to_string()).collect();
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

fn draw(page: &Page, marks: bool) -> String {
    let mut out = match &page.canvas {
        Some(canvas) => draw_canvas(page, canvas, marks),
        None => draw_lines(page, marks),
    };
    if marks {
        let flags: Vec<&str> = [(page.typing, "typing"), (page.space, "space"), (page.more_keys, "more keys"), (page.mouse, "mouse")].into_iter().filter(|(on, _)| *on).map(|(_, f)| f).collect();
        if !flags.is_empty() {
            out.push_str(&format!("[{}]\n", flags.join(", ")));
        }
    }
    out
}

fn draw_lines(page: &Page, marks: bool) -> String {
    let rule = page.lines.iter().filter(|l| !l.rule).map(|l| width(spans(&l.spans, false).trim_end())).max().unwrap_or(0).max(8);
    let mut lines = Vec::new();
    for (i, l) in page.lines.iter().enumerate() {
        let text = if l.rule { "─".repeat(rule) } else { spans(&l.spans, marks) };
        if marks {
            let gutter = if page.focus == Some(i as u32) { "> " } else { "  " };
            lines.push(format!("{gutter}{text}"));
        } else {
            lines.push(text);
        }
        if let Some(popup) = page.popup.as_ref().filter(|p| p.line as usize == i) {
            let indent = if marks { "    " } else { "  " };
            for row in &popup.rows {
                lines.push(format!("{indent}│ {}", spans(row, marks)));
            }
        }
    }
    let mut out = trim(lines);
    if !page.keys.is_empty() {
        out.push('\n');
        out.push_str(&page.keys.iter().map(|k| format!("{} {}", k.keys, k.what)).collect::<Vec<_>>().join("  "));
        out.push('\n');
    }
    out
}

/// What a canvas's cell is marked with.
#[derive(Clone, Default, PartialEq)]
struct Look {
    role: Option<String>,
    panel: bool,
    bg: Option<String>,
    under: Option<String>,
    color: Option<String>,
}

impl Look {
    fn tag(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        parts.extend(self.role.clone());
        if self.panel {
            parts.push("panel".into());
        }
        parts.extend(self.bg.as_ref().map(|b| format!("bg={b}")));
        parts.extend(self.under.as_ref().map(|u| format!("under={u}")));
        parts.extend(self.color.as_ref().map(|c| format!("fg={c}")));
        parts.join(" ")
    }
}

/// The chars of each line of `canvas`, long enough for every cell
/// anything is drawn in.
fn grid(page: &Page, canvas: &Canvas) -> Vec<Vec<char>> {
    let mut rows: Vec<Vec<char>> = canvas.text.split('\n').map(|l| l.chars().collect()).collect();
    let mut reach = |cells: &Cells| {
        let y = cells.line as usize;
        if rows.len() <= y {
            rows.resize(y + 1, Vec::new());
        }
        if rows[y].len() < cells.end as usize {
            rows[y].resize(cells.end as usize, ' ');
        }
    };
    for c in canvas.rules.iter().chain(&canvas.panels).chain(canvas.focus.iter()).chain(&page.focuses) {
        reach(c);
    }
    for s in &canvas.spans {
        reach(&s.cells);
    }
    for s in &page.styles {
        reach(&s.cells);
    }
    for p in &canvas.pictures {
        for r in 0..p.rows.max(1) {
            reach(&Cells { line: p.cells.line + r, ..p.cells });
        }
    }
    rows
}

/// The char index of display column `col` in `row`: none in the right
/// half of a wide character. Past the end, the spaces it would take.
fn at_column(row: &[char], col: usize) -> Option<usize> {
    let mut x = 0;
    for (i, c) in row.iter().enumerate() {
        if x == col {
            return Some(i);
        }
        x += c.width().unwrap_or(0);
        if x > col {
            return None;
        }
    }
    Some(row.len() + (col - x))
}

fn draw_canvas(page: &Page, canvas: &Canvas, marks: bool) -> String {
    let mut rows = grid(page, canvas);
    let over_blank = |rows: &mut Vec<Vec<char>>, y: usize, x: usize, glyph: char| {
        if rows.len() <= y {
            rows.resize(y + 1, Vec::new());
        }
        let row = &mut rows[y];
        if row.len() <= x {
            row.resize(x + 1, ' ');
        }
        if row[x] == ' ' {
            row[x] = glyph;
        }
    };
    for r in &canvas.rules {
        for x in r.start..r.end {
            over_blank(&mut rows, r.line as usize, x as usize, '─');
        }
    }
    for p in &canvas.pictures {
        for y in p.cells.line..p.cells.line + p.rows.max(1) {
            for x in p.cells.start..p.cells.end {
                over_blank(&mut rows, y as usize, x as usize, '▒');
            }
        }
    }
    for v in &page.vertical_rules {
        for y in v.top..v.bottom {
            let row = rows.get(y as usize).cloned().unwrap_or_default();
            if let Some(x) = at_column(&row, v.column as usize) {
                over_blank(&mut rows, y as usize, x, '│');
            }
        }
    }

    // What each cell is marked with.
    let mut looks: Vec<Vec<Look>> = rows.iter().map(|r| vec![Look::default(); r.len()]).collect();
    let mut each = |cells: &Cells, f: &dyn Fn(&mut Look)| {
        if let Some(row) = looks.get_mut(cells.line as usize) {
            for x in cells.start..cells.end.min(row.len() as u32) {
                f(&mut row[x as usize]);
            }
        }
    };
    if marks {
        for s in &canvas.spans {
            let name = (s.role != Role::Text).then(|| role_name(&s.role));
            each(&s.cells, &|l| l.role = name.clone());
        }
        for p in &canvas.panels {
            each(p, &|l| l.panel = true);
        }
        for s in &page.styles {
            if let Some(bg) = &s.background {
                let name = shade_name(bg);
                each(&s.cells, &|l| l.bg = Some(name.clone()));
            }
            if let Some(u) = &s.underline {
                let name = role_name(u);
                each(&s.cells, &|l| l.under = Some(name.clone()));
            }
        }
        for c in &page.colors {
            let name = rgb(c.color);
            each(&c.cells, &|l| l.color = Some(name.clone()));
        }
    }
    let focused: Vec<u32> = canvas.focus.iter().chain(&page.focuses).map(|c| c.line).collect();

    let mut lines = Vec::new();
    for (y, row) in rows.iter().enumerate() {
        let mut text = String::new();
        if marks {
            text.push_str(if focused.contains(&(y as u32)) { "> " } else { "  " });
            // Runs of one look; a plain one's trailing spaces may go.
            let mut x = 0;
            while x < row.len() {
                let look = &looks[y][x];
                let mut end = x + 1;
                while end < row.len() && looks[y][end] == *look {
                    end += 1;
                }
                let run: String = row[x..end].iter().collect();
                let tag = look.tag();
                if tag.is_empty() {
                    text.push_str(&run);
                } else {
                    text.push_str(&format!("«{tag}:{run}»"));
                }
                x = end;
            }
        } else {
            text.extend(row.iter());
        }
        lines.push(text);
        if let Some(popup) = canvas.popup.as_ref().filter(|p| p.line as usize == y) {
            let indent = " ".repeat(popup.column as usize + if marks { 2 } else { 0 });
            for r in &popup.rows {
                lines.push(format!("{indent}│ {}", spans(r, marks)));
            }
        }
    }
    trim(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fenix_extension_api::{page as p, CellSpan, CellStyle};

    #[test]
    fn a_page_of_lines_reads_as_it_shows() {
        let page = Page::new()
            .line([p::title("Counter")])
            .rule()
            .line([p::text("count "), p::accent("3")])
            .line([p::muted("trailing   ")])
            .focus(2)
            .popup(2, [vec![p::text("Reset")], vec![p::bad("Delete")]])
            .key("j", "count")
            .key("q", "close");
        assert_eq!(text(&page), "Counter\n────────\ncount 3\n  │ Reset\n  │ Delete\ntrailing\n\nj count  q close\n");
        assert_eq!(marked(&page), "  «title:Counter»\n  ────────\n> count «accent:3»\n    │ Reset\n    │ «bad:Delete»\n  «muted:trailing   »\n\nj count  q close\n");
    }

    #[test]
    fn a_canvas_reads_as_its_cells_with_its_hairlines() {
        let cells = |line, start, end| Cells { line, start, end };
        let canvas = Canvas {
            text: "日本 ok\n\nname  value".into(),
            spans: vec![CellSpan { cells: cells(0, 0, 2), role: Role::Title }, CellSpan { cells: cells(2, 6, 11), role: Role::Good }],
            rules: vec![cells(1, 0, 6)],
            focus: Some(cells(2, 0, 11)),
            panels: vec![cells(2, 0, 4)],
            pictures: Vec::new(),
            popup: None,
        };
        let page = Page::canvas(canvas).vertical_rule(4, 0, 3).typing(true);
        let page = Page { styles: vec![CellStyle { cells: cells(0, 3, 5), background: Some(Shade::Rgb((1, 2, 255))), underline: Some(Role::Accent) }], ..page };
        assert_eq!(text(&page), "日本│ok\n──────\nname│ value\n", "the rule down column 4 is in cells, past a wide character, and only over blanks");
        assert_eq!(marked(&page), "  «title:日本»│«bg=#0102ff under=accent:ok»\n  ──────\n> «panel:name»│ «good:value»\n[typing]\n");
    }

    #[cfg(feature = "ui-kit")]
    #[test]
    fn a_ui_kit_canvas_reads_the_same() {
        use fenix_ui_kit::{Canvas, Fill, Role as R, Style};
        let mut c = Canvas::new();
        c.put(0, 0, "Rows", R::Title);
        c.put(6, 0, "Plan", Style::new(R::Muted).bg(Fill::Panel));
        c.rule(1, 0..10);
        c.put(0, 2, "a", R::Text);
        c.focus(2, 0..10);
        assert_eq!(canvas(&c), "Rows  Plan\n──────────\na\n");
        assert_eq!(canvas_marked(&c), "  «title:Rows»  «muted panel:Plan»\n  ──────────\n> a\n");
    }
}
