//! Mermaid diagrams without a browser: `mermaid-rs-renderer` parses,
//! lays out and writes SVG, `resvg` turns that into pixels. On top of
//! it, what Fenix needs: the theme a diagram is drawn in (Mermaid's own
//! five, `fenix` -- made from the editor's colours -- and your own), the
//! line an error is really on, which source line each node comes from,
//! the diagram types with a starter for each, and the source rewritten
//! so it renders the same anywhere (a Fenix-only theme spelled out).

use std::sync::{Arc, LazyLock};

use mermaid_rs_renderer::{compute_layout, parse_mermaid_strict, render_svg, LayoutConfig, ParseError, Theme};

mod route;
pub mod theme;

pub use theme::{EditorColors, ThemeSpec, Themes};

/// A diagram type: its header keyword, a short tag, what it's for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Kind {
    pub header: &'static str,
    pub tag: &'static str,
    pub about: &'static str,
}

pub const KINDS: &[Kind] = &[
    Kind { header: "flowchart", tag: "FLOW", about: "steps and decisions" },
    Kind { header: "sequenceDiagram", tag: "SEQ", about: "who sends what to whom, in order" },
    Kind { header: "stateDiagram-v2", tag: "STATE", about: "modes and the transitions between them" },
    Kind { header: "classDiagram", tag: "CLASS", about: "types and their relations" },
    Kind { header: "erDiagram", tag: "ER", about: "tables and keys" },
    Kind { header: "gantt", tag: "GANTT", about: "tasks on a timeline" },
    Kind { header: "timeline", tag: "TIME", about: "events by period" },
    Kind { header: "mindmap", tag: "MIND", about: "ideas around a centre" },
    Kind { header: "pie", tag: "PIE", about: "shares of a whole" },
    Kind { header: "gitGraph", tag: "GIT", about: "branches and merges" },
    Kind { header: "journey", tag: "JOURN", about: "a user's steps and how they feel" },
    Kind { header: "quadrantChart", tag: "QUAD", about: "items on two axes" },
    Kind { header: "xychart-beta", tag: "XY", about: "bars and lines on axes" },
    Kind { header: "sankey-beta", tag: "SANKEY", about: "flows between nodes" },
    Kind { header: "block-beta", tag: "BLOCK", about: "blocks in columns" },
    Kind { header: "packet-beta", tag: "PACKET", about: "a packet's fields by bit" },
    Kind { header: "requirementDiagram", tag: "REQ", about: "requirements and what satisfies them" },
    Kind { header: "C4Context", tag: "C4", about: "systems and people (C4)" },
    Kind { header: "architecture-beta", tag: "ARCH", about: "services and their groups" },
    Kind { header: "kanban", tag: "KANBAN", about: "cards in columns" },
];

/// The first line that isn't front matter, a directive or a comment.
fn header_line(source: &str) -> Option<(usize, &str)> {
    let mut in_fm = false;
    for (n, line) in source.lines().enumerate() {
        let t = line.trim();
        if n == 0 && t == "---" {
            in_fm = true;
            continue;
        }
        if in_fm {
            if t == "---" {
                in_fm = false;
            }
            continue;
        }
        if t.is_empty() || t.starts_with("%%") {
            continue;
        }
        return Some((n, t));
    }
    None
}

/// The diagram's type, from its header.
pub fn kind(source: &str) -> Option<Kind> {
    let (_, header) = header_line(source)?;
    let word = header.split(|c: char| c.is_whitespace() || c == ';').next().unwrap_or("");
    let word = if word == "graph" { "flowchart" } else { word };
    let word = if word == "stateDiagram" { "stateDiagram-v2" } else { word };
    KINDS.iter().find(|k| k.header.eq_ignore_ascii_case(word) || k.header.trim_end_matches("-beta").eq_ignore_ascii_case(word)).copied()
}

