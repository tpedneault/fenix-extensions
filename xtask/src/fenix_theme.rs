//! A theme file, held to Fenix's own rules, as `validate` holds a manifest
//! to them: the structs and checks of Fenix's `crates/fenix-gui/src/theme.rs`
//! (`ThemeFile`, `ColorSpec`, `parse`), copied, so what's refused here is
//! what Fenix refuses -- every section and key known (Fenix denies unknown
//! ones), each colour `#rrggbb`, `#rrggbbaa` or `{ color, alpha }`, text
//! never see-through, `tabs` underline or block, 16 terminal colours, one
//! to six bracket colours.
//!
//! **And the Fenix it's for.** Fenix reads a theme with
//! `deny_unknown_fields`, so a key newer than the Fenix reading it makes
//! the whole theme fail to load there. [`INTRODUCED`] says which Fenix
//! first read each optional key (from its tags), and [`check`] refuses one
//! the extension's `engine.fenix` floor can't read.

use serde::Deserialize;

/// The keys a Fenix after 1.0 added, and the first release to read each.
pub const INTRODUCED: &[(&str, (u64, u64, u64))] = &[
    ("surfaces.tray", (1, 4, 0)),
    ("surfaces.card", (1, 4, 0)),
    ("surfaces.input", (1, 4, 0)),
    ("terminal", (1, 4, 0)),
    ("brackets", (1, 5, 0)),
    ("syntax.interface", (1, 5, 2)),
    ("syntax.parameter", (1, 5, 2)),
    ("syntax.macro", (1, 5, 2)),
];

/// The names of the themes Fenix ships (`BUILT_IN`, by their `name`). An
/// installed theme with one of these names (any case) takes its place.
pub const BUILT_IN_NAMES: &[&str] = &[
    "Orbit Dark", "TempleOS", "Gruvbox Dark", "Nord", "Dracula", "Solarized Dark", "One Dark", "Visual Studio Dark", "Catppuccin Latte", "Catppuccin Frappé", "Catppuccin Macchiato", "Catppuccin Mocha", "Monokai", "GitHub Dark", "Tokyo Night Storm", "Kanagawa", "Rosé Pine", "Everforest Dark", "Ayu Mirage", "Night Owl", "Material Palenight", "Zenburn", "GitHub Light", "One Light", "Solarized Light", "Gruvbox Light", "Rosé Pine Dawn", "Tokyo Night Day", "Visual Studio Light",
];

