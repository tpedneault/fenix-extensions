//! `cargo xtask theme-import <theme.json> [--out <dir>] [--name "..."] [--fenix <version>] [--source "..."]`:
//! a VS Code colour theme as a Fenix theme file, and a report of what it
//! couldn't map.
//!
//! **What's read.** A VS Code theme's JSON, with comments and trailing
//! commas as VS Code allows: `type`, `colors` (the workbench's colours),
//! `tokenColors` (TextMate rules, or the path of a JSON file of them),
//! `semanticTokenColors`, and `include` (another theme it builds on,
//! whose colours and rules its own override). A `.tmTheme` (a plist) is
//! refused: convert it to JSON first.
//!
//! **How it maps: by what a colour is for**, not by its name.
//!
//! - *Surfaces.* `bg` is `editor.background`; `sidebar_bg` the side bar's.
//!   `bg_modeline` isn't the status bar: Fenix draws its popups, palette,
//!   title bars and notifications on it, with body text and syntax colours
//!   on top, so it's the first of `editorWidget.background` (VS Code's
//!   own popups), the tab strip, the title bar, the status bar, the side
//!   bar that keeps the editor's text at 4.5:1 -- else a step from `bg`
//!   towards `fg`. `hl_line`, `selection`, `bracket_match`, `search_match`
//!   (every match: `editor.findMatchHighlightBackground`) keep their alpha.
//! - *Syntax.* Each Fenix role is looked up the way VS Code colours a
//!   token: TextMate scopes a grammar gives that kind of thing
//!   (`keyword.control`, `entity.name.function`, `constant.numeric`...)
//!   matched against `tokenColors`' selectors, the most specific rule
//!   winning, a later one on a tie. A role no rule colours is body text,
//!   as VS Code would draw it. Interface, parameter and macro -- what only
//!   a language server tells apart -- take `semanticTokenColors` first.
//! - *Git.* `gitDecoration.*`: added is Fenix's staged green (Fenix uses
//!   it for "added" in the terminal's green and its notes too), not
//!   `stageModifiedResourceForeground`, which is an amber in most themes.
//! - *What VS Code has no colour for* is derived from the theme's own:
//!   the mode badges by hue from its terminal, Git and syntax colours
//!   (Normal blue, Insert green, Visual purple, Replace amber, Command
//!   cyan; Explorer as Normal, Picker as Visual), turning a colour's hue
//!   when it has none near; their lettering the theme's background or
//!   text, whichever reads better; the folder icon Normal's blue; the
//!   caret's colour as text the first accent that reads on `bg_modeline`.
//!   Every one is said in the report.
//! - *Text with alpha* (VS Code allows it, Fenix doesn't) is laid over the
//!   surface it's drawn on, and written opaque.
//! - *The Fenix it's for* (`--fenix`, 1.0 unless said): Fenix refuses a
//!   whole theme with a key it doesn't know, so terminal colours (1.4),
//!   bracket pair colours (1.5) and interface, parameter and macro (1.5.2)
//!   are only written for a Fenix that reads them; below that, the report
//!   says what was left out and which `--fenix` would keep it.
//!
//! **What can't be mapped**, and the report lists: workbench colours with
//! no place in Fenix (buttons, lists, badges, the minimap, diff and merge
//! colours, ...), token rules no role asks about (language-specific and
//! Markdown ones mostly), font styles (Fenix's themes have none), token
//! backgrounds, and semantic colours with modifiers or for one language.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::fenix_theme;

const USAGE: &str = "cargo xtask theme-import <theme.json> [--out <dir>] [--name \"Theme name\"] [--fenix <version>] [--source \"where it's from\"]";

/// Red, green, blue and alpha, each 0 to 1.
type Col = [f32; 4];

/// What the command line says.
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    pub input: PathBuf,
    pub out: PathBuf,
    pub name: Option<String>,
    pub floor: semver::Version,
    pub source: Option<String>,
}

pub fn options(args: &[String]) -> Result<Options, String> {
    let mut o = Options { input: PathBuf::new(), out: PathBuf::from("."), name: None, floor: semver::Version::new(1, 0, 0), source: None };
    let mut input = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f, Some(v.to_string())),
            _ => (arg.as_str(), None),
        };
        let mut value = |what: &str| inline.clone().or_else(|| args.next().cloned()).ok_or(format!("{what} needs a value -- {USAGE}"));
        match flag {
            "--out" => o.out = PathBuf::from(value("--out")?),
            "--name" => o.name = Some(value("--name")?),
            "--source" => o.source = Some(value("--source")?),
            "--fenix" => o.floor = version(&value("--fenix")?)?,
            f if f.starts_with("--") => return Err(format!("theme-import: {f}? -- {USAGE}")),
            _ if input.is_none() => input = Some(PathBuf::from(arg)),
            _ => return Err(format!("theme-import: one theme at a time -- {USAGE}")),
        }
    }
    o.input = input.ok_or(USAGE)?;
    if let Some(name) = &o.name {
        if name.trim().is_empty() || name.chars().any(char::is_control) {
            return Err(format!("--name {name:?}: a few words, on one line"));
        }
    }
    if o.source.as_ref().is_some_and(|s| s.chars().any(char::is_control)) {
        return Err("--source: one line".into());
    }
    Ok(o)
}

/// `1.4` or `1.5.2` as a version.
fn version(s: &str) -> Result<semver::Version, String> {
    let full = match s.matches('.').count() {
        0 => format!("{s}.0.0"),
        1 => format!("{s}.0"),
        _ => s.to_string(),
    };
    semver::Version::parse(&full).map_err(|e| format!("--fenix {s}: {e}"))
}

pub fn run(args: &[String]) -> Result<(), String> {
    let o = options(args)?;
    let (toml, report) = import(&o)?;
    std::fs::create_dir_all(&o.out).map_err(|e| format!("{}: {e}", o.out.display()))?;
    let path = o.out.join(&report.file);
    std::fs::write(&path, toml).map_err(|e| format!("{}: {e}", path.display()))?;
    println!("wrote {}", path.display());
    print!("{report}");
    Ok(())
}

/// A TextMate rule from `tokenColors`.
#[derive(Debug, Clone)]
struct Rule {
    /// Each selector it lists (`"a, b"` and `["a", "b"]` alike).
    selectors: Vec<String>,
    foreground: Option<String>,
    background: Option<String>,
    font_style: Option<String>,
}

/// A VS Code theme, its `include`s folded in.
#[derive(Debug, Default)]
struct Vs {
    name: Option<String>,
    kind: Option<String>,
    colors: Vec<(String, String)>,
    rules: Vec<Rule>,
    semantic: Vec<(String, serde_json::Value)>,
    /// Top-level keys not read.
    other: Vec<String>,
}