/// Small working examples to start a diagram from.
pub fn starters(kind: &Kind) -> Vec<(&'static str, &'static str)> {
    let example = match kind.header {
        "flowchart" => "flowchart TD\n    start([Request]) --> check{Valid?}\n    check -- yes --> done[Accepted]\n    check -- no --> fail[Rejected]\n",
        "sequenceDiagram" => "sequenceDiagram\n    participant G as Ground\n    participant S as Spacecraft\n    G->>S: TC(17,1) ping\n    S-->>G: TM(17,2) pong\n    Note over G,S: round trip\n",
        "stateDiagram-v2" => "stateDiagram-v2\n    [*] --> Idle\n    Idle --> Active: start\n    Active --> Idle: stop\n    Active --> [*]\n",
        "classDiagram" => "classDiagram\n    class Packet {\n        +u16 apid\n        +bytes data\n    }\n    class Frame\n    Frame o-- Packet\n",
        "erDiagram" => "erDiagram\n    TELECOMMAND ||--o{ PARAMETER : has\n    TELECOMMAND {\n        string name\n        int apid\n    }\n",
        "gantt" => "gantt\n    title Plan\n    dateFormat YYYY-MM-DD\n    section Build\n    Design   :a1, 2026-10-01, 7d\n    Code     :a2, after a1, 14d\n",
        "timeline" => "timeline\n    title Mission\n    2026 : Design\n    2027 : Build : Test\n    2028 : Launch\n",
        "mindmap" => "mindmap\n  root((Topic))\n    First\n      Detail\n    Second\n",
        "pie" => "pie title Share\n    \"A\" : 60\n    \"B\" : 40\n",
        "gitGraph" => "gitGraph\n    commit\n    branch feature\n    commit\n    checkout main\n    merge feature\n",
        _ => "",
    };
    let blank = match kind.header {
        "flowchart" => "flowchart TD\n    \n",
        h => Box::leak(format!("{h}\n    \n").into_boxed_str()),
    };
    let mut out = vec![("Blank", blank)];
    if !example.is_empty() {
        out.push(("Example", example));
    }
    out
}

/// A problem with a diagram, on a 0-based line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub line: usize,
    pub col: usize,
    pub message: String,
}

/// A node's box in the SVG's coordinates and the line it's defined on.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeBox {
    pub id: String,
    pub label: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub line: Option<usize>,
}

/// A drawn diagram.
#[derive(Debug, Clone)]
pub struct Rendered {
    pub svg: String,
    pub width: f32,
    pub height: f32,
    pub nodes: Vec<NodeBox>,
    /// The theme it was drawn in, by name.
    pub theme: String,
}

/// Where a parse error really is: the renderer reports some at 1:1 with
/// the offending text in the message, so that text is looked for.
fn locate(err: &ParseError, source: &str) -> Diagnostic {
    let message = err.to_string();
    let from1 = |l: u32| (l as usize).saturating_sub(1);
    match err {
        ParseError::UnknownParticipant { line, .. } => Diagnostic { line: from1(*line), col: 0, message },
        ParseError::UnclosedSubgraph { opened_at } => Diagnostic { line: from1(*opened_at), col: 0, message },
        ParseError::InvalidDirective { line, col, .. } => Diagnostic { line: from1(*line), col: (*col as usize).saturating_sub(1), message },
        ParseError::UnexpectedToken { line, col, expected, .. } => {
            // "invalid flowchart edge syntax: b -->" -- find that line.
            let fragment = expected.rsplit_once(": ").map(|(_, f)| f.trim()).filter(|f| !f.is_empty());
            let found = fragment.and_then(|f| source.lines().position(|l| l.trim() == f).or_else(|| source.lines().position(|l| l.contains(f))));
            let clean = match fragment {
                Some(f) => format!("{} -- {f}", expected.rsplit_once(": ").map(|(a, _)| a).unwrap_or(expected)),
                None => expected.clone(),
            };
            match found {
                Some(l) if *line <= 1 => Diagnostic { line: l, col: source.lines().nth(l).map(|t| t.len() - t.trim_start().len()).unwrap_or(0), message: clean },
                _ => Diagnostic { line: from1(*line), col: (*col as usize).saturating_sub(1), message: clean },
            }
        }
        _ => Diagnostic { line: header_line(source).map(|(l, _)| l).unwrap_or(0), col: 0, message },
    }
}

/// The line each node id is first written on.
fn node_line(source: &str, id: &str) -> Option<usize> {
    let skip = header_line(source).map(|(l, _)| l).unwrap_or(0);
    source.lines().enumerate().skip(skip + 1).find(|(_, l)| {
        let mut rest = *l;
        while let Some(at) = rest.find(id) {
            let before = rest[..at].chars().last();
            let after = rest[at + id.len()..].chars().next();
            let edge = |c: Option<char>| c.is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
            if edge(before) && edge(after) {
                return true;
            }
            rest = &rest[at + id.len()..];
        }
        false
    }).map(|(n, _)| n)
}