#[derive(Deserialize)]
#[serde(untagged)]
enum ColorSpec {
    Hex(String),
    #[allow(dead_code)]
    Alpha { color: String, alpha: f32 },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct ThemeFile {
    name: String,
    #[serde(default)]
    tabs: Option<String>,
    #[serde(default)]
    font_family: Option<String>,
    #[serde(default)]
    border: Option<ColorSpec>,
    surfaces: Surfaces,
    #[serde(default)]
    terminal: Option<TerminalColors>,
    #[serde(default)]
    brackets: Option<BracketColors>,
    text: Text,
    modes: Modes,
    syntax: Syntax,
    explorer: Explorer,
    git: Git,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Surfaces {
    bg: ColorSpec,
    #[serde(default)]
    sidebar_bg: Option<ColorSpec>,
    #[serde(default)]
    tray: Option<ColorSpec>,
    #[serde(default)]
    card: Option<ColorSpec>,
    #[serde(default)]
    input: Option<ColorSpec>,
    bg_modeline: ColorSpec,
    divider: ColorSpec,
    caret: ColorSpec,
    hl_line: ColorSpec,
    selection: ColorSpec,
    bracket_match: ColorSpec,
    search_match: ColorSpec,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TerminalColors {
    ansi: Vec<ColorSpec>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BracketColors {
    colors: Vec<ColorSpec>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Text {
    fg: ColorSpec,
    #[serde(default)]
    fg_modeline: Option<ColorSpec>,
    #[serde(default)]
    caret_text: Option<ColorSpec>,
    gutter_fg: ColorSpec,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Modes {
    normal: ColorSpec,
    insert: ColorSpec,
    visual: ColorSpec,
    replace: ColorSpec,
    command: ColorSpec,
    explorer: ColorSpec,
    picker: ColorSpec,
    text_dark: ColorSpec,
    #[serde(default)]
    text_light: Option<ColorSpec>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Syntax {
    keyword: ColorSpec,
    string: ColorSpec,
    comment: ColorSpec,
    function: ColorSpec,
    #[serde(rename = "type")]
    ty: ColorSpec,
    number: ColorSpec,
    constant: ColorSpec,
    variable: ColorSpec,
    operator: ColorSpec,
    punctuation: ColorSpec,
    attribute: ColorSpec,
    #[serde(default)]
    interface: Option<ColorSpec>,
    #[serde(default)]
    parameter: Option<ColorSpec>,
    #[serde(default, rename = "macro")]
    macro_: Option<ColorSpec>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Explorer {
    folder: ColorSpec,
    file: ColorSpec,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Git {
    modified: ColorSpec,
    staged: ColorSpec,
    untracked: ColorSpec,
    ignored: ColorSpec,
    conflicted: ColorSpec,
}

impl ColorSpec {
    /// Fenix's `ColorSpec::rgba`: the alpha, or what's wrong.
    fn alpha(&self, key: &str) -> Result<f32, String> {
        let (hex, alpha) = match self {
            ColorSpec::Hex(hex) => (hex.as_str(), None),
            ColorSpec::Alpha { color, alpha } => (color.as_str(), Some(*alpha)),
        };
        let digits = hex.strip_prefix('#').filter(|d| (d.len() == 6 || d.len() == 8) && d.chars().all(|c| c.is_ascii_hexdigit()));
        let Some(digits) = digits else { return Err(format!("{key}: {hex} isn't a color -- write #rrggbb")) };
        match (alpha, digits.len()) {
            (Some(a), 6) if (0.0..=1.0).contains(&a) => Ok(a),
            (Some(_), 6) => Err(format!("{key}: alpha is from 0 to 1")),
            (Some(_), _) => Err(format!("{key}: an alpha of its own, or #rrggbbaa -- not both")),
            (None, 8) => Ok(u8::from_str_radix(&digits[6..8], 16).expect("checked hex") as f32 / 255.0),
            (None, _) => Ok(1.0),
        }
    }

    fn fill(&self, key: &str) -> Result<(), String> {
        self.alpha(key).map(|_| ())
    }

    /// Fenix's `ColorSpec::text`: opaque.
    fn text(&self, key: &str) -> Result<(), String> {
        if self.alpha(key)? < 1.0 {
            return Err(format!("{key}: text can't be see-through -- write #rrggbb"));
        }
        Ok(())
    }
}

/// Whether `text` is a theme Fenix reads, as `theme::parse` does --
/// and, given the oldest Fenix it's meant for, whether that one reads it.
pub fn check(text: &str, floor: Option<&semver::Version>) -> Result<(), String> {
    let f: ThemeFile = toml::from_str(text).map_err(|e| e.message().to_string())?;
    if f.name.trim().is_empty() {
        return Err("name: a theme needs one".into());
    }
    match f.tabs.as_deref().unwrap_or("underline") {
        "underline" | "block" => {}
        other => return Err(format!("tabs: {other} -- underline or block")),
    }
    let s = &f.surfaces;
    for (key, spec) in [("surfaces.bg", &s.bg), ("surfaces.caret", &s.caret), ("surfaces.bg_modeline", &s.bg_modeline), ("surfaces.divider", &s.divider), ("surfaces.hl_line", &s.hl_line), ("surfaces.selection", &s.selection), ("surfaces.bracket_match", &s.bracket_match), ("surfaces.search_match", &s.search_match)] {
        spec.fill(key)?;
    }
    for (key, spec) in [("surfaces.sidebar_bg", &s.sidebar_bg), ("surfaces.tray", &s.tray), ("surfaces.card", &s.card), ("surfaces.input", &s.input), ("border", &f.border)] {
        if let Some(spec) = spec {
            spec.fill(key)?;
        }
    }
    let m = &f.modes;
    for (key, spec) in [("modes.normal", &m.normal), ("modes.insert", &m.insert), ("modes.visual", &m.visual), ("modes.replace", &m.replace), ("modes.command", &m.command), ("modes.explorer", &m.explorer), ("modes.picker", &m.picker)] {
        spec.fill(key)?;
    }
    let (t, x, e, g) = (&f.text, &f.syntax, &f.explorer, &f.git);
    let texts = [
        ("text.fg", &t.fg), ("text.gutter_fg", &t.gutter_fg), ("modes.text_dark", &m.text_dark),
        ("syntax.keyword", &x.keyword), ("syntax.string", &x.string), ("syntax.comment", &x.comment), ("syntax.function", &x.function), ("syntax.type", &x.ty), ("syntax.number", &x.number), ("syntax.constant", &x.constant), ("syntax.variable", &x.variable), ("syntax.operator", &x.operator), ("syntax.punctuation", &x.punctuation), ("syntax.attribute", &x.attribute),
        ("explorer.folder", &e.folder), ("explorer.file", &e.file),
        ("git.modified", &g.modified), ("git.staged", &g.staged), ("git.untracked", &g.untracked), ("git.ignored", &g.ignored), ("git.conflicted", &g.conflicted),
    ];
    for (key, spec) in texts {
        spec.text(key)?;
    }
    for (key, spec) in [("text.fg_modeline", &t.fg_modeline), ("text.caret_text", &t.caret_text), ("modes.text_light", &m.text_light), ("syntax.interface", &x.interface), ("syntax.parameter", &x.parameter), ("syntax.macro", &x.macro_)] {
        if let Some(spec) = spec {
            spec.text(key)?;
        }
    }
    if let Some(terminal) = &f.terminal {
        if terminal.ansi.len() != 16 {
            return Err(format!("terminal.ansi: 16 colours -- black, red, green, yellow, blue, magenta, cyan, white, then the bright ones -- not {}", terminal.ansi.len()));
        }
        for (i, c) in terminal.ansi.iter().enumerate() {
            c.text(&format!("terminal.ansi[{i}]"))?;
        }
    }
    if let Some(brackets) = &f.brackets {
        if brackets.colors.is_empty() || brackets.colors.len() > 6 {
            return Err(format!("brackets.colors: one to six colours, the outermost first, not {}", brackets.colors.len()));
        }
        for (i, c) in brackets.colors.iter().enumerate() {
            c.text(&format!("brackets.colors[{i}]"))?;
        }
    }
    if let Some(floor) = floor {
        let table: toml::Table = toml::from_str(text).map_err(|e| e.message().to_string())?;
        for (key, (major, minor, patch)) in INTRODUCED {
            let since = semver::Version::new(*major, *minor, *patch);
            let present = match key.split_once('.') {
                Some((section, field)) => table.get(section).and_then(|s| s.get(field)).is_some(),
                None => table.contains_key(*key),
            };
            if present && *floor < since {
                return Err(format!("{key}: Fenix reads it from {since}, and this is for Fenix {floor} and later -- an older one would refuse the whole theme (raise engine.fenix, or leave it out)"));
            }
        }
    }
    Ok(())
}

/// The oldest Fenix a range admits (`>=1.0, <2` is 1.0.0): the highest
/// lower bound its comparators set, or 0.0.0.
pub fn floor(req: &semver::VersionReq) -> semver::Version {
    use semver::Op;
    req.comparators
        .iter()
        .filter(|c| matches!(c.op, Op::Exact | Op::Greater | Op::GreaterEq | Op::Tilde | Op::Caret | Op::Wildcard))
        .map(|c| semver::Version::new(c.major, c.minor.unwrap_or(0), c.patch.unwrap_or(0)))
        .max()
        .unwrap_or(semver::Version::new(0, 0, 0))
}

/// A theme's name as Fenix's settings importer compares them
/// (`fenix-config`'s `import::norm`): lower case, letters and digits only,
/// accents off -- `Rosé Pine` is `rosepine`.
#[cfg(test)]
pub fn norm(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            'é' | 'è' | 'ê' | 'É' => 'e',
            'á' | 'à' | 'â' => 'a',
            c => c,
        })
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// What Fenix's settings importer makes of VS Code's
/// `"workbench.colorTheme": name` given these themes (`import::theme_for`,
/// as far as an exact name and a name it begins go): the theme it picks,
/// and whether it's that theme itself (`true`) or only the nearest.
#[cfg(test)]
pub fn import_match(name: &str, themes: &[&str]) -> Option<(String, bool)> {
    let n = norm(name);
    if let Some(t) = themes.iter().find(|t| norm(t) == n) {
        return Some((t.to_string(), true));
    }
    let mut within: Vec<&&str> = themes.iter().filter(|t| n.starts_with(&norm(t))).collect();
    within.sort_by_key(|t| std::cmp::Reverse(norm(t).len()));
    within.first().map(|t| (t.to_string(), false))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// One of Fenix's own theme files, as it ships in 1.5.2: every key it
    /// has today.
    pub const FULL: &str = r##"name = "Sample"
tabs = "block"
[surfaces]
bg = "#1a1b26"
sidebar_bg = "#16161e"
tray = "#000000"
card = "#101010"
input = { color = "#ffffff", alpha = 0.1 }
bg_modeline = "#16161e"
divider = "#2a2b3d"
caret = "#e0af68"
hl_line = "#22253480"
selection = { color = "#7aa2f7", alpha = 0.25 }
bracket_match = { color = "#9ece6a", alpha = 0.2 }
search_match = { color = "#e0af68", alpha = 0.25 }
[terminal]
ansi = ["#000000", "#ff0000", "#00ff00", "#ffff00", "#0000ff", "#ff00ff", "#00ffff", "#ffffff", "#000000", "#ff0000", "#00ff00", "#ffff00", "#0000ff", "#ff00ff", "#00ffff", "#ffffff"]
[brackets]
colors = ["#ff0000", "#00ff00"]
[text]
fg = "#c0caf5"
gutter_fg = "#737aa2"
[modes]
normal = "#7aa2f7"
insert = "#9ece6a"
visual = "#bb9af7"
replace = "#e0af68"
command = "#7dcfff"
explorer = "#7aa2f7"
picker = "#bb9af7"
text_dark = "#1a1b26"
[syntax]
keyword = "#bb9af7"
string = "#9ece6a"
comment = "#737aa2"
function = "#7aa2f7"
type = "#2ac3de"
number = "#ff9e64"
constant = "#ff9e64"
variable = "#c0caf5"
operator = "#89ddff"
punctuation = "#a9b1d6"
attribute = "#e0af68"
interface = "#2ac3de"
parameter = "#e0af68"
macro = "#7dcfff"
[explorer]
folder = "#7aa2f7"
file = "#c0caf5"
[git]
modified = "#e0af68"
staged = "#9ece6a"
untracked = "#7aa2f7"
ignored = "#737aa2"
conflicted = "#f7768e"
"##;

    #[test]
    fn fenix_s_rules_are_held_to() {
        check(FULL, None).unwrap();
        for (from, to, says) in [
            ("keyword = \"#bb9af7\"", "keyword = \"purple\"", "syntax.keyword: purple isn't a color -- write #rrggbb"),
            ("keyword = \"#bb9af7\"", "keyword = { color = \"#bb9af7\", alpha = 0.5 }", "syntax.keyword: text can't be see-through"),
            ("keyword = \"#bb9af7\"", "keyword = \"#bb9af780\"", "syntax.keyword: text can't be see-through"),
            ("tabs = \"block\"", "tabs = \"round\"", "underline or block"),
            ("[git]", "[gits]", ""),
            ("caret = \"#e0af68\"", "caret = \"#e0af68\"\nglow = \"#ffffff\"", "glow"),
            ("hl_line = \"#22253480\"", "hl_line = { color = \"#22253480\", alpha = 0.5 }", "not both"),
            ("alpha = 0.25 }\nbracket", "alpha = 1.5 }\nbracket", "alpha is from 0 to 1"),
            ("colors = [\"#ff0000\", \"#00ff00\"]", "colors = []", "one to six"),
            ("ansi = [\"#000000\", ", "ansi = [", "16 colours"),
            ("name = \"Sample\"", "name = \" \"", "a theme needs one"),
            ("[modes]\nnormal = \"#7aa2f7\"\n", "[modes]\n", "normal"),
        ] {
            assert!(FULL.contains(from), "{from}");
            let err = check(&FULL.replacen(from, to, 1), None).unwrap_err();
            assert!(err.contains(says), "{to}: {err}");
        }
    }

    #[test]
    fn a_key_newer_than_the_fenix_it_is_for_is_refused() {
        let v = |s: &str| semver::Version::parse(s).unwrap();
        check(FULL, Some(&v("1.5.2"))).unwrap();
        assert!(check(FULL, Some(&v("1.5.0"))).unwrap_err().starts_with("syntax.interface: Fenix reads it from 1.5.2"));
        let older: String = FULL.lines().filter(|l| !["interface", "parameter", "macro"].iter().any(|k| l.starts_with(k))).map(|l| format!("{l}\n")).collect();
        check(&older, Some(&v("1.5.0"))).unwrap();
        assert!(check(&older, Some(&v("1.4.0"))).unwrap_err().starts_with("brackets"));
        assert!(check(&older, Some(&v("1.0.0"))).unwrap_err().contains("1.4.0"));
        assert_eq!(floor(&semver::VersionReq::parse(">=1.0, <2").unwrap()), v("1.0.0"));
        assert_eq!(floor(&semver::VersionReq::parse(">=1.5.2, <2").unwrap()), v("1.5.2"));
        assert_eq!(floor(&semver::VersionReq::parse("^1.4").unwrap()), v("1.4.0"));
    }

    /// Every theme Fenix ships reads here as it does there, and has the
    /// name `BUILT_IN_NAMES` gives it: run against a checkout of Fenix,
    /// `FENIX_SOURCE=../fenix cargo test -p xtask -- --ignored`, when
    /// Fenix's theme format or its themes change.
    #[test]
    #[ignore = "needs a checkout of Fenix: FENIX_SOURCE"]
    fn fenix_s_own_themes_read_here() {
        let source = std::path::PathBuf::from(std::env::var("FENIX_SOURCE").expect("FENIX_SOURCE: a checkout of Fenix"));
        let dir = source.join("crates/fenix-gui/themes");
        let mut names = Vec::new();
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let text = std::fs::read_to_string(entry.path()).unwrap();
            check(&text, None).unwrap_or_else(|e| panic!("{}: {e}", entry.path().display()));
            names.push(toml::from_str::<toml::Table>(&text).unwrap()["name"].as_str().unwrap().to_string());
        }
        names.sort();
        let mut ours: Vec<String> = BUILT_IN_NAMES.iter().map(|n| n.to_string()).collect();
        ours.sort();
        assert_eq!(names, ours);
    }

    #[test]
    fn the_settings_importer_s_match_is_mirrored() {
        assert_eq!(norm("Rosé Pine Dawn"), "rosepinedawn");
        let themes = ["One Dark", "Vitesse Dark", "Vitesse Dark Soft"];
        assert_eq!(import_match("Vitesse Dark Soft", &themes), Some(("Vitesse Dark Soft".into(), true)));
        assert_eq!(import_match("vitesse-dark", &themes), Some(("Vitesse Dark".into(), true)));
        assert_eq!(import_match("One Dark Pro", &themes), Some(("One Dark".into(), false)));
        assert_eq!(import_match("Abyss", &themes), None);
    }
}