impl Vs {
    fn color(&self, key: &str) -> Option<&str> {
        self.colors.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
}

fn read_json(path: &Path) -> Result<serde_json::Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    crate::jsonc::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Reads `path` and what it includes, the included first.
fn load(path: &Path, seen: &mut Vec<PathBuf>) -> Result<Vs, String> {
    let canonical = path.canonicalize().map_err(|e| format!("{}: {e}", path.display()))?;
    if seen.contains(&canonical) {
        return Err(format!("{}: includes itself, by way of {}", path.display(), seen.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(" -> ")));
    }
    seen.push(canonical);
    let doc = read_json(path)?;
    let obj = doc.as_object().ok_or(format!("{}: a theme is a JSON object", path.display()))?;
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut vs = match obj.get("include").and_then(|i| i.as_str()) {
        Some(include) => load(&dir.join(include), seen)?,
        None => Vs::default(),
    };
    for (key, value) in obj {
        match key.as_str() {
            "name" => vs.name = value.as_str().map(str::to_string),
            "type" => vs.kind = value.as_str().map(str::to_string),
            // Monaco's way of saying it (`vs`, `vs-dark`), when `type` doesn't.
            "base" if !obj.contains_key("type") => vs.kind = value.as_str().map(str::to_string),
            "include" | "$schema" => {}
            "colors" => {
                for (k, v) in value.as_object().ok_or(format!("{}: colors isn't an object", path.display()))? {
                    match v.as_str() {
                        Some(v) => vs.colors.push((k.clone(), v.to_string())),
                        // `null` takes back an included colour.
                        None => vs.colors.retain(|(c, _)| c != k),
                    }
                }
            }
            "tokenColors" => match value {
                serde_json::Value::String(file) => {
                    let file = dir.join(file);
                    if file.extension().is_some_and(|x| x.eq_ignore_ascii_case("tmTheme")) {
                        return Err(format!("{}: tokenColors is a TextMate .tmTheme ({}), a plist this doesn't read -- convert it to JSON first", path.display(), file.display()));
                    }
                    let doc = read_json(&file)?;
                    let rules = doc.as_array().cloned().or_else(|| doc.get("tokenColors").and_then(|t| t.as_array()).cloned()).or_else(|| doc.get("settings").and_then(|t| t.as_array()).cloned());
                    vs.rules.extend(rules.ok_or(format!("{}: no rules in it", file.display()))?.iter().filter_map(rule));
                }
                serde_json::Value::Array(rules) => vs.rules.extend(rules.iter().filter_map(rule)),
                _ => return Err(format!("{}: tokenColors is a list of rules, or a file's path", path.display())),
            },
            "semanticTokenColors" => {
                for (k, v) in value.as_object().ok_or(format!("{}: semanticTokenColors isn't an object", path.display()))? {
                    vs.semantic.retain(|(s, _)| s != k);
                    vs.semantic.push((k.clone(), v.clone()));
                }
            }
            other => vs.other.push(other.to_string()),
        }
    }
    seen.pop();
    Ok(vs)
}

fn rule(v: &serde_json::Value) -> Option<Rule> {
    let settings = v.get("settings")?;
    let field = |k: &str| settings.get(k).and_then(|f| f.as_str()).map(str::to_string);
    let selectors = match v.get("scope") {
        Some(serde_json::Value::String(s)) => s.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
        Some(serde_json::Value::Array(a)) => a.iter().filter_map(|s| s.as_str()).flat_map(|s| s.split(',')).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
        _ => Vec::new(),
    };
    Some(Rule { selectors, foreground: field("foreground"), background: field("background"), font_style: field("fontStyle") })
}

/// `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa`.
fn parse_hex(s: &str) -> Option<Col> {
    let d = s.trim().strip_prefix('#')?;
    if !d.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let expand: String = match d.len() {
        3 | 4 => d.chars().flat_map(|c| [c, c]).collect(),
        6 | 8 => d.to_string(),
        _ => return None,
    };
    let byte = |i: usize| u8::from_str_radix(&expand[i..i + 2], 16).ok().map(|b| b as f32 / 255.0);
    Some([byte(0)?, byte(2)?, byte(4)?, if expand.len() == 8 { byte(6)? } else { 1.0 }])
}

fn byte(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// `#rrggbb`, or `#rrggbbaa` when it's see-through.
fn hex(c: Col) -> String {
    let rgb = format!("#{:02x}{:02x}{:02x}", byte(c[0]), byte(c[1]), byte(c[2]));
    if byte(c[3]) == 255 { rgb } else { format!("{rgb}{:02x}", byte(c[3])) }
}

/// `top` laid over `under`, opaque.
fn over(top: Col, under: Col) -> Col {
    let a = top[3];
    [top[0] * a + under[0] * (1.0 - a), top[1] * a + under[1] * (1.0 - a), top[2] * a + under[2] * (1.0 - a), 1.0]
}

fn mix(a: Col, b: Col, t: f32) -> Col {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t, 1.0]
}

/// WCAG's relative luminance, as Fenix works it out.
fn luminance(c: Col) -> f32 {
    let lin = |v: f32| if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
    0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2])
}

fn contrast(a: Col, b: Col) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// Hue in degrees, saturation and lightness.
fn hsl(c: Col) -> (f32, f32, f32) {
    let (max, min) = (c[0].max(c[1]).max(c[2]), c[0].min(c[1]).min(c[2]));
    let l = (max + min) / 2.0;
    let d = max - min;
    if d < 1e-6 {
        return (0.0, 0.0, l);
    }
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let h = if max == c[0] { ((c[1] - c[2]) / d).rem_euclid(6.0) } else if max == c[1] { (c[2] - c[0]) / d + 2.0 } else { (c[0] - c[1]) / d + 4.0 };
    (h * 60.0, s, l)
}

fn from_hsl(h: f32, s: f32, l: f32) -> Col {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (h.rem_euclid(360.0) / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [r + m, g + m, b + m, 1.0]
}

fn hue_distance(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(360.0);
    d.min(360.0 - d)
}

/// How many dot-separated parts of `scope` the selector part `sel`
/// matches, when it's `scope` or a prefix of it at a dot.
fn segment_match(sel: &str, scope: &str) -> Option<usize> {
    (scope == sel || (scope.starts_with(sel) && scope[sel.len()..].starts_with('.'))).then(|| sel.split('.').count())
}

/// How well one selector (`a b`, `a - b`) matches a stack of scopes,
/// outermost first: its last part against the innermost scope, the parts
/// before it against those around it, in order. More of the innermost
/// scope matched is more specific; then more parts.
fn selector_score(selector: &str, stack: &[&str]) -> Option<(usize, usize)> {
    let (positive, negative) = match selector.split_once(" - ") {
        Some((p, n)) => (p, Some(n)),
        None => (selector, None),
    };
    let path: Vec<&str> = positive.split_whitespace().filter(|p| *p != ">").collect();
    let (leaf, parents) = path.split_last()?;
    let (inner, around) = stack.split_last()?;
    let leaf_score = segment_match(leaf, inner)?;
    let mut limit = around.len();
    for parent in parents.iter().rev() {
        limit = (0..limit).rev().find(|&j| segment_match(parent, around[j]).is_some())?;
    }
    if negative.is_some_and(|n| n.split('-').any(|n| selector_score(n.trim(), stack).is_some())) {
        return None;
    }
    Some((leaf_score, path.len()))
}

/// A Fenix key, what it was given, and where that came from.
#[derive(Debug, Clone)]
struct Role {
    key: &'static str,
    value: String,
    /// `None`: straight from the VS Code key `from` names.
    derived: Option<String>,
    from: Option<String>,
}

/// What the import said.
#[derive(Debug, Default)]
pub struct Report {
    pub name: String,
    pub file: String,
    pub light: bool,
    /// Fenix keys and the VS Code colour each was given.
    pub mapped: Vec<(String, String)>,
    /// Fenix keys given a colour VS Code doesn't name: the key and how.
    pub derived: Vec<(String, String)>,
    /// Fenix keys left out, for Fenix to work out: the key and why.
    pub omitted: Vec<(String, String)>,
    /// What the theme has that the Fenix it's for can't read.
    pub floor_dropped: Vec<(String, String)>,
    /// `colors` keys nothing was taken from.
    pub unmapped_colors: Vec<String>,
    /// Token rules no Fenix role was given a colour from: their selectors.
    pub unused_rules: Vec<String>,
    /// Semantic colours not used, and why.
    pub semantic_unmapped: Vec<(String, String)>,
    /// Font styles the rules ask for, and how often.
    pub font_styles: BTreeMap<String, usize>,
    /// Token rules with a background of their own.
    pub backgrounds: usize,
    /// Top-level keys not read.
    pub other_keys: Vec<String>,
    /// Colours that couldn't be read.
    pub unreadable: Vec<String>,
    pub notes: Vec<String>,
}

impl std::fmt::Display for Report {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{} ({}): {}", self.name, if self.light { "light" } else { "dark" }, self.file)?;
        let list = |f: &mut std::fmt::Formatter<'_>, title: &str, items: &[(String, String)]| -> std::fmt::Result {
            if !items.is_empty() {
                writeln!(f, "\n{title} ({}):", items.len())?;
                for (k, why) in items {
                    writeln!(f, "  {k}: {why}")?;
                }
            }
            Ok(())
        };
        list(f, "Mapped", &self.mapped)?;
        list(f, "Fenix roles VS Code has no colour for, derived", &self.derived)?;
        list(f, "Fenix roles left for Fenix to work out", &self.omitted)?;
        list(f, "In the theme, but not read by the Fenix it's for", &self.floor_dropped)?;
        if !self.unmapped_colors.is_empty() {
            writeln!(f, "\nWorkbench colours with no place in a Fenix theme ({}):", self.unmapped_colors.len())?;
            let mut groups: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
            for key in &self.unmapped_colors {
                let (group, rest) = key.split_once('.').unwrap_or(("", key));
                groups.entry(group).or_default().push(rest);
            }
            for (group, keys) in groups {
                writeln!(f, "  {}: {}", if group.is_empty() { "(top level)" } else { group }, keys.join(", "))?;
            }
        }
        if !self.unused_rules.is_empty() {
            writeln!(f, "\nToken rules no Fenix role took its colour from ({}):", self.unused_rules.len())?;
            for r in &self.unused_rules {
                writeln!(f, "  {r}")?;
            }
        }
        list(f, "Semantic token colours not used", &self.semantic_unmapped)?;
        if !self.font_styles.is_empty() {
            let styles: Vec<String> = self.font_styles.iter().map(|(s, n)| format!("{s} x{n}")).collect();
            writeln!(f, "\nFont styles (Fenix's themes have none): {}", styles.join(", "))?;
        }
        if self.backgrounds > 0 {
            writeln!(f, "\nToken backgrounds (Fenix colours text only): {} rule(s)", self.backgrounds)?;
        }
        if !self.other_keys.is_empty() {
            writeln!(f, "\nNot read: {}", self.other_keys.join(", "))?;
        }
        if !self.unreadable.is_empty() {
            writeln!(f, "\nColours that aren't colours, skipped: {}", self.unreadable.join(", "))?;
        }
        if !self.notes.is_empty() {
            writeln!(f)?;
            for n in &self.notes {
                writeln!(f, "Note: {n}")?;
            }
        }
        Ok(())
    }
}