/// Draws `source` in the theme `themes` says for it (or `theme`, when
/// given, whatever the source asks for).
pub fn render(source: &str, themes: &Themes, theme: Option<&str>) -> Result<Rendered, Diagnostic> {
    if header_line(source).is_none() {
        return Err(Diagnostic { line: 0, col: 0, message: "empty -- a diagram starts with its type, like flowchart TD".into() });
    }
    let parsed = parse_mermaid_strict(source).map_err(|e| locate(&e, source))?;
    let (mermaid_theme, name) = themes.resolve(source, parsed.init_config.as_ref(), theme);
    let config = LayoutConfig::default();
    let mut layout = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| compute_layout(&parsed.graph, &mermaid_theme, &config)))
        .map_err(|_| Diagnostic { line: 0, col: 0, message: "the renderer couldn't lay this diagram out".into() })?;
    if layout.kind == mermaid_rs_renderer::DiagramKind::Flowchart {
        route::flowchart(&mut layout, parsed.graph.direction);
    }
    let svg = render_svg(&layout, &mermaid_theme, &config);
    let svg = if themes.rounded_edges { svg } else { square_edges(&svg) };
    let (width, height) = svg_size(&svg).unwrap_or((layout.width, layout.height));
    // Node boxes in the picture's own coordinates: the SVG's view box
    // can start left of or above 0 (a sequence diagram's does).
    let (ox, oy) = view_origin(&svg);
    let nodes = layout
        .nodes
        .values()
        .filter(|n| !n.hidden)
        .map(|n| NodeBox { id: n.id.clone(), label: n.label.lines.join(" "), x: n.x - ox, y: n.y - oy, w: n.width, h: n.height, line: node_line(source, &n.id) })
        .collect();
    Ok(Rendered { svg, width, height, nodes, theme: name })
}

/// Where the SVG's view box starts.
fn view_origin(svg: &str) -> (f32, f32) {
    let head = &svg[..svg.find('>').unwrap_or(0)];
    let Some(at) = head.find(" viewBox=\"").map(|a| a + 10) else { return (0.0, 0.0) };
    let mut parts = head[at..].split('"').next().unwrap_or("").split([' ', ',']).filter_map(|p| p.parse::<f32>().ok());
    (parts.next().unwrap_or(0.0), parts.next().unwrap_or(0.0))
}

fn svg_size(svg: &str) -> Option<(f32, f32)> {
    let attr = |name: &str| -> Option<f32> {
        let head = &svg[..svg.find('>')?];
        let at = head.find(&format!(" {name}=\""))? + name.len() + 3;
        head[at..].split('"').next()?.parse().ok()
    };
    Some((attr("width")?, attr("height")?))
}

/// The families diagrams are drawn in, as their files are named: Mermaid's
/// own choices and the usual sans-serifs.
const DIAGRAM_FONTS: &[&str] = &["trebuc", "verdana", "arial", "segoeui", "helvetica", "dejavusans", "liberationsans", "notosans", "inter", "roboto", "opensans", "ubuntu"];

static FONTS: LazyLock<Arc<resvg::usvg::fontdb::Database>> = LazyLock::new(|| {
    let mut db = resvg::usvg::fontdb::Database::new();
    // In Fenix's sandbox the system's fonts are at `/fonts/<n>`
    // (`permissions.fonts`); outside it, where the system keeps them.
    // There, only the families diagrams are drawn in are read: every
    // font on the system would be more memory than an extension has.
    let mut sandboxed = false;
    for n in 0.. {
        let dir = std::path::PathBuf::from(format!("/fonts/{n}"));
        let Ok(entries) = std::fs::read_dir(&dir) else { break };
        sandboxed = true;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_lowercase().replace([' ', '-', '_'], "");
            if DIAGRAM_FONTS.iter().any(|f| name.starts_with(f)) && (name.ends_with(".ttf") || name.ends_with(".otf") || name.ends_with(".ttc")) {
                // Read here: fontdb would map the file, which the sandbox
                // can't do.
                if let Ok(bytes) = std::fs::read(entry.path()) {
                    db.load_font_data(bytes);
                }
            }
        }
    }
    if !sandboxed {
        db.load_system_fonts();
    }
    Arc::new(db)
});