/// A theme's name as a file name: `Vitesse Dark Soft` is
/// `vitesse-dark-soft`.
pub fn file_stem(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars().map(|c| match c {
        'é' | 'è' | 'ê' | 'É' => 'e',
        'á' | 'à' | 'â' => 'a',
        c => c,
    }) {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() { "theme".into() } else { out }
}

/// The theme `o` names, as a Fenix theme file, and what was said.
pub fn import(o: &Options) -> Result<(String, Report), String> {
    let vs = load(&o.input, &mut Vec::new())?;
    let fallback_name = o.input.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let name = o.name.clone().or(vs.name.clone()).filter(|n| !n.trim().is_empty()).unwrap_or(fallback_name).trim().to_string();
    let source = o.source.clone().unwrap_or_else(|| o.input.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
    let (text, mut report) = convert(&vs, &name, &o.floor, &source);
    // What's written is what Fenix reads: held to its rules here too.
    fenix_theme::check(&text, Some(&o.floor)).map_err(|e| format!("the theme made from {} doesn't read in Fenix ({e}) -- a bug in theme-import", o.input.display()))?;
    report.file = format!("{}.toml", file_stem(&name));
    Ok((text, report))
}

/// The conversion itself: `vs` as Fenix's theme file for Fenix `floor`.
fn convert(vs: &Vs, name: &str, floor: &semver::Version, source: &str) -> (String, Report) {
    let mut report = Report { name: name.to_string(), other_keys: vs.other.clone(), ..Report::default() };
    let mut used: BTreeSet<String> = BTreeSet::new();
    let mut used_rules: BTreeSet<usize> = BTreeSet::new();
    let mut roles: Vec<Role> = Vec::new();
    let reads = |v: semver::Version| *floor >= v;

    // A colour of `colors`, read, or said to be unreadable.
    let mut unreadable = BTreeSet::new();
    let mut color = |key: &str| -> Option<Col> {
        let raw = vs.color(key)?;
        let c = parse_hex(raw);
        if c.is_none() {
            unreadable.insert(format!("{key} = {raw:?}"));
        }
        c
    };

    // Light or dark: what it says it is, else how bright its page is.
    let bg_raw = color("editor.background");
    let light = match vs.kind.as_deref() {
        Some("light" | "hcLight" | "vs" | "hc-light") => true,
        Some("dark" | "hc" | "hcDark" | "vs-dark" | "hc-black") => false,
        _ => match bg_raw {
            Some(bg) => luminance(bg) > 0.5,
            None => {
                report.notes.push("the theme says neither light nor dark and has no editor.background: read as dark, as VS Code would".into());
                false
            }
        },
    };
    report.light = light;
    let paper: Col = if light { [1.0, 1.0, 1.0, 1.0] } else { [0.0, 0.0, 0.0, 1.0] };

    // The page and the ink, which everything else is laid over or read on.
    let (bg, bg_how) = match bg_raw {
        Some(c) if c[3] < 1.0 => (over(c, paper), Some(format!("editor.background has alpha: laid over {}", if light { "white" } else { "black" }))),
        Some(c) => (c, None),
        None => (parse_hex(if light { "#ffffff" } else { "#1e1e1e" }).unwrap(), Some("no editor.background: VS Code's own for a theme this light or dark".into())),
    };
    if bg_raw.is_some() {
        used.insert("editor.background".into());
    }
    let global_fg = vs.rules.iter().enumerate().rev().find(|(_, r)| r.selectors.is_empty() && r.foreground.is_some());
    let (fg, fg_how, fg_from) = if let Some(c) = color("editor.foreground") {
        used.insert("editor.foreground".into());
        (over(c, bg), (c[3] < 1.0).then(|| "editor.foreground has alpha: laid over the background".to_string()), Some("editor.foreground".to_string()))
    } else if let Some((i, c)) = global_fg.and_then(|(i, r)| Some((i, parse_hex(r.foreground.as_deref()?)?))) {
        used_rules.insert(i);
        (over(c, bg), None, Some("tokenColors' rule with no scope".to_string()))
    } else if let Some(c) = color("foreground") {
        used.insert("foreground".into());
        (over(c, bg), None, Some("foreground".to_string()))
    } else {
        (parse_hex(if light { "#000000" } else { "#d4d4d4" }).unwrap(), Some("no editor.foreground: VS Code's own for a theme this light or dark".into()), None)
    };

    roles.push(Role { key: "surfaces.bg", value: hex(bg), from: bg_raw.is_some().then(|| "editor.background".into()), derived: bg_how });
    roles.push(Role { key: "text.fg", value: hex(fg), from: fg_from, derived: fg_how });

    // Token colours, by the scopes a grammar gives each kind of thing.
    let token = |probes: &[&str], used_rules: &mut BTreeSet<usize>| -> Option<(Col, String)> {
        for probe in probes {
            let stack: Vec<&str> = probe.split_whitespace().collect();
            let best = vs
                .rules
                .iter()
                .enumerate()
                .filter(|(_, r)| r.foreground.as_deref().and_then(parse_hex).is_some())
                .filter_map(|(i, r)| r.selectors.iter().filter_map(|s| selector_score(s, &stack)).max().map(|score| (score, i)))
                .max();
            if let Some((_, i)) = best {
                used_rules.insert(i);
                let c = parse_hex(vs.rules[i].foreground.as_deref().unwrap()).unwrap();
                return Some((over(c, bg), format!("tokenColors: {}", vs.rules[i].selectors.join(", "))));
            }
        }
        None
    };
    const SYNTAX: &[(&str, &[&str])] = &[
        ("keyword", &["keyword.control", "keyword", "storage.type", "storage.modifier"]),
        ("string", &["string.quoted.double", "string"]),
        ("comment", &["comment.line.double-slash", "comment.block", "comment"]),
        ("function", &["entity.name.function", "support.function"]),
        ("type", &["entity.name.type", "entity.name.class", "support.type", "support.class"]),
        ("number", &["constant.numeric"]),
        ("constant", &["constant.language", "variable.other.constant", "constant.other", "constant"]),
        ("variable", &["variable.other.readwrite", "variable.other", "variable"]),
        ("operator", &["keyword.operator"]),
        ("punctuation", &["punctuation.separator", "punctuation.definition", "punctuation", "meta.brace"]),
        ("attribute", &["meta.attribute", "entity.name.function.decorator", "meta.decorator", "entity.other.attribute-name", "meta.preprocessor"]),
    ];
    let mut syntax: BTreeMap<&str, Col> = BTreeMap::new();
    let mut syntax_roles = Vec::new();
    for (role, probes) in SYNTAX {
        let key: &'static str = match *role {
            "keyword" => "syntax.keyword",
            "string" => "syntax.string",
            "comment" => "syntax.comment",
            "function" => "syntax.function",
            "type" => "syntax.type",
            "number" => "syntax.number",
            "constant" => "syntax.constant",
            "variable" => "syntax.variable",
            "operator" => "syntax.operator",
            "punctuation" => "syntax.punctuation",
            _ => "syntax.attribute",
        };
        match token(probes, &mut used_rules) {
            Some((c, from)) => {
                syntax.insert(role, c);
                syntax_roles.push(Role { key, value: hex(c), from: Some(from), derived: None });
            }
            None => {
                syntax.insert(role, fg);
                syntax_roles.push(Role { key, value: hex(fg), from: None, derived: Some(format!("no token rule colours a {role}: body text, as VS Code draws it")) });
            }
        }
    }

    // What only a language server tells apart: its own colour first.
    let semantic_plain = |kind: &str| -> Option<(Col, String)> {
        let (sel, v) = vs.semantic.iter().rev().find(|(s, _)| s == kind)?;
        let raw = v.as_str().map(str::to_string).or_else(|| v.get("foreground").and_then(|f| f.as_str()).map(str::to_string))?;
        Some((over(parse_hex(&raw)?, bg), format!("semanticTokenColors: {sel}")))
    };
    let mut lsp_roles = Vec::new();
    let mut semantic_used = BTreeSet::new();
    for (kind, key, probes, since) in [
        ("interface", "syntax.interface", &["entity.name.type.interface"][..], (1, 5, 2)),
        ("parameter", "syntax.parameter", &["variable.parameter"][..], (1, 5, 2)),
        ("macro", "syntax.macro", &["entity.name.function.macro", "entity.name.function.preprocessor", "support.function.macro"][..], (1, 5, 2)),
    ] {
        let since = semver::Version::new(since.0, since.1, since.2);
        let mut scratch = used_rules.clone();
        let found = semantic_plain(kind).map(|(c, from)| (c, from, true)).or_else(|| token(probes, &mut scratch).map(|(c, from)| (c, from, false)));
        match found {
            Some((c, from, semantic)) if reads(since.clone()) => {
                if semantic {
                    semantic_used.insert(kind.to_string());
                } else {
                    used_rules = scratch;
                }
                lsp_roles.push(Role { key, value: hex(c), from: Some(from), derived: None });
            }
            Some((_, from, _)) => report.floor_dropped.push((key.into(), format!("{from} -- Fenix reads {key} from {since} (--fenix {}.{}.{})", since.major, since.minor, since.patch))),
            None => {
                if reads(since) {
                    report.omitted.push((key.into(), format!("nothing in the theme for it: Fenix gives it the {} colour", match kind { "interface" => "type's", "parameter" => "variable's", _ => "function's" })));
                }
            }
        }
    }

    // Colours with a hue, for what VS Code has no colour for.
    let mut hued: Vec<(String, Col)> = Vec::new();
    for key in [
        "terminal.ansiBlue", "terminal.ansiGreen", "terminal.ansiMagenta", "terminal.ansiYellow", "terminal.ansiCyan", "terminal.ansiRed",
        "terminal.ansiBrightBlue", "terminal.ansiBrightGreen", "terminal.ansiBrightMagenta", "terminal.ansiBrightYellow", "terminal.ansiBrightCyan", "terminal.ansiBrightRed",
        "gitDecoration.addedResourceForeground", "gitDecoration.modifiedResourceForeground", "gitDecoration.untrackedResourceForeground", "gitDecoration.conflictingResourceForeground", "gitDecoration.deletedResourceForeground",
        "editorCursor.foreground", "focusBorder", "textLink.foreground", "button.background", "badge.background", "activityBarBadge.background", "statusBar.background",
    ] {
        if let Some(c) = color(key) {
            hued.push((key.to_string(), over(c, bg)));
        }
    }
    for role in ["function", "keyword", "string", "type", "number", "constant", "attribute"] {
        hued.push((format!("its {role} colour"), syntax[role]));
    }
    let lively = |c: &Col| {
        let (_, s, l) = hsl(*c);
        s >= 0.25 && (0.2..=0.85).contains(&l)
    };
    // The nearest to `target` in hue, but none of `taken` (the colours
    // already given to the others of its group, so no two look the same).
    let by_hue = |target: f32, what: &str, taken: &[Col]| -> (Col, String) {
        let best = hued.iter().filter(|(_, c)| lively(c) && !taken.iter().any(|t| hex(*t) == hex(*c))).map(|(k, c)| (hue_distance(hsl(*c).0, target), k, *c)).fold(None, |best: Option<(f32, &String, Col)>, x| match best {
            Some(b) if b.0 <= x.0 => Some(b),
            _ => Some(x),
        });
        match best {
            Some((d, k, c)) if d <= 40.0 => (c, format!("the theme's {what}: {k} ({})", hex(c))),
            _ => {
                // The liveliest colour it has, turned to that hue.
                let seed = hued.iter().filter(|(_, c)| lively(c)).max_by(|a, b| hsl(a.1).1.total_cmp(&hsl(b.1).1));
                match seed {
                    Some((k, c)) => {
                        let (_, s, l) = hsl(*c);
                        let made = from_hsl(target, s, l);
                        (made, format!("no {what} in the theme: {k} ({}) turned to that hue", hex(*c)))
                    }
                    None => (from_hsl(target, 0.55, if light { 0.42 } else { 0.62 }), format!("the theme has no colour with a hue: a plain {what}")),
                }
            }
        }
    };

    // Mode badges.
    let modes = [("modes.normal", 215.0, "blue"), ("modes.insert", 130.0, "green"), ("modes.visual", 285.0, "purple"), ("modes.replace", 40.0, "amber"), ("modes.command", 185.0, "cyan")];
    let mut mode_cols = BTreeMap::new();
    let mut mode_roles = Vec::new();
    for (key, hue, what) in modes {
        let taken: Vec<Col> = mode_cols.values().copied().collect();
        let (c, how) = by_hue(hue, what, &taken);
        mode_cols.insert(key, c);
        mode_roles.push(Role { key, value: hex(c), from: None, derived: Some(how) });
    }
    mode_roles.push(Role { key: "modes.explorer", value: hex(mode_cols["modes.normal"]), from: None, derived: Some("Normal's colour".into()) });
    mode_roles.push(Role { key: "modes.picker", value: hex(mode_cols["modes.visual"]), from: None, derived: Some("Visual's colour".into()) });
    let lettering = |badges: &[Col]| -> (Col, &'static str) {
        let options: [(Col, &'static str); 4] = [(bg, "the background"), (fg, "the text colour"), ([0.0, 0.0, 0.0, 1.0], "black"), ([1.0, 1.0, 1.0, 1.0], "white")];
        let score = |c: Col| badges.iter().map(|b| contrast(c, *b)).fold(f32::MAX, f32::min);
        // The theme's own when one reads at 4.5:1 on every badge.
        let own = options[..2].iter().filter(|(c, _)| score(*c) >= 4.5).max_by(|a, b| score(a.0).total_cmp(&score(b.0)));
        *own.unwrap_or_else(|| options.iter().max_by(|a, b| score(a.0).total_cmp(&score(b.0))).unwrap())
    };
    let dark_badges: Vec<Col> = ["modes.normal", "modes.insert", "modes.replace", "modes.command"].iter().map(|k| mode_cols[k]).collect();
    let (text_dark, how) = lettering(&dark_badges);
    mode_roles.push(Role { key: "modes.text_dark", value: hex(text_dark), from: None, derived: Some(format!("{how}: what reads best on the badges")) });
    let (text_light, how) = lettering(&[mode_cols["modes.visual"]]);
    if hex(text_light) != hex(text_dark) {
        mode_roles.push(Role { key: "modes.text_light", value: hex(text_light), from: None, derived: Some(format!("{how}: what reads best on Visual's badge")) });
    }

    // Surfaces.
    let mut surface_roles = Vec::new();
    let sidebar = color("sideBar.background").map(|c| over(c, bg));
    if sidebar.is_some() {
        used.insert("sideBar.background".into());
    }
    let mut modeline = None;
    for (key, text_key) in [("editorWidget.background", "editorWidget.foreground"), ("editorGroupHeader.tabsBackground", "tab.activeForeground"), ("titleBar.activeBackground", "titleBar.activeForeground"), ("statusBar.background", "statusBar.foreground"), ("sideBar.background", "sideBar.foreground")] {
        if let Some(c) = color(key).map(|c| over(c, bg)) {
            if contrast(fg, c) >= 4.5 {
                modeline = Some((c, key, text_key));
                break;
            }
        }
    }
    let modeline_col = match modeline {
        Some((c, key, _)) => {
            used.insert(key.to_string());
            surface_roles.push(Role { key: "surfaces.bg_modeline", value: hex(c), from: Some(key.into()), derived: None });
            c
        }
        None => {
            let c = mix(bg, fg, 0.06);
            surface_roles.push(Role { key: "surfaces.bg_modeline", value: hex(c), from: None, derived: Some("no popup, tab strip, title bar or status bar colour keeps the text readable: a step from the background towards the text".into()) });
            c
        }
    };
    if let Some(s) = sidebar {
        surface_roles.push(Role { key: "surfaces.sidebar_bg", value: hex(s), from: Some("sideBar.background".into()), derived: None });
    } else {
        report.omitted.push(("surfaces.sidebar_bg".into(), "no sideBar.background: Fenix uses the editor's background".into()));
    }
    let fg_modeline = modeline.and_then(|(c, _, text_key)| color(text_key).map(|t| (over(t, c), text_key))).filter(|(t, _)| contrast(*t, modeline_col) >= 4.5);
    match fg_modeline {
        Some((t, key)) if hex(t) != hex(fg) => {
            used.insert(key.to_string());
            surface_roles.push(Role { key: "text.fg_modeline", value: hex(t), from: Some(key.into()), derived: None });
        }
        _ => report.omitted.push(("text.fg_modeline".into(), "the text colour, as Fenix has it".into())),
    }
    let divider = ["editorGroup.border", "sideBar.border", "panel.border", "editorGroupHeader.tabsBorder", "contrastBorder"].into_iter().find_map(|k| color(k).filter(|c| c[3] > 0.0).map(|c| (k, c)));
    match divider {
        Some((k, c)) => {
            used.insert(k.into());
            surface_roles.push(Role { key: "surfaces.divider", value: hex(c), from: Some(k.into()), derived: None });
        }
        None => surface_roles.push(Role { key: "surfaces.divider", value: hex(mix(bg, fg, 0.15)), from: None, derived: Some("no border colour: a step from the background towards the text".into()) }),
    }
    let caret = match color("editorCursor.foreground") {
        Some(c) => {
            used.insert("editorCursor.foreground".into());
            surface_roles.push(Role { key: "surfaces.caret", value: hex(over(c, bg)), from: Some("editorCursor.foreground".into()), derived: None });
            over(c, bg)
        }
        None => {
            surface_roles.push(Role { key: "surfaces.caret", value: hex(fg), from: None, derived: Some("no editorCursor.foreground: the text colour".into()) });
            fg
        }
    };
    let hl_line = if let Some(c) = color("editor.lineHighlightBackground") {
        used.insert("editor.lineHighlightBackground".into());
        Role { key: "surfaces.hl_line", value: hex(c), from: Some("editor.lineHighlightBackground".into()), derived: None }
    } else if let Some(c) = color("editor.lineHighlightBorder") {
        used.insert("editor.lineHighlightBorder".into());
        Role { key: "surfaces.hl_line", value: hex(c), from: Some("editor.lineHighlightBorder".into()), derived: Some("editor.lineHighlightBorder's colour as a fill: Fenix shades the line, it doesn't frame it".into()) }
    } else {
        Role { key: "surfaces.hl_line", value: hex(mix(bg, fg, 0.05)), from: None, derived: Some("no line highlight: a step from the background towards the text".into()) }
    };
    surface_roles.push(hl_line);
    let fill = |key: &str, fallback: Col, how: &str, used: &mut BTreeSet<String>, color: &mut dyn FnMut(&str) -> Option<Col>| -> (String, Option<String>, Option<String>) {
        match color(key) {
            Some(c) => {
                used.insert(key.into());
                (hex(c), Some(key.into()), None)
            }
            None => (hex(fallback), None, Some(how.into())),
        }
    };
    let with_alpha = |c: Col, a: f32| [c[0], c[1], c[2], a];
    let (v, from, how) = fill("editor.selectionBackground", with_alpha(caret, 0.25), "no editor.selectionBackground: the caret's colour at a quarter", &mut used, &mut color);
    surface_roles.push(Role { key: "surfaces.selection", value: v, from, derived: how });
    let bracket = match (color("editorBracketMatch.background"), color("editorBracketMatch.border")) {
        (Some(c), _) => {
            used.insert("editorBracketMatch.background".into());
            Role { key: "surfaces.bracket_match", value: hex(c), from: Some("editorBracketMatch.background".into()), derived: None }
        }
        (None, Some(c)) => {
            used.insert("editorBracketMatch.border".into());
            Role { key: "surfaces.bracket_match", value: hex(with_alpha(c, 0.35)), from: Some("editorBracketMatch.border".into()), derived: Some("editorBracketMatch.border's colour, at 35%, as a fill: Fenix shades a bracket's match, it doesn't frame it".into()) }
        }
        (None, None) => Role { key: "surfaces.bracket_match", value: hex(with_alpha(mode_cols["modes.insert"], 0.2)), from: None, derived: Some("no bracket match colour: Insert's green at a fifth".into()) },
    };
    surface_roles.push(bracket);
    let search = ["editor.findMatchHighlightBackground", "editor.findMatchBackground"].into_iter().find_map(|k| color(k).map(|c| (k, c)));
    match search {
        Some((k, c)) => {
            used.insert(k.into());
            surface_roles.push(Role { key: "surfaces.search_match", value: hex(c), from: Some(k.into()), derived: None });
        }
        None => surface_roles.push(Role { key: "surfaces.search_match", value: hex(with_alpha(mode_cols["modes.replace"], 0.3)), from: None, derived: Some("no find match colour: Replace's amber at 30%".into()) }),
    }
    if let Some(c) = color("window.activeBorder") {
        used.insert("window.activeBorder".into());
        surface_roles.push(Role { key: "border", value: hex(c), from: Some("window.activeBorder".into()), derived: None });
    }
    if reads(semver::Version::new(1, 4, 0)) {
        report.omitted.push(("surfaces.tray, card, input".into(), "VS Code has nothing like them: Fenix works them out from the background and text, keeping text readable".into()));
    }

    // Text.
    let mut text_roles = Vec::new();
    match color("editorLineNumber.foreground") {
        Some(c) => {
            used.insert("editorLineNumber.foreground".into());
            text_roles.push(Role { key: "text.gutter_fg", value: hex(over(c, bg)), from: Some("editorLineNumber.foreground".into()), derived: (c[3] < 1.0).then(|| "it has alpha: laid over the background".into()) });
        }
        None => text_roles.push(Role { key: "text.gutter_fg", value: hex(mix(bg, fg, 0.55)), from: None, derived: Some("no editorLineNumber.foreground: half way from the background to the text".into()) }),
    }
    // The caret's colour as text, on `bg_modeline` (the which-key keys).
    let caret_candidates: Vec<(String, Col)> = std::iter::once(("the caret".to_string(), caret))
        .chain(["textLink.foreground", "focusBorder"].into_iter().filter_map(|k| color(k).map(|c| (k.to_string(), over(c, modeline_col)))))
        .chain(["function", "keyword", "type"].into_iter().map(|r| (format!("its {r} colour"), syntax[r])))
        .collect();
    let readable = caret_candidates.iter().find(|(_, c)| contrast(*c, modeline_col) >= 4.5).or_else(|| caret_candidates.iter().max_by(|a, b| contrast(a.1, modeline_col).total_cmp(&contrast(b.1, modeline_col))));
    if let Some((what, c)) = readable {
        if what == "the caret" {
            report.omitted.push(("text.caret_text".into(), "the caret's own colour, which reads on bg_modeline".into()));
        } else {
            text_roles.push(Role { key: "text.caret_text", value: hex(*c), from: None, derived: Some(format!("the caret's colour doesn't read on bg_modeline: {what}, the first that does")) });
        }
    }

    // Explorer and Git.
    let mut tail_roles = Vec::new();
    match color("symbolIcon.folderForeground") {
        Some(c) => {
            used.insert("symbolIcon.folderForeground".into());
            tail_roles.push(Role { key: "explorer.folder", value: hex(over(c, sidebar.unwrap_or(bg))), from: Some("symbolIcon.folderForeground".into()), derived: None });
        }
        None => tail_roles.push(Role { key: "explorer.folder", value: hex(mode_cols["modes.normal"]), from: None, derived: Some("folder icons come with VS Code's icon theme, not its colour theme: Normal's blue".into()) }),
    }
    match color("sideBar.foreground") {
        Some(c) => {
            used.insert("sideBar.foreground".into());
            tail_roles.push(Role { key: "explorer.file", value: hex(over(c, sidebar.unwrap_or(bg))), from: Some("sideBar.foreground".into()), derived: None });
        }
        None => tail_roles.push(Role { key: "explorer.file", value: hex(fg), from: None, derived: Some("no sideBar.foreground: the text colour".into()) }),
    }
    let git_keys = ["gitDecoration.modifiedResourceForeground", "gitDecoration.addedResourceForeground", "gitDecoration.untrackedResourceForeground", "gitDecoration.conflictingResourceForeground"];
    let mut git_taken: Vec<Col> = git_keys.iter().filter_map(|k| color(k)).map(|c| over(c, sidebar.unwrap_or(bg))).collect();
    for (key, vs_key, hue, what) in [
        ("git.modified", "gitDecoration.modifiedResourceForeground", 40.0, "amber"),
        ("git.staged", "gitDecoration.addedResourceForeground", 130.0, "green"),
        ("git.untracked", "gitDecoration.untrackedResourceForeground", 160.0, "green"),
        ("git.ignored", "gitDecoration.ignoredResourceForeground", -1.0, ""),
        ("git.conflicted", "gitDecoration.conflictingResourceForeground", 0.0, "red"),
    ] {
        match color(vs_key) {
            Some(c) => {
                used.insert(vs_key.into());
                tail_roles.push(Role { key, value: hex(over(c, sidebar.unwrap_or(bg))), from: Some(vs_key.into()), derived: None });
            }
            None if hue < 0.0 => {
                let quiet = roles_value(&text_roles, "text.gutter_fg").unwrap_or_else(|| hex(fg));
                tail_roles.push(Role { key, value: quiet, from: None, derived: Some(format!("no {vs_key}: the line numbers' colour")) });
            }
            None => {
                let (c, how) = by_hue(hue, what, &git_taken);
                git_taken.push(c);
                tail_roles.push(Role { key, value: hex(c), from: None, derived: Some(format!("no {vs_key}: {how}")) });
            }
        }
    }

    // What later Fenix releases read.
    const ANSI: [&str; 16] = ["Black", "Red", "Green", "Yellow", "Blue", "Magenta", "Cyan", "White", "BrightBlack", "BrightRed", "BrightGreen", "BrightYellow", "BrightBlue", "BrightMagenta", "BrightCyan", "BrightWhite"];
    let ansi: Vec<Option<Col>> = ANSI.iter().map(|n| color(&format!("terminal.ansi{n}"))).collect();
    let mut terminal = None;
    if ansi.iter().all(Option::is_some) {
        if reads(semver::Version::new(1, 4, 0)) {
            for n in ANSI {
                used.insert(format!("terminal.ansi{n}"));
            }
            let bgt = color("terminal.background").map(|c| over(c, bg)).unwrap_or(bg);
            terminal = Some(ansi.iter().map(|c| hex(over(c.unwrap(), bgt))).collect::<Vec<_>>());
        } else {
            report.floor_dropped.push(("terminal.ansi".into(), "terminal.ansi* (all 16) -- Fenix reads a theme's terminal colours from 1.4.0 (--fenix 1.4); until then it makes them from the theme's Git and syntax colours".into()));
        }
    } else if reads(semver::Version::new(1, 4, 0)) {
        report.omitted.push(("terminal.ansi".into(), format!("the theme has {} of VS Code's 16 terminal colours: Fenix makes all 16 from its Git and syntax colours", ansi.iter().filter(|c| c.is_some()).count())));
    }
    let bracket_pairs: Vec<Col> = (1..=6).map_while(|i| color(&format!("editorBracketHighlight.foreground{i}"))).collect();
    let mut brackets = None;
    if !bracket_pairs.is_empty() {
        if reads(semver::Version::new(1, 5, 0)) {
            for i in 1..=bracket_pairs.len() {
                used.insert(format!("editorBracketHighlight.foreground{i}"));
            }
            brackets = Some(bracket_pairs.iter().map(|c| hex(over(*c, bg))).collect::<Vec<_>>());
        } else {
            report.floor_dropped.push(("brackets.colors".into(), format!("editorBracketHighlight.foreground1-{} -- Fenix reads bracket pair colours from 1.5.0 (--fenix 1.5); until then its rainbow brackets are the function, keyword and type colours", bracket_pairs.len())));
        }
    }

    // `tabs`: Visual Studio's filled tabs, with an accent along the top,
    // when the theme draws one there.
    let block = color("tab.activeBorderTop").is_some_and(|c| c[3] > 0.0);
    if block {
        used.insert("tab.activeBorderTop".into());
    }

    roles.extend(surface_roles);
    roles.extend(text_roles);
    roles.extend(mode_roles);
    roles.extend(syntax_roles);
    roles.extend(lsp_roles);
    roles.extend(tail_roles);
    for r in &roles {
        match (&r.derived, &r.from) {
            (Some(how), _) => report.derived.push((r.key.to_string(), how.clone())),
            (None, Some(from)) => report.mapped.push((r.key.to_string(), from.clone())),
            (None, None) => {}
        }
    }

    // What wasn't used.
    let mut seen_keys = BTreeSet::new();
    for (k, _) in vs.colors.iter().rev() {
        if seen_keys.insert(k.clone()) && !used.contains(k) {
            report.unmapped_colors.push(k.clone());
        }
    }
    report.unmapped_colors.sort();
    report.unmapped_colors.retain(|k| !unreadable.iter().any(|u| u.starts_with(&format!("{k} ="))));
    report.unreadable = unreadable.into_iter().collect();
    for (i, r) in vs.rules.iter().enumerate() {
        if let Some(style) = &r.font_style {
            for s in style.split_whitespace() {
                *report.font_styles.entry(s.to_string()).or_default() += 1;
            }
        }
        if r.background.is_some() {
            report.backgrounds += 1;
        }
        if !used_rules.contains(&i) && r.foreground.is_some() && !r.selectors.is_empty() {
            let shown: Vec<&str> = r.selectors.iter().take(4).map(String::as_str).collect();
            report.unused_rules.push(format!("{}{}", shown.join(", "), if r.selectors.len() > 4 { format!(", ... ({} in all)", r.selectors.len()) } else { String::new() }));
        }
    }
    for (selector, value) in &vs.semantic {
        if semantic_used.contains(selector) {
            continue;
        }
        for flag in ["bold", "italic", "underline", "strikethrough"] {
            if value.get(flag).and_then(|b| b.as_bool()) == Some(true) {
                *report.font_styles.entry(flag.into()).or_default() += 1;
            }
        }
        if let Some(style) = value.get("fontStyle").and_then(|s| s.as_str()) {
            for s in style.split_whitespace() {
                *report.font_styles.entry(s.to_string()).or_default() += 1;
            }
        }
        let (kind, language) = selector.split_once(':').map(|(k, l)| (k, Some(l))).unwrap_or((selector.as_str(), None));
        let (kind, modifiers) = kind.split_once('.').map(|(k, m)| (k, Some(m))).unwrap_or((kind, None));
        let role = match kind {
            "type" | "class" | "struct" | "enum" | "typeParameter" | "namespace" => Some("type"),
            "interface" => Some("interface"),
            "function" | "method" => Some("function"),
            "macro" => Some("macro"),
            "parameter" => Some("parameter"),
            "variable" | "property" => Some("variable"),
            "enumMember" => Some("constant"),
            "keyword" | "comment" | "string" | "number" | "operator" => Some(kind),
            "regexp" => Some("string"),
            "decorator" => Some("attribute"),
            _ => None,
        };
        let why = if language.is_some() {
            "Fenix colours a token by its type alone, not per language".to_string()
        } else if modifiers.is_some() || kind == "*" {
            "Fenix colours a token by its type alone, not its modifiers".to_string()
        } else {
            match role {
                Some(r @ ("interface" | "parameter" | "macro")) => format!("Fenix reads syntax.{r} from 1.5.2 (--fenix 1.5.2)"),
                Some(r) => format!("Fenix has one colour for a {r}, and takes it from the TextMate rules a grammar's tokens get"),
                None => "Fenix has no colour for it".to_string(),
            }
        };
        report.semantic_unmapped.push((selector.clone(), why));
    }
    if vs.other.iter().any(|k| k == "semanticHighlighting") {
        report.notes.push("semanticHighlighting isn't a theme's to say in Fenix: it uses a language server's tokens whenever one sends them".into());
    }

    // Readability, as published.
    let quiet = parse_hex(&roles_value(&roles, "text.gutter_fg").unwrap_or_default()).unwrap_or(fg);
    for (what, c) in [("body text", fg), ("comments", syntax["comment"]), ("line numbers", quiet)] {
        let ratio = contrast(c, bg);
        if ratio < 4.5 {
            report.notes.push(format!("{what} {} on {} is {ratio:.2}:1, as published -- Fenix's own themes keep 4.5:1", hex(c), hex(bg)));
        }
    }
    if let Some(built_in) = fenix_theme::BUILT_IN_NAMES.iter().find(|b| b.eq_ignore_ascii_case(name)) {
        report.notes.push(format!("Fenix ships a theme named {built_in}: installed, this one takes its place"));
    }

    // The file.
    let get = |key: &str| roles.iter().rev().find(|r| r.key == key).map(|r| r.value.clone());
    let q = |s: &str| toml::Value::String(s.to_string()).to_string();
    let mut out = String::new();
    let _ = writeln!(out, "# {name}: imported from a VS Code colour theme by `cargo xtask theme-import`.");
    let _ = writeln!(out, "# From: {source}");
    let _ = writeln!(out, "# For Fenix {floor} and later. What VS Code has no colour for is worked out");
    let _ = writeln!(out, "# from the theme's own (the importer's report says how).");
    let _ = writeln!(out, "name = {}", q(name));
    let _ = writeln!(out, "tabs = \"{}\"", if block { "block" } else { "underline" });
    if let Some(border) = get("border") {
        let _ = writeln!(out, "border = \"{border}\"");
    }
    let section = |out: &mut String, title: &str, keys: &[&str]| {
        let _ = writeln!(out, "\n[{title}]");
        for key in keys {
            if let Some(v) = get(&format!("{title}.{key}")) {
                let _ = writeln!(out, "{key} = \"{v}\"");
            }
        }
    };
    section(&mut out, "surfaces", &["bg", "sidebar_bg", "bg_modeline", "divider", "caret", "hl_line", "selection", "bracket_match", "search_match"]);
    if let Some(ansi) = &terminal {
        let _ = writeln!(out, "\n[terminal]\nansi = [{}]", ansi.iter().map(|c| format!("\"{c}\"")).collect::<Vec<_>>().join(", "));
    }
    if let Some(colors) = &brackets {
        let _ = writeln!(out, "\n[brackets]\ncolors = [{}]", colors.iter().map(|c| format!("\"{c}\"")).collect::<Vec<_>>().join(", "));
    }
    section(&mut out, "text", &["fg", "fg_modeline", "caret_text", "gutter_fg"]);
    section(&mut out, "modes", &["normal", "insert", "visual", "replace", "command", "explorer", "picker", "text_dark", "text_light"]);
    section(&mut out, "syntax", &["keyword", "string", "comment", "function", "type", "number", "constant", "variable", "operator", "punctuation", "attribute", "interface", "parameter", "macro"]);
    section(&mut out, "explorer", &["folder", "file"]);
    section(&mut out, "git", &["modified", "staged", "untracked", "ignored", "conflicted"]);
    (out, report)
}

fn roles_value(roles: &[Role], key: &str) -> Option<String> {
    roles.iter().rev().find(|r| r.key == key).map(|r| r.value.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/theme-import").join(name)
    }

    fn import_at(name: &str, floor: &str) -> (String, Report) {
        let o = options(&[fixture(name).display().to_string(), "--fenix".into(), floor.into()]).unwrap();
        import(&o).unwrap_or_else(|e| panic!("{name}: {e}"))
    }

    fn table(text: &str) -> toml::Table {
        toml::from_str(text).unwrap()
    }

    fn at<'a>(t: &'a toml::Table, path: &str) -> &'a str {
        let (s, k) = path.split_once('.').unwrap();
        t[s][k].as_str().unwrap_or_else(|| panic!("{path}"))
    }

    #[test]
    fn the_command_line_is_read() {
        let o = options(&["t.json".into(), "--out".into(), "d".into(), "--name=Dusk".into(), "--fenix".into(), "1.4".into()]).unwrap();
        assert_eq!((o.input.as_path(), o.out.as_path(), o.name.as_deref(), o.floor.to_string()), (Path::new("t.json"), Path::new("d"), Some("Dusk"), "1.4.0".to_string()));
        assert_eq!(options(&["t.json".into()]).unwrap().floor.to_string(), "1.0.0");
        assert!(options(&[]).is_err());
        assert!(options(&["a.json".into(), "b.json".into()]).unwrap_err().contains("one theme at a time"));
        assert!(options(&["a.json".into(), "--colour".into()]).unwrap_err().contains("--colour"));
        assert!(options(&["a.json".into(), "--name".into(), "a\nb".into()]).is_err());
    }

    #[test]
    fn colours_are_read_as_vs_code_writes_them() {
        assert_eq!(parse_hex("#fff"), Some([1.0, 1.0, 1.0, 1.0]));
        assert_eq!(hex(parse_hex("#0008").unwrap()), "#00000088");
        assert_eq!(hex(parse_hex("#12AbEf").unwrap()), "#12abef");
        assert_eq!(hex(parse_hex("#12abef80").unwrap()), "#12abef80");
        assert!(parse_hex("red").is_none() && parse_hex("#12345").is_none());
        assert_eq!(hex(over(parse_hex("#ffffff80").unwrap(), parse_hex("#000000").unwrap())), "#808080");
        assert_eq!(file_stem("Rosé Pine (Moon)"), "rose-pine-moon");
    }

    #[test]
    fn a_selector_matches_as_vs_code_matches_it() {
        assert_eq!(selector_score("keyword", &["keyword.control"]), Some((1, 1)));
        assert_eq!(selector_score("keyword.control", &["keyword.control"]), Some((2, 1)));
        assert_eq!(selector_score("keyword.control.flow", &["keyword.control"]), None, "more specific than the scope");
        assert_eq!(selector_score("key", &["keyword"]), None, "a whole part, not a prefix of one");
        assert_eq!(selector_score("source.rust keyword", &["keyword.control"]), None, "for one language only");
        assert_eq!(selector_score("meta.function keyword", &["source", "meta.function.rust", "keyword"]), Some((1, 2)));
        assert_eq!(selector_score("string - string.regexp", &["string.regexp"]), None);
        assert_eq!(selector_score("string - string.regexp", &["string.quoted"]), Some((1, 1)));
    }

    /// A dark theme with everything VS Code would say: each role from its
    /// own key, and what a newer Fenix reads only when it's for one.
    #[test]
    fn a_dark_theme_maps_by_meaning() {
        let (text, report) = import_at("dark.jsonc", "1.0");
        let t = table(&text);
        assert!(!report.light);
        assert_eq!(t["name"].as_str(), Some("Fixture Dark"));
        assert_eq!(at(&t, "surfaces.bg"), "#1b1d23");
        assert_eq!(at(&t, "surfaces.bg_modeline"), "#22252d", "the popups' colour, not the bright status bar");
        assert_eq!(at(&t, "surfaces.selection"), "#3a5a8c80", "an overlay keeps its alpha");
        assert_eq!(at(&t, "surfaces.search_match"), "#e5c07b44", "every match, as hlsearch shows them");
        assert_eq!(at(&t, "text.fg"), "#c8ccd4");
        assert_eq!(at(&t, "text.gutter_fg"), "#5b5f6a", "alpha laid over the page");
        assert_eq!(at(&t, "syntax.keyword"), "#c678dd");
        assert_eq!(at(&t, "syntax.operator"), "#56b6c2", "keyword.operator is more specific than keyword");
        assert_eq!(at(&t, "syntax.function"), "#61afef");
        assert_eq!(at(&t, "syntax.type"), "#e5c07b");
        assert_eq!(at(&t, "syntax.string"), "#98c379");
        assert_eq!(at(&t, "syntax.comment"), "#7f848e");
        assert_eq!(at(&t, "syntax.constant"), "#d19a66");
        assert_eq!(at(&t, "syntax.variable"), "#e06c75", "the later of two equal rules");
        assert_eq!(at(&t, "syntax.punctuation"), "#c8ccd4", "no rule: body text");
        assert_eq!(at(&t, "git.staged"), "#98c379", "added, not stageModified");
        assert_eq!(at(&t, "git.modified"), "#e5c07b");
        assert_eq!(at(&t, "modes.normal"), "#4d78cc", "the bluest of its colours: the status bar's");
        assert_eq!(at(&t, "modes.insert"), "#73c991", "the greenest");
        assert_eq!(at(&t, "explorer.folder"), "#4d78cc", "Normal's");
        assert_eq!(t["tabs"].as_str(), Some("block"), "an accent along the active tab's top");
        // Fenix 1.0 reads none of these: left out, and said.
        assert!(t.get("terminal").is_none() && t.get("brackets").is_none() && t["syntax"].get("parameter").is_none());
        let dropped: Vec<&str> = report.floor_dropped.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(dropped, ["syntax.interface", "syntax.parameter", "syntax.macro", "terminal.ansi", "brackets.colors"]);
        assert!(report.unmapped_colors.contains(&"button.background".to_string()));
        assert!(report.unmapped_colors.contains(&"gitDecoration.stageModifiedResourceForeground".to_string()));
        assert!(!report.unmapped_colors.contains(&"editor.background".to_string()));
        assert!(report.unused_rules.iter().any(|r| r.starts_with("markup.heading")));
        assert_eq!(report.font_styles.get("italic"), Some(&3), "two rules and a semantic colour");
        assert_eq!(report.backgrounds, 1);
        assert!(report.semantic_unmapped.iter().any(|(s, why)| s == "variable.readonly" && why.contains("modifiers")));
        assert!(report.semantic_unmapped.iter().any(|(s, why)| s == "property:python" && why.contains("per language")));
        assert!(report.derived.iter().any(|(k, _)| k == "modes.visual"));
        let said = report.to_string();
        for s in ["button: background", "Font styles", "--fenix 1.4", "modes.text_dark"] {
            assert!(said.contains(s), "{s}: {said}");
        }

        let (text, report) = import_at("dark.jsonc", "1.5.2");
        let t = table(&text);
        assert_eq!(t["terminal"]["ansi"].as_array().unwrap().len(), 16);
        assert_eq!(t["brackets"]["colors"].as_array().unwrap().iter().map(|c| c.as_str().unwrap()).collect::<Vec<_>>(), ["#ffd700", "#da70d6", "#179fff"]);
        assert_eq!(at(&t, "syntax.parameter"), "#ef596f", "the semantic colour first");
        assert_eq!(at(&t, "syntax.macro"), "#56b6c2", "a TextMate rule when there's no semantic one");
        assert!(report.floor_dropped.is_empty());
        assert!(!report.unmapped_colors.contains(&"terminal.ansiRed".to_string()));
    }

    /// A light theme that leaves most out, and builds on another by
    /// `include`: what's missing is derived and said, and nothing written
    /// is see-through where Fenix wants text.
    #[test]
    fn a_sparse_light_theme_is_filled_in_and_says_how() {
        let (text, report) = import_at("light.json", "1.0");
        let t = table(&text);
        assert!(report.light);
        assert_eq!(t["name"].as_str(), Some("Fixture Light"));
        assert_eq!(at(&t, "surfaces.bg"), "#fdfdfd");
        assert_eq!(at(&t, "syntax.keyword"), "#a626a4", "from the theme it includes");
        assert_eq!(at(&t, "syntax.string"), "#2e7d32", "its own rule over the included one");
        assert_eq!(at(&t, "surfaces.hl_line"), "#eeeeee", "the line's border as a fill");
        for key in ["surfaces.caret", "surfaces.divider", "modes.normal", "modes.visual", "git.staged", "git.conflicted", "explorer.folder"] {
            assert!(report.derived.iter().any(|(k, _)| k == key), "{key}: {:?}", report.derived);
        }
        let normal = parse_hex(at(&t, "modes.normal")).unwrap();
        assert!(hue_distance(hsl(normal).0, 215.0) <= 40.0, "Normal is a blue: {}", at(&t, "modes.normal"));
        let lettering = parse_hex(at(&t, "modes.text_dark")).unwrap();
        assert!(contrast(lettering, normal) >= 3.0);
        assert!(report.notes.iter().any(|n| n.contains("comments")), "{:?}", report.notes);
        let badges: BTreeSet<&str> = ["normal", "insert", "visual", "replace", "command"].iter().map(|m| at(&t, &format!("modes.{m}"))).collect();
        assert_eq!(badges.len(), 5, "no two modes alike: {badges:?}");
        // Monaco's `base` says light or dark when `type` doesn't; `rules`
        // (Monaco's own token rules) are said, not read.
        let (_, report) = import_at("monaco.json", "1.0");
        assert!(report.light && report.other_keys == ["rules"], "{:?}", report.other_keys);
    }

    /// A theme can't include itself, and a .tmTheme isn't read.
    #[test]
    fn what_cant_be_read_is_refused() {
        let o = |f: &str| options(&[fixture(f).display().to_string()]).unwrap();
        assert!(import(&o("loop.json")).unwrap_err().contains("includes itself"));
        assert!(import(&o("tmtheme.json")).unwrap_err().contains(".tmTheme"));
    }

    /// Every import, of every fixture, for every Fenix, reads under
    /// Fenix's rules for that Fenix (`import` checks; this says so).
    #[test]
    fn every_import_reads_in_the_fenix_it_is_for() {
        for f in ["dark.jsonc", "light.json", "base-light.json", "bare.json"] {
            for floor in ["1.0", "1.4", "1.5", "1.5.2"] {
                let (text, _) = import_at(f, floor);
                fenix_theme::check(&text, Some(&version(floor).unwrap())).unwrap_or_else(|e| panic!("{f} for {floor}: {e}"));
            }
        }
        // Nothing at all: still a theme, all of it said.
        let (text, report) = import_at("bare.json", "1.0");
        fenix_theme::check(&text, None).unwrap();
        assert!(report.derived.iter().any(|(k, why)| k == "surfaces.bg" && why.contains("VS Code's own")));
    }

    #[test]
    fn a_name_fenix_ships_is_said() {
        let o = options(&[fixture("bare.json").display().to_string(), "--name".into(), "nord".into()]).unwrap();
        let (_, report) = import(&o).unwrap();
        assert!(report.notes.iter().any(|n| n.contains("Fenix ships a theme named Nord")));
        assert_eq!(report.file, "nord.toml");
    }
}