/// Edges drawn straight, their corners square: the renderer rounds each
/// corner of a routed edge (`L before Q corner after`); here the line goes
/// to the corner and turns there (`L corner L after`). Only edges are
/// touched -- a node's outline keeps its curves.
pub fn square_edges(svg: &str) -> String {
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg;
    while let Some(at) = rest.find("class=\"edgePath\"") {
        let Some(d_at) = rest[at..].find(" d=\"").map(|i| at + i + 4) else { break };
        let Some(end) = rest[d_at..].find('"').map(|i| d_at + i) else { break };
        out.push_str(&rest[..d_at]);
        out.push_str(&square_path(&rest[d_at..end]));
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// One edge's path made orthogonal: its rounded corners squared (`L
/// before Q corner after` -> `L corner L after`), then each slanted run
/// stepped -- down (or across, when it runs more sideways), over, and
/// down again through its middle -- so every line is level or plumb and
/// the last one still meets its arrowhead head on.
fn square_path(d: &str) -> String {
    let words: Vec<&str> = d.split_whitespace().collect();
    let mut cmds: Vec<String> = Vec::with_capacity(words.len());
    let mut i = 0;
    while i < words.len() {
        if words[i] == "Q" && i + 2 < words.len() && cmds.len() >= 2 && cmds[cmds.len() - 2] == "L" {
            cmds.pop();
            cmds.push(words[i + 1].to_string());
            cmds.push("L".into());
            cmds.push(words[i + 2].to_string());
            i += 3;
            continue;
        }
        cmds.push(words[i].to_string());
        i += 1;
    }
    // Anything but M and L (an arc, a curve the renderer drew itself):
    // left as it is.
    if cmds.iter().any(|w| w.chars().all(|c| c.is_ascii_alphabetic()) && w != "M" && w != "L") {
        return cmds.join(" ");
    }
    let points: Vec<(f32, f32)> = cmds
        .iter()
        .filter(|w| *w != "M" && *w != "L")
        .filter_map(|w| w.split_once(',').and_then(|(x, y)| Some((x.parse().ok()?, y.parse().ok()?))))
        .collect();
    if points.len() < 2 {
        return cmds.join(" ");
    }
    // The router's small wiggles first smoothed out, so each turn is one
    // clean step rather than a staircase.
    let points = simplify(&points, 10.0);
    let mut steps: Vec<(f32, f32)> = vec![points[0]];
    for pair in points.windows(2) {
        let ((x1, y1), (x2, y2)) = (pair[0], pair[1]);
        let (dx, dy) = (x2 - x1, y2 - y1);
        if dx.abs() > 0.5 && dy.abs() > 0.5 {
            if dy.abs() >= dx.abs() {
                let mid = (y1 + y2) / 2.0;
                steps.push((x1, mid));
                steps.push((x2, mid));
            } else {
                let mid = (x1 + x2) / 2.0;
                steps.push((mid, y1));
                steps.push((mid, y2));
            }
        }
        steps.push((x2, y2));
    }
    // A point in the middle of a straight run says nothing.
    let mut kept: Vec<(f32, f32)> = Vec::with_capacity(steps.len());
    for p in steps {
        if kept.last().is_some_and(|q: &(f32, f32)| (q.0 - p.0).abs() < 0.01 && (q.1 - p.1).abs() < 0.01) {
            continue;
        }
        if kept.len() >= 2 {
            let (a, b) = (kept[kept.len() - 2], kept[kept.len() - 1]);
            let cross = (b.0 - a.0) * (p.1 - b.1) - (b.1 - a.1) * (p.0 - b.0);
            if cross.abs() < 0.01 {
                kept.pop();
            }
        }
        kept.push(p);
    }
    let mut out = format!("M {:.3},{:.3}", kept[0].0, kept[0].1);
    for (x, y) in &kept[1..] {
        out.push_str(&format!(" L {x:.3},{y:.3}"));
    }
    out
}

/// `points` with every point closer than `tolerance` to the line between
/// its neighbours dropped (Ramer-Douglas-Peucker): its ends kept.
fn simplify(points: &[(f32, f32)], tolerance: f32) -> Vec<(f32, f32)> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let (a, b) = (points[0], points[points.len() - 1]);
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = dx.hypot(dy);
    let distance = |p: &(f32, f32)| if len < 1e-3 { (p.0 - a.0).hypot(p.1 - a.1) } else { ((p.0 - a.0) * dy - (p.1 - a.1) * dx).abs() / len };
    let (far, at) = points[1..points.len() - 1].iter().enumerate().map(|(i, p)| (distance(p), i + 1)).fold((0.0, 0), |m, x| if x.0 > m.0 { x } else { m });
    if far <= tolerance {
        return vec![a, b];
    }
    let mut left = simplify(&points[..=at], tolerance);
    left.pop();
    left.extend(simplify(&points[at..], tolerance));
    left
}

/// How many font faces diagrams can be drawn with.
pub fn fonts_loaded() -> usize {
    FONTS.len()
}

/// What's behind a drawn diagram.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Background {
    /// The theme's own.
    Theme,
    Transparent,
    White,
}

fn tree(svg: &str, background: Background) -> Result<resvg::usvg::Tree, String> {
    let svg = svg_with(svg, background);
    let opt = resvg::usvg::Options { fontdb: FONTS.clone(), ..Default::default() };
    resvg::usvg::Tree::from_str(&svg, &opt).map_err(|e| e.to_string())
}

/// The biggest side a raster is drawn at.
pub const MAX_PIXELS: u32 = 4096;

/// Pixels of `svg` at `scale` (clamped so no side passes `MAX_PIXELS`),
/// as straight RGBA: (width, height, pixels).
pub fn rasterize(svg: &str, scale: f32, background: Background) -> Result<(u32, u32, Vec<u8>), String> {
    let tree = tree(svg, background)?;
    let size = tree.size();
    let scale = scale.min(MAX_PIXELS as f32 / size.width().max(size.height()).max(1.0)).max(0.05);
    let (w, h) = ((size.width() * scale).ceil().max(1.0) as u32, (size.height() * scale).ceil().max(1.0) as u32);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h).ok_or("the picture would be empty")?;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    // tiny-skia keeps premultiplied alpha; straight alpha is wanted.
    let mut px = pixmap.take();
    for p in px.as_chunks_mut::<4>().0 {
        let a = p[3] as u32;
        if a > 0 && a < 255 {
            for c in &mut p[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    Ok((w, h, px))
}

/// The diagram as a PNG file's bytes.
pub fn png(svg: &str, scale: f32, background: Background) -> Result<Vec<u8>, String> {
    let tree = tree(svg, background)?;
    let size = tree.size();
    let scale = scale.min(MAX_PIXELS as f32 * 2.0 / size.width().max(size.height()).max(1.0));
    let (w, h) = ((size.width() * scale).ceil().max(1.0) as u32, (size.height() * scale).ceil().max(1.0) as u32);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h).ok_or("the picture would be empty")?;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    pixmap.encode_png().map_err(|e| e.to_string())
}

/// The SVG with its background as asked: the renderer paints a
/// backdrop rect first (before its `<defs>`), whose fill is replaced.
pub fn svg_with(svg: &str, background: Background) -> String {
    let fill = match background {
        Background::Theme => return svg.to_string(),
        Background::White => "#ffffff",
        Background::Transparent => "none",
    };
    let Some(open) = svg.find("<svg") else { return svg.to_string() };
    let Some(at) = svg[open..].find("<rect ").map(|a| a + open) else { return svg.to_string() };
    if svg.find("<defs").is_some_and(|d| d < at) {
        return svg.to_string();
    }
    let Some(end) = svg[at..].find("/>").map(|e| at + e) else { return svg.to_string() };
    let rect = &svg[at..end];
    let Some(f) = rect.find(" fill=\"") else { return svg.to_string() };
    let close = rect[f + 7..].find('"').map(|c| f + 7 + c).unwrap_or(rect.len());
    format!("{}{} fill=\"{fill}\"{}{}", &svg[..at], &rect[..f], &rect[close + 1..], &svg[end..])
}

/// The front matter lines' range, fences included.
fn front_matter(source: &str) -> Option<(usize, usize)> {
    let lines: Vec<&str> = source.lines().collect();
    if lines.first().map(|l| l.trim()) != Some("---") {
        return None;
    }
    let end = lines.iter().skip(1).position(|l| l.trim() == "---")? + 1;
    Some((0, end))
}

/// `source` with its theme set to `name` (in `config: theme:` front
/// matter, made when there's none).
pub fn set_theme(source: &str, name: &str) -> String {
    let mut lines: Vec<String> = source.lines().map(str::to_string).collect();
    let newline = source.ends_with('\n');
    match front_matter(source) {
        Some((_, end)) => {
            let config = (1..end).find(|&i| lines[i].trim_end() == "config:");
            let theme = config.and_then(|c| (c + 1..end).take_while(|&i| lines[i].starts_with(' ')).find(|&i| lines[i].trim_start().starts_with("theme:")));
            match (config, theme) {
                (_, Some(i)) => {
                    let indent: String = lines[i].chars().take_while(|c| *c == ' ').collect();
                    lines[i] = format!("{indent}theme: {name}");
                }
                (Some(c), None) => lines.insert(c + 1, format!("  theme: {name}")),
                (None, None) => {
                    lines.insert(end, "config:".into());
                    lines.insert(end + 1, format!("  theme: {name}"));
                }
            }
        }
        None => {
            lines.splice(0..0, ["---".to_string(), "config:".to_string(), format!("  theme: {name}"), "---".to_string()]);
        }
    }
    let mut out = lines.join("\n");
    if newline {
        out.push('\n');
    }
    out
}

/// `source` as it renders anywhere: a theme only Fenix knows (`fenix`,
/// one of yours) is written out as `base` and its variables.
pub fn portable(source: &str, themes: &Themes, theme: Option<&str>) -> String {
    let parsed_init = parse_mermaid_strict(source).ok().and_then(|p| p.init_config);
    let name = theme.map(str::to_string).unwrap_or_else(|| themes.name_for(source, parsed_init.as_ref()));
    if theme::is_builtin(&name) {
        return if theme.is_some() { set_theme(source, &name) } else { source.to_string() };
    }
    let vars = themes.variables_of(&name);
    // Drop any front matter theme lines, then write base + variables.
    let mut body: Vec<String> = source.lines().map(str::to_string).collect();
    if let Some((_, end)) = front_matter(source) {
        body.drain(0..=end);
    }
    let mut out = vec!["---".to_string(), "config:".to_string(), "  theme: base".to_string(), "  themeVariables:".to_string()];
    for (k, v) in vars {
        out.push(format!("    {k}: \"{v}\""));
    }
    out.push("---".to_string());
    out.extend(body);
    let mut text = out.join("\n");
    text.push('\n');
    text
}

/// The renderer's own theme for a name, for callers building one.
pub fn builtin(name: &str) -> Option<Theme> {
    theme::builtin(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Flowchart edges come square by default: a corner is a turn, not a
    /// curve -- unless rounded ones are asked for.
    #[test]
    fn edges_are_square_unless_rounded() {
        let source = "flowchart TD
  A[Request] --> B{Valid?}
  B -->|yes| C[Run]
  B -->|no| D[Reject]
";
        let edges = |svg: &str| svg.split("class=\"edgePath\"").skip(1).map(|p| p.split(" d=\"").nth(1).unwrap().split('"').next().unwrap().to_string()).collect::<Vec<_>>();
        let square = render(source, &Themes::default(), None).unwrap();
        assert!(!edges(&square.svg).is_empty());
        assert!(edges(&square.svg).iter().all(|d| !d.contains('Q')), "{:?}", edges(&square.svg));
        let rounded = render(source, &Themes { rounded_edges: true, ..Themes::default() }, None).unwrap();
        assert!(edges(&rounded.svg).iter().any(|d| d.contains('Q')), "the branches turn a corner");
        assert_eq!(square_path("M 0,0 L 10,0 Q 20,0 20,10 L 20,30"), "M 0.000,0.000 L 20.000,0.000 L 20.000,30.000", "a corner squared, a straight run one line");
        assert_eq!(square_path("M 0,0 L 10,40"), "M 0.000,0.000 L 0.000,20.000 L 10.000,20.000 L 10.000,40.000", "a slant stepped, down first");
        assert_eq!(square_path("M 0,0 L 40,10"), "M 0.000,0.000 L 20.000,0.000 L 20.000,10.000 L 40.000,10.000", "across first");
        let level = |d: &str| d.split(" L ").map(|p| p.trim_start_matches("M ").split_once(',').map(|(x, y)| (x.parse::<f32>().unwrap(), y.parse::<f32>().unwrap())).unwrap()).collect::<Vec<_>>().windows(2).all(|w| (w[0].0 - w[1].0).abs() < 0.01 || (w[0].1 - w[1].1).abs() < 0.01);
        assert!(edges(&square.svg).iter().all(|d| level(d)), "every line level or plumb: {:?}", edges(&square.svg));
    }

    /// A flowchart drawn as by hand: a decision's branches out of its
    /// side corners, no line turning more than twice, a loop round the
    /// side, and each label by where its line starts.
    #[test]
    fn flowcharts_route_like_hand_drawn_ones() {
        let source = "flowchart TD
  start([Start]) --> read[/Read A, B, C/]
  read --> ab{Is A > B}
  ab -- No --> bc{Is B > C}
  ab -- Yes --> ac{Is A > C}
  bc -- Yes --> pb[/Print B/]
  bc -- No --> pc[/Print C/]
  ac -- No --> pc
  ac -- Yes --> pa[/Print A/]
  pb --> stop([Stop])
  pc --> stop
  pa --> stop
  stop --> read
";
        let parsed = parse_mermaid_strict(source).unwrap();
        let (theme, _) = Themes::default().resolve(source, None, None);
        let config = LayoutConfig::default();
        let mut layout = compute_layout(&parsed.graph, &theme, &config);
        route::flowchart(&mut layout, parsed.graph.direction);
        let node = |id: &str| layout.nodes[id].clone();
        let edge = |from: &str, to: &str| layout.edges.iter().find(|e| e.from == from && e.to == to).unwrap().clone();
        for e in layout.edges.iter().filter(|e| e.to != "read") {
            assert!(e.points.len() <= 4, "{} -> {} turns at most twice: {:?}", e.from, e.to, e.points);
            assert!(e.points.windows(2).all(|w| (w[0].0 - w[1].0).abs() < 0.01 || (w[0].1 - w[1].1).abs() < 0.01), "{} -> {} level or plumb: {:?}", e.from, e.to, e.points);
        }
        // `Is A > B`'s branches leave from its left and right corners.
        let ab = node("ab");
        let (no, yes) = (edge("ab", "bc"), edge("ab", "ac"));
        assert!((no.points[0].0 - ab.x).abs() < 0.5 && (no.points[0].1 - (ab.y + ab.height / 2.0)).abs() < 0.5, "{:?}", no.points);
        assert!((yes.points[0].0 - (ab.x + ab.width)).abs() < 0.5, "{:?}", yes.points);
        assert_eq!(no.points.len(), 3, "one turn");
        // Its labels sit by those corners, above the line.
        let (lx, ly) = no.label_anchor.unwrap();
        assert!(lx < ab.x && ly < ab.y + ab.height / 2.0 && ab.x - lx < 60.0, "{lx},{ly}");
        // Straight down where boxes line up.
        assert_eq!(edge("start", "read").points.len(), 2);
        // The loop back up: Stop's sides taken by the lines merging into
        // it, out of its bottom, round, and into Read's side.
        let back = edge("stop", "read");
        let (stop, read) = (node("stop"), node("read"));
        assert_eq!(back.points.len(), 5, "{:?}", back.points);
        assert!((back.points[0].1 - (stop.y + stop.height)).abs() < 0.5 && (back.points[4].1 - (read.y + read.height / 2.0)).abs() < 0.5, "{:?}", back.points);
        assert!(back.points.windows(2).all(|w| (w[0].0 - w[1].0).abs() < 0.01 || (w[0].1 - w[1].1).abs() < 0.01), "{:?}", back.points);
    }

    const FLOW: &str = "flowchart TD\n  op[Operator request] --> val[Validate TC]\n  val --> mib{In MIB?}\n  mib -- yes --> enc([Encode])\n  mib -- no --> rej[Reject]\n";

    #[test]
    fn kinds_come_from_the_header() {
        assert_eq!(kind(FLOW).unwrap().tag, "FLOW");
        assert_eq!(kind("---\ntitle: x\n---\n%% c\ngraph LR\n").unwrap().tag, "FLOW");
        assert_eq!(kind("sequenceDiagram\n").unwrap().tag, "SEQ");
        assert_eq!(kind("xychart-beta\n").unwrap().tag, "XY");
        assert!(kind("nonsense\n").is_none());
        assert!(starters(&KINDS[0]).len() == 2);
        for k in KINDS {
            for (_, s) in starters(k) {
                assert_eq!(kind(s).map(|x| x.header), Some(k.header), "{s}");
            }
        }
    }

    #[test]
    fn a_flowchart_renders_with_its_nodes_lines() {
        let r = render(FLOW, &Themes::default(), None).unwrap();
        assert!(r.svg.starts_with("<svg"));
        assert!(r.width > 50.0 && r.height > 50.0);
        let rej = r.nodes.iter().find(|n| n.id == "rej").unwrap();
        assert_eq!(rej.line, Some(4));
        assert_eq!(rej.label, "Reject");
        assert_eq!(r.nodes.iter().find(|n| n.id == "op").unwrap().line, Some(1));
        let (w, h, px) = rasterize(&r.svg, 1.0, Background::Theme).unwrap();
        assert_eq!(px.len(), (w * h * 4) as usize);
        assert!(png(&r.svg, 1.0, Background::White).unwrap().starts_with(b"\x89PNG"));
    }

    #[test]
    fn node_boxes_are_in_the_pictures_coordinates() {
        let r = render("sequenceDiagram
    Ground->>OBC: hi
", &Themes::default(), None).unwrap();
        let g = r.nodes.iter().find(|n| n.id == "Ground").unwrap();
        // The view box starts at -50: the first actor sits 50 in.
        assert!(r.svg.contains("viewBox=\"-50 "));
        assert_eq!(g.x, 50.0);
    }

    #[test]
    fn errors_point_at_the_line_that_caused_them() {
        let e = render("flowchart TD\n  a --> b\n  b -->\n", &Themes::default(), None).unwrap_err();
        assert_eq!(e.line, 2);
        assert!(e.message.contains("b -->"), "{}", e.message);
        let e = render("sequenceDiagram\n  A->>B: hi\n  loop x\n", &Themes::default(), None).unwrap_err();
        assert_eq!(e.line, 2);
        assert!(render("", &Themes::default(), None).is_err());
    }

    #[test]
    fn themes_come_from_front_matter_directives_or_the_default() {
        let themes = Themes { default: "fenix".into(), ..Themes::default() };
        assert_eq!(render(FLOW, &themes, None).unwrap().theme, "fenix");
        let forest = set_theme(FLOW, "forest");
        assert!(forest.starts_with("---\nconfig:\n  theme: forest\n---\nflowchart TD"));
        assert_eq!(render(&forest, &themes, None).unwrap().theme, "forest");
        assert_eq!(set_theme(&forest, "dark").matches("theme:").count(), 1);
        let init = format!("%%{{init: {{\"theme\": \"neutral\"}}}}%%\n{FLOW}");
        assert_eq!(render(&init, &themes, None).unwrap().theme, "neutral");
        assert_eq!(render(FLOW, &themes, Some("dark")).unwrap().theme, "dark");
    }

    #[test]
    fn fenix_themes_are_written_out_for_elsewhere() {
        let themes = Themes { default: "fenix".into(), ..Themes::default() };
        let out = portable(FLOW, &themes, None);
        assert!(out.starts_with("---\nconfig:\n  theme: base\n  themeVariables:\n"), "{out}");
        assert!(out.contains("primaryColor: \"#"));
        assert!(out.ends_with(FLOW));
        assert_eq!(render(&out, &Themes::default(), None).unwrap().theme, "base");
        let forest = set_theme(FLOW, "forest");
        assert_eq!(portable(&forest, &themes, None), forest);
    }

    #[test]
    fn transparent_backgrounds_drop_the_backdrop() {
        let r = render(FLOW, &Themes::default(), None).unwrap();
        let clear = svg_with(&r.svg, Background::Transparent);
        assert!(clear.contains("fill=\"none\""));
        let seq = render("sequenceDiagram
    A->>B: hi
", &Themes::default(), None).unwrap();
        assert!(svg_with(&seq.svg, Background::White).contains("<rect x=\"-50\" y=\"-10\" width=\"450\""), "the backdrop at -50 too");
        assert!(svg_with(&seq.svg, Background::White).split("<defs").next().unwrap().contains("fill=\"#ffffff\""));
        let (_, _, px) = rasterize(&r.svg, 1.0, Background::Transparent).unwrap();
        assert_eq!(px[3], 0, "the corner is see-through");
    }
}
