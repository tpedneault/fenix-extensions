//! Colour literals in text: found on a line, read, and written again in
//! the way they were written -- `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`,
//! `rgb()` and `rgba()`, `hsl()` and `hsla()`, `oklch()`, CSS's named
//! colours, and a Fenix theme's `{ color = "#rrggbb", alpha = 0.12 }`.

use crate::colour::Colour;
use crate::names;

/// How a literal's alpha was written, if it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alpha {
    None,
    /// `0.5`
    Number,
    /// `50%`
    Percent,
}

/// How a hue was written: a number (degrees), or with a unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hue {
    Bare,
    Deg,
    Rad,
    Grad,
    Turn,
}

impl Hue {
    fn from_unit(unit: &str) -> Option<Hue> {
        Some(match unit {
            "" => Hue::Bare,
            "deg" => Hue::Deg,
            "rad" => Hue::Rad,
            "grad" => Hue::Grad,
            "turn" => Hue::Turn,
            _ => return None,
        })
    }

    fn to_degrees(self, v: f64) -> f64 {
        match self {
            Hue::Bare | Hue::Deg => v,
            Hue::Rad => v.to_degrees(),
            Hue::Grad => v * 0.9,
            Hue::Turn => v * 360.0,
        }
    }

    /// `degrees` in this unit, to `places` decimals of a degree.
    fn write(self, degrees: f64, places: usize) -> String {
        match self {
            Hue::Bare => num(degrees, places),
            Hue::Deg => format!("{}deg", num(degrees, places)),
            Hue::Rad => format!("{}rad", num(degrees.to_radians(), places + 2)),
            Hue::Grad => format!("{}grad", num(degrees / 0.9, places)),
            Hue::Turn => format!("{}turn", num(degrees / 360.0, places + 3)),
        }
    }
}

/// How a literal is written, so a colour can be written the same way.
#[derive(Debug, Clone, PartialEq)]
pub enum Format {
    /// 3, 4, 6 or 8 digits, in capitals or not.
    Hex { digits: u8, upper: bool },
    /// `rgb(…)`: its name as written (`rgba`, `RGB`), commas between its
    /// numbers (CSS's legacy syntax) or not, channels as percentages.
    Rgb { name: String, legacy: bool, percent: bool, alpha: Alpha },
    Hsl { name: String, legacy: bool, hue: Hue, alpha: Alpha },
    /// `oklch(L C H)`: its lightness and its chroma as percentages or not.
    Oklch { name: String, l_percent: bool, c_percent: bool, hue: Hue, alpha: Alpha },
    /// A CSS name, as written.
    Named(String),
    /// A Fenix theme's colour with an alpha of its own:
    /// `{ color = "#rrggbb", alpha = 0.12 }`.
    Theme,
}

/// What a colour can be converted to (`colours.convert`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Hex,
    Rgb,
    Hsl,
    Oklch,
}

impl Target {
    pub const ALL: [Target; 4] = [Target::Hex, Target::Rgb, Target::Hsl, Target::Oklch];

    pub fn name(self) -> &'static str {
        match self {
            Target::Hex => "hex",
            Target::Rgb => "rgb",
            Target::Hsl => "hsl",
            Target::Oklch => "oklch",
        }
    }

    pub fn from_name(name: &str) -> Option<Target> {
        Target::ALL.into_iter().find(|t| t.name() == name)
    }

    /// The format a literal written `from` way becomes: CSS's legacy
    /// syntax kept when it had it (or `legacy`), its alpha's way, its
    /// capitals for hex.
    pub fn format(self, from: &Format, legacy: bool) -> Format {
        let (was_legacy, alpha) = match from {
            Format::Rgb { legacy, alpha, .. } | Format::Hsl { legacy, alpha, .. } => (*legacy, *alpha),
            Format::Oklch { alpha, .. } => (false, *alpha),
            _ => (legacy, Alpha::None),
        };
        let legacy = was_legacy || legacy;
        match self {
            Target::Hex => Format::Hex { digits: 6, upper: matches!(from, Format::Hex { upper: true, .. }) },
            Target::Rgb => Format::Rgb { name: "rgb".into(), legacy, percent: false, alpha },
            Target::Hsl => Format::Hsl { name: "hsl".into(), legacy, hue: Hue::Bare, alpha },
            Target::Oklch => Format::Oklch { name: "oklch".into(), l_percent: true, c_percent: false, hue: Hue::Bare, alpha: if alpha == Alpha::None { Alpha::None } else { Alpha::Number } },
        }
    }
}

/// A colour literal on a line: its characters `start..end`, its colour,
/// and how it's written.
#[derive(Debug, Clone, PartialEq)]
pub struct Literal {
    pub start: usize,
    pub end: usize,
    pub colour: Colour,
    pub format: Format,
}

/// What to look for, by the kind of file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Scan {
    /// CSS's names (`red`, `rebeccapurple`), and `#123` (all digits,
    /// three or four of them), which elsewhere is an issue's number:
    /// stylesheets and markup.
    pub style: bool,
    /// A Fenix theme's `{ color = "#rrggbb", alpha = 0.12 }`: TOML.
    pub theme: bool,
}

impl Scan {
    /// For a file named `path`.
    pub fn for_path(path: &str) -> Scan {
        let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
        let style = matches!(ext.as_str(), "css" | "scss" | "sass" | "less" | "styl" | "stylus" | "pcss" | "postcss" | "html" | "htm" | "xhtml" | "vue" | "svelte" | "astro" | "svg");
        Scan { style, theme: ext == "toml" }
    }
}

/// `v` with at most `places` decimals, the trailing zeros dropped.
pub fn num(v: f64, places: usize) -> String {
    let s = format!("{:.*}", places, v);
    let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s };
    if s == "-0" {
        "0".into()
    } else {
        s
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Every colour literal on `line`, left to right.
pub fn find(line: &str, scan: Scan) -> Vec<Literal> {
    let chars: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let prev = i.checked_sub(1).map(|p| chars[p]);
        if scan.theme && c == '{' {
            if let Some(lit) = theme_table(&chars, i) {
                i = lit.end;
                out.push(lit);
                continue;
            }
        }
        if c == '#' && !prev.is_some_and(|p| is_word(p) || p == '&' || p == '#') {
            if let Some(lit) = hex_at(&chars, i, scan.style) {
                i = lit.end;
                out.push(lit);
                continue;
            }
            i += 1;
            continue;
        }
        if c.is_ascii_alphabetic() && !prev.is_some_and(|p| is_word(p) || matches!(p, '-' | '.' | '$' | '@' | '#' | '%' | '\\')) {
            let mut end = i;
            while end < chars.len() && (is_word(chars[end]) || chars[end] == '-') {
                end += 1;
            }
            let word: String = chars[i..end].iter().collect();
            let lower = word.to_ascii_lowercase();
            if chars.get(end) == Some(&'(') {
                if let Some(lit) = function(&chars, i, end, &word, &lower) {
                    i = lit.end;
                    out.push(lit);
                    continue;
                }
            } else if scan.style && chars[..i].iter().any(|&p| p == ':' || p == '=') {
                if let Some(rgb) = names::lookup(&lower) {
                    out.push(Literal { start: i, end, colour: rgb, format: Format::Named(word) });
                }
            }
            i = end;
            continue;
        }
        i += 1;
    }
    out
}

/// A `#` and its digits, at `at`.
fn hex_at(chars: &[char], at: usize, style: bool) -> Option<Literal> {
    let mut end = at + 1;
    while end < chars.len() && chars[end].is_ascii_hexdigit() {
        end += 1;
    }
    let digits: String = chars[at + 1..end].iter().collect();
    if chars.get(end).is_some_and(|&c| is_word(c) || c == '-') {
        return None;
    }
    if digits.len() <= 4 && !style && digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let colour = parse_hex(&digits)?;
    let upper = digits.chars().any(|c| c.is_ascii_uppercase());
    Some(Literal { start: at, end, colour, format: Format::Hex { digits: digits.len() as u8, upper } })
}

/// `rgb`, `#rrggbb`'s digits (3, 4, 6 or 8) as a colour.
pub fn parse_hex(digits: &str) -> Option<Colour> {
    if !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let nibble = |i: usize| u8::from_str_radix(&digits[i..i + 1], 16).ok().map(|v| v * 17);
    let byte = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).ok();
    let [r, g, b, a] = match digits.len() {
        3 => [nibble(0)?, nibble(1)?, nibble(2)?, 255],
        4 => [nibble(0)?, nibble(1)?, nibble(2)?, nibble(3)?],
        6 => [byte(0)?, byte(2)?, byte(4)?, 255],
        8 => [byte(0)?, byte(2)?, byte(4)?, byte(6)?],
        _ => return None,
    };
    Some(Colour::from_u8(r, g, b).with_alpha(f64::from(a) / 255.0))
}

/// A number and its unit (`50%`, `120deg`), or `none` (0).
#[derive(Debug, Clone, PartialEq)]
struct Arg {
    value: f64,
    unit: String,
}

/// `name(` … `)` from `start`, its arguments after `open` (the `(`):
/// three, and an alpha, by commas or by spaces and a `/`.
fn function(chars: &[char], start: usize, open: usize, name: &str, lower: &str) -> Option<Literal> {
    let kind = match lower {
        "rgb" | "rgba" => 0,
        "hsl" | "hsla" => 1,
        "oklch" => 2,
        _ => return None,
    };
    let close = (open + 1..chars.len()).find(|&i| chars[i] == ')')?;
    let inner: String = chars[open + 1..close].iter().collect();
    let (args, legacy, slash) = args(&inner)?;
    if args.len() != 3 && args.len() != 4 {
        return None;
    }
    // Legacy syntax has its alpha after a comma; modern after a `/`.
    if !legacy && args.len() == 4 && !slash {
        return None;
    }
    let alpha = match args.get(3) {
        None => Alpha::None,
        Some(a) if a.unit == "%" => Alpha::Percent,
        Some(a) if a.unit.is_empty() => Alpha::Number,
        Some(_) => return None,
    };
    let a = args.get(3).map_or(1.0, |a| if alpha == Alpha::Percent { a.value / 100.0 } else { a.value }).clamp(0.0, 1.0);
    let (colour, format) = match kind {
        0 => {
            let percent = args[0].unit == "%";
            let mut ch = [0.0; 3];
            for (k, arg) in args[..3].iter().enumerate() {
                ch[k] = match arg.unit.as_str() {
                    "%" => arg.value / 100.0,
                    "" => arg.value / 255.0,
                    _ => return None,
                };
                if legacy && (arg.unit == "%") != percent {
                    return None;
                }
            }
            (Colour::new(ch[0], ch[1], ch[2], a).clip(), Format::Rgb { name: name.into(), legacy, percent, alpha })
        }
        1 => {
            let hue = Hue::from_unit(&args[0].unit)?;
            let pct = |arg: &Arg| match arg.unit.as_str() {
                "%" => Some(arg.value / 100.0),
                "" if !legacy => Some(arg.value / 100.0),
                _ => None,
            };
            let colour = Colour::from_hsl(hue.to_degrees(args[0].value), pct(&args[1])?, pct(&args[2])?, a);
            (colour, Format::Hsl { name: name.into(), legacy, hue, alpha })
        }
        _ => {
            if legacy {
                return None;
            }
            let l_percent = args[0].unit == "%";
            let c_percent = args[1].unit == "%";
            let l = match args[0].unit.as_str() {
                "%" => args[0].value / 100.0,
                "" => args[0].value,
                _ => return None,
            };
            let c = match args[1].unit.as_str() {
                "%" => args[1].value / 100.0 * 0.4,
                "" => args[1].value,
                _ => return None,
            };
            let hue = Hue::from_unit(&args[2].unit)?;
            let colour = Colour::from_oklch(l, c, hue.to_degrees(args[2].value), a);
            (colour, Format::Oklch { name: name.into(), l_percent, c_percent, hue, alpha })
        }
    };
    Some(Literal { start, end: close + 1, colour, format })
}

/// A function's arguments: each a number and its unit; whether they're
/// separated by commas; whether a `/` came before the last.
fn args(inner: &str) -> Option<(Vec<Arg>, bool, bool)> {
    let legacy = inner.contains(',');
    let slash = inner.contains('/');
    if legacy && slash {
        return None;
    }
    let parts: Vec<&str> = if legacy {
        inner.split(',').map(str::trim).collect()
    } else {
        let (main, alpha) = match inner.split_once('/') {
            Some((m, a)) => (m, Some(a.trim())),
            None => (inner, None),
        };
        let mut parts: Vec<&str> = main.split_whitespace().collect();
        if let Some(a) = alpha {
            if parts.len() != 3 || a.split_whitespace().count() != 1 {
                return None;
            }
            parts.push(a);
        }
        parts
    };
    let args = parts.iter().map(|p| arg(p)).collect::<Option<Vec<_>>>()?;
    Some((args, legacy, slash))
}

fn arg(text: &str) -> Option<Arg> {
    if text.eq_ignore_ascii_case("none") {
        return Some(Arg { value: 0.0, unit: String::new() });
    }
    let split = text.find(|c: char| c.is_ascii_alphabetic() || c == '%').unwrap_or(text.len());
    let (n, unit) = text.split_at(split);
    if n.is_empty() || !n.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e')) {
        return None;
    }
    let value: f64 = n.parse().ok()?;
    value.is_finite().then(|| Arg { value, unit: unit.to_ascii_lowercase() })
}

/// `{ color = "#rrggbb", alpha = 0.12 }` at `at`.
fn theme_table(chars: &[char], at: usize) -> Option<Literal> {
    let mut i = at + 1;
    let ws = |i: &mut usize| {
        while *i < chars.len() && chars[*i].is_whitespace() {
            *i += 1;
        }
    };
    let word = |i: &mut usize, w: &str| {
        let ok = chars.get(*i..*i + w.len()).is_some_and(|s| s.iter().copied().eq(w.chars()));
        if ok {
            *i += w.len();
        }
        ok
    };
    ws(&mut i);
    if !word(&mut i, "color") {
        return None;
    }
    ws(&mut i);
    if !word(&mut i, "=") {
        return None;
    }
    ws(&mut i);
    if !word(&mut i, "\"#") {
        return None;
    }
    let from = i;
    while i < chars.len() && chars[i].is_ascii_hexdigit() {
        i += 1;
    }
    let digits: String = chars[from..i].iter().collect();
    if digits.len() != 6 || !word(&mut i, "\"") {
        return None;
    }
    ws(&mut i);
    if !word(&mut i, ",") {
        return None;
    }
    ws(&mut i);
    if !word(&mut i, "alpha") {
        return None;
    }
    ws(&mut i);
    if !word(&mut i, "=") {
        return None;
    }
    ws(&mut i);
    let from = i;
    while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
        i += 1;
    }
    let alpha: f64 = chars[from..i].iter().collect::<String>().parse().ok()?;
    ws(&mut i);
    if !word(&mut i, "}") || !(0.0..=1.0).contains(&alpha) {
        return None;
    }
    let colour = parse_hex(&digits)?.with_alpha(alpha);
    Some(Literal { start: at, end: i, colour, format: Format::Theme })
}

impl Format {
    /// `colour` written this way. A colour out of sRGB's gamut is mapped
    /// in first (CSS Color 4), but for OKLCH, which holds it as it is. A
    /// named colour that's no name any more is written `#rrggbb`.
    pub fn write(&self, colour: &Colour) -> String {
        // As few decimals as read back as the same bytes: `62.8%` where
        // that's enough, `62.796%` where it isn't.
        let mapped = colour.gamut_map();
        let mut text = self.write_with(colour, 0);
        if matches!(self, Format::Rgb { .. } | Format::Hsl { .. } | Format::Oklch { .. }) {
            for extra in 1..=4 {
                let back = find(&text, Scan { style: true, theme: false });
                if back.first().is_some_and(|l| l.colour.gamut_map().same_bytes(&mapped)) {
                    break;
                }
                text = self.write_with(colour, extra);
            }
        }
        text
    }

    /// `colour` written with `extra` decimals more than it's written with
    /// at least.
    fn write_with(&self, colour: &Colour, extra: usize) -> String {
        let mapped = colour.gamut_map();
        let a = colour.a.clamp(0.0, 1.0);
        let opaque = mapped.bytes()[3] == 255;
        let alpha_text = |alpha: Alpha| match alpha {
            Alpha::Percent => format!("{}%", num(a * 100.0, 1)),
            _ => num(a, 3),
        };
        // Its alpha, when it had one or it's see-through now.
        let alpha_of = |alpha: Alpha| (alpha != Alpha::None || !opaque).then(|| alpha_text(if alpha == Alpha::None { Alpha::Number } else { alpha }));
        match self {
            Format::Hex { digits, upper } => {
                let [r, g, b, al] = mapped.bytes();
                let short = [r, g, b, al].iter().all(|v| v % 17 == 0);
                let text = match (digits, short, opaque) {
                    (3, true, true) => format!("{:x}{:x}{:x}", r / 17, g / 17, b / 17),
                    (3 | 4, true, _) => format!("{:x}{:x}{:x}{:x}", r / 17, g / 17, b / 17, al / 17),
                    (3 | 6, _, true) => format!("{r:02x}{g:02x}{b:02x}"),
                    _ => format!("{r:02x}{g:02x}{b:02x}{al:02x}"),
                };
                format!("#{}", if *upper { text.to_ascii_uppercase() } else { text })
            }
            Format::Rgb { name, legacy, percent, alpha } => {
                let ch = |v: f64| if *percent { format!("{}%", num(v * 100.0, 1 + extra)) } else { num((v * 255.0).round(), 0) };
                let (r, g, b) = (ch(mapped.clip().r), ch(mapped.clip().g), ch(mapped.clip().b));
                function_text(name, *legacy, [r, g, b], alpha_of(*alpha))
            }
            Format::Hsl { name, legacy, hue, alpha } => {
                let (h, s, l) = mapped.hsl();
                let parts = [hue.write(h, 1 + extra), format!("{}%", num(s * 100.0, 1 + extra)), format!("{}%", num(l * 100.0, 1 + extra))];
                function_text(name, *legacy, parts, alpha_of(*alpha))
            }
            Format::Oklch { name, l_percent, c_percent, hue, alpha } => {
                let [l, c, h] = colour.oklch();
                let l = if *l_percent { format!("{}%", num(l * 100.0, 2 + extra)) } else { num(l, 4 + extra) };
                let c = if *c_percent { format!("{}%", num(c / 0.4 * 100.0, 2 + extra)) } else { num(c, 4 + extra) };
                let alpha = (*alpha != Alpha::None || !opaque).then(|| alpha_text(*alpha));
                let mut s = format!("{name}({l} {c} {})", hue.write(h, 2 + extra));
                if let Some(a) = alpha {
                    s.insert_str(s.len() - 1, &format!(" / {a}"));
                }
                s
            }
            Format::Named(was) => {
                if opaque {
                    if names::lookup(&was.to_ascii_lowercase()).is_some_and(|c| c.same_bytes(&mapped)) {
                        return was.clone();
                    }
                    if let Some(name) = names::name_of(&mapped) {
                        return if was.chars().any(|c| c.is_ascii_uppercase()) && was.chars().all(|c| !c.is_ascii_lowercase()) { name.to_ascii_uppercase() } else { name.to_string() };
                    }
                }
                Format::Hex { digits: 6, upper: false }.write(colour)
            }
            Format::Theme => {
                let [r, g, b, _] = mapped.bytes();
                format!("{{ color = \"#{r:02x}{g:02x}{b:02x}\", alpha = {} }}", num(a, 3))
            }
        }
    }

    /// What it's called, for the convert picker and the page: `hex`,
    /// `rgb`, `hsl`, `oklch`, `name`, `theme`.
    pub fn kind(&self) -> &'static str {
        match self {
            Format::Hex { .. } => "hex",
            Format::Rgb { .. } => "rgb",
            Format::Hsl { .. } => "hsl",
            Format::Oklch { .. } => "oklch",
            Format::Named(_) => "name",
            Format::Theme => "theme",
        }
    }
}

/// `name(a, b, c)` or `name(a b c)`, its alpha after.
fn function_text(name: &str, legacy: bool, parts: [String; 3], alpha: Option<String>) -> String {
    if legacy {
        // `rgb` takes four in legacy syntax only as `rgba`, to old eyes.
        let name = match &alpha {
            Some(_) if name.eq_ignore_ascii_case("rgb") || name.eq_ignore_ascii_case("hsl") => {
                let a = if name.chars().all(|c| c.is_ascii_uppercase()) { "A" } else { "a" };
                format!("{name}{a}")
            }
            _ => name.to_string(),
        };
        match alpha {
            Some(a) => format!("{name}({}, {}, {}, {a})", parts[0], parts[1], parts[2]),
            None => format!("{name}({}, {}, {})", parts[0], parts[1], parts[2]),
        }
    } else {
        match alpha {
            Some(a) => format!("{name}({} {} {} / {a})", parts[0], parts[1], parts[2]),
            None => format!("{name}({} {} {})", parts[0], parts[1], parts[2]),
        }
    }
}

/// The literal under column `col`, or else the nearest on the line.
pub fn at_column(literals: &[Literal], col: usize) -> Option<&Literal> {
    literals.iter().find(|l| (l.start..=l.end).contains(&col)).or_else(|| literals.iter().min_by_key(|l| if col < l.start { l.start - col } else { col - l.end }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CSS: Scan = Scan { style: true, theme: false };
    const CODE: Scan = Scan { style: false, theme: false };
    const TOML: Scan = Scan { style: false, theme: true };

    fn one(line: &str, scan: Scan) -> Literal {
        let found = find(line, scan);
        assert_eq!(found.len(), 1, "{line}: {found:?}");
        found.into_iter().next().unwrap()
    }

    fn texts(line: &str, scan: Scan) -> Vec<String> {
        let chars: Vec<char> = line.chars().collect();
        find(line, scan).iter().map(|l| chars[l.start..l.end].iter().collect()).collect()
    }

    #[test]
    fn hex_in_every_length() {
        assert_eq!(texts("a: #fff; b: #ffff; c: #3B82F6; d: #3b82f680;", CODE), ["#fff", "#ffff", "#3B82F6", "#3b82f680"]);
        let l = one("color: #3B82F6;", CODE);
        assert_eq!((l.start, l.end, l.colour.rgb()), (7, 14, (59, 130, 246)));
        assert_eq!(l.format, Format::Hex { digits: 6, upper: true });
        assert_eq!(one("x = '#3b82f680'", CODE).colour.bytes(), [59, 130, 246, 128]);
        assert_eq!(one("#f008", CODE).colour.bytes(), [255, 0, 0, 136]);
        // Not colours.
        assert!(texts("#12345 #abcdefg a#fff &#123; #define #[derive] ##fff", CODE).is_empty());
        assert!(texts("Part of #27, fixes #123 and #1234", CODE).is_empty(), "issue numbers");
        assert_eq!(texts("color: #123", CSS), ["#123"], "but in a stylesheet");
        assert_eq!(texts("Fixed #123456", CODE), ["#123456"], "six digits is a colour");
        assert_eq!(texts("日本 #abc", CODE), ["#abc"]);
        assert_eq!(one("日本 #abc", CODE).start, 3, "in characters");
    }

    #[test]
    fn functions_old_and_new() {
        let rgb = one("color: rgb(59, 130, 246);", CODE);
        assert_eq!(rgb.colour.rgb(), (59, 130, 246));
        assert_eq!(rgb.format, Format::Rgb { name: "rgb".into(), legacy: true, percent: false, alpha: Alpha::None });
        assert_eq!(one("rgba(59,130,246,0.5)", CODE).colour.bytes()[3], 128);
        let modern = one("rgb(59 130 246 / 50%)", CODE);
        assert_eq!((modern.colour.bytes(), &modern.format), ([59, 130, 246, 128], &Format::Rgb { name: "rgb".into(), legacy: false, percent: false, alpha: Alpha::Percent }));
        assert_eq!(one("RGB(100%, 0%, 0%)", CODE).colour.rgb(), (255, 0, 0));
        assert_eq!(one("hsl(217.2 91.2% 59.8%)", CODE).colour.rgb(), (59, 130, 246));
        assert_eq!(one("hsla(120, 100%, 25%, .5)", CODE).colour.bytes(), [0, 128, 0, 128]);
        assert_eq!(one("hsl(0.5turn 100% 50%)", CODE).colour.rgb(), (0, 255, 255));
        assert_eq!(one("hsl(120deg 100 50)", CODE).colour.rgb(), (0, 255, 0), "modern takes bare numbers");
        let ok = one("oklch(62.8% 0.2577 29.23)", CODE);
        assert_eq!(ok.colour.rgb(), (255, 0, 0));
        assert_eq!(one("oklch(0.628 0.2577 29.23 / 0.5)", CODE).colour.bytes(), [255, 0, 0, 128]);
        assert_eq!(one("oklch(62.8% 64.4% 29.23)", CODE).colour.rgb(), (255, 0, 0), "chroma 100% is 0.4");
        assert_eq!(one("rgb(none 0 0)", CODE).colour.rgb(), (0, 0, 0));
        assert_eq!(one("rgb(300, 0, 0)", CODE).colour.rgb(), (255, 0, 0), "clipped, as CSS does");
        // Not colours.
        for bad in ["rgb(1, 2)", "rgb(1 2 3 4)", "rgb(1, 2, 3 / 4)", "rgb(1, 50%, 3)", "hsl(1, 2, 3)", "oklch(1, 2, 3)", "rgb(a b c)", "foorgb(1 2 3)", "rgb(1 2 3", "my-rgb(1 2 3)", "rgb(1px 2 3)"] {
            assert!(find(bad, CODE).is_empty(), "{bad}");
        }
        assert_eq!(texts("mix(rgb(0 0 0), hsl(0 0% 100%))", CODE), ["rgb(0 0 0)", "hsl(0 0% 100%)"]);
    }

    #[test]
    fn names_in_stylesheets() {
        assert_eq!(texts("  border: 1px solid RebeccaPurple; color: red;", CSS), ["RebeccaPurple", "red"]);
        assert_eq!(one("color: tan", CSS).colour.rgb(), (210, 180, 140));
        assert!(texts("white-space: nowrap; .red { } #red { } $red: 1; --red: 2; red()", CSS).is_empty());
        assert!(texts("let red = 1; color: red", CODE).is_empty(), "not in code");
        assert!(texts("red green blue", CSS).is_empty(), "a declaration's, after a colon");
        assert_eq!(texts(r#"<rect fill="navy"/>"#, CSS), ["navy"]);
    }

    #[test]
    fn a_themes_alpha_table() {
        let l = one(r##"selection = { color = "#7c7f93", alpha = 0.25 }"##, TOML);
        assert_eq!((l.start, l.end, l.format.clone()), (12, 47, Format::Theme));
        assert_eq!(l.colour.bytes(), [124, 127, 147, 64]);
        assert_eq!(texts(r##"bg = "#eff1f5""##, TOML), ["#eff1f5"]);
        assert_eq!(texts(r##"x = {color="#7c7f93",alpha=1}"##, TOML).len(), 1);
        assert_eq!(texts(r##"x = { color = "#7c7f93" }"##, TOML), ["#7c7f93"], "no alpha: just the hex");
        assert_eq!(texts(r##"x = { color = "#7c7f93", alpha = 2 }"##, TOML), ["#7c7f93"]);
    }

    fn rewrite(line: &str, scan: Scan, colour: Colour) -> String {
        let l = one(line, scan);
        let chars: Vec<char> = line.chars().collect();
        let mut s: String = chars[..l.start].iter().collect();
        s.push_str(&l.format.write(&colour));
        s.extend(&chars[l.end..]);
        s
    }

    #[test]
    fn written_again_the_way_they_were() {
        let blue = Colour::from_u8(59, 130, 246);
        let half = blue.with_alpha(0.5);
        let white = Colour::WHITE;
        assert_eq!(rewrite("a: #fff;", CODE, Colour::from_u8(0x33, 0x66, 0x99)), "a: #369;");
        assert_eq!(rewrite("a: #fff;", CODE, blue), "a: #3b82f6;", "too fine for three digits");
        assert_eq!(rewrite("a: #fff;", CODE, Colour::from_u8(0x33, 0x66, 0x99).with_alpha(0.0)), "a: #3690;");
        assert_eq!(rewrite("a: #FFFFFF;", CODE, half), "a: #3B82F680;", "capitals kept, the alpha added");
        assert_eq!(rewrite("a: #ffffff80;", CODE, white), "a: #ffffffff;", "eight digits stay eight");
        assert_eq!(rewrite("rgb(0, 0, 0)", CODE, blue), "rgb(59, 130, 246)");
        assert_eq!(rewrite("rgb(0, 0, 0)", CODE, half), "rgba(59, 130, 246, 0.5)");
        assert_eq!(rewrite("rgba(0, 0, 0, 1)", CODE, blue), "rgba(59, 130, 246, 1)");
        assert_eq!(rewrite("rgb(0 0 0 / 100%)", CODE, half), "rgb(59 130 246 / 50%)");
        assert_eq!(rewrite("rgb(0% 0% 0%)", CODE, Colour::from_u8(255, 128, 0)), "rgb(100% 50.2% 0%)");
        assert_eq!(rewrite("hsl(0, 0%, 0%)", CODE, blue), "hsl(217.2, 91.2%, 59.8%)");
        assert_eq!(rewrite("HSL(0deg 0% 0%)", CODE, half), "HSL(217.2deg 91.2% 59.8% / 0.5)");
        assert_eq!(rewrite("hsl(0.1turn 0% 0%)", CODE, Colour::from_u8(0, 255, 255)), "hsl(0.5turn 100% 50%)");
        assert_eq!(rewrite("oklch(0% 0 0)", CODE, Colour::from_u8(255, 0, 0)), "oklch(62.8% 0.2577 29.23)");
        assert_eq!(rewrite("oklch(0.5 0 0 / 1)", CODE, Colour::from_u8(255, 0, 0).with_alpha(0.25)), "oklch(0.628 0.2577 29.23 / 0.25)");
        assert_eq!(rewrite("color: red", CSS, Colour::from_u8(0, 0, 255)), "color: blue");
        assert_eq!(rewrite("color: RED", CSS, Colour::from_u8(0, 0, 128)), "color: NAVY");
        assert_eq!(rewrite("color: red", CSS, blue), "color: #3b82f6", "no name for it");
        assert_eq!(rewrite("color: aqua", CSS, Colour::from_u8(0, 255, 255)), "color: aqua", "its own name, not cyan");
        assert_eq!(rewrite(r##"s = { color = "#000000", alpha = 0.1 }"##, TOML, half), r##"s = { color = "#3b82f6", alpha = 0.5 }"##);
    }

    #[test]
    fn written_and_read_back_the_same() {
        let colours = [Colour::from_u8(59, 130, 246), Colour::from_u8(1, 2, 3).with_alpha(0.2), Colour::from_u8(250, 128, 114), Colour::WHITE, Colour::BLACK.with_alpha(0.0)];
        let formats = ["#000", "#000000", "#00000000", "rgb(0, 0, 0)", "rgb(0 0 0 / 1)", "rgb(0% 0% 0%)", "hsl(0, 0%, 0%)", "hsl(0deg 0% 0% / 1)", "hsl(0rad 0% 0%)", "hsl(0grad 0% 0%)", "oklch(0% 0 0)", "oklch(0 0% 0 / 1)"];
        for f in formats {
            let format = one(f, CSS).format;
            for c in &colours {
                let text = format.write(c);
                let back = one(&text, CSS).colour;
                assert!(back.same_bytes(c), "{f} {c:?}: {text} reads as {back:?}");
            }
        }
    }

    /// Every colour of 4096 (#rgb's), through each function, back to the
    /// same bytes: the decimals written are enough.
    #[test]
    fn decimals_enough_for_every_byte() {
        let formats: Vec<Format> = ["rgb(0% 0% 0%)", "hsl(0 0% 0%)", "hsl(0, 0%, 0%)", "oklch(0% 0 0)", "oklch(0 0 0)", "oklch(0% 0% 0)"].iter().map(|f| one(f, CODE).format).collect();
        for v in 0..4096u32 {
            let n = |s: u32| ((v >> s) & 15) as u8 * 17;
            // And a neighbour, off the sixteenths.
            for c in [Colour::from_u8(n(8), n(4), n(0)), Colour::from_u8(n(8) ^ 1, n(4) ^ 3, n(0) ^ 2)] {
                for f in &formats {
                    let text = f.write(&c);
                    assert!(one(&text, CODE).colour.same_bytes(&c), "{text} for {:?}", c.rgb());
                }
            }
        }
    }

    #[test]
    fn out_of_gamut_is_written_mapped() {
        let wild = Colour::from_oklch(0.7, 0.4, 145.0, 1.0);
        let hex = Format::Hex { digits: 6, upper: false }.write(&wild);
        assert_eq!(hex, Format::Hex { digits: 6, upper: false }.write(&wild.gamut_map()));
        let ok = one("oklch(70% 0.4 145)", CODE);
        assert_eq!(ok.format.write(&ok.colour), "oklch(70% 0.4 145)", "OKLCH keeps what sRGB can't show");
    }

    #[test]
    fn converted_to_another_format() {
        let blue = Colour::from_u8(59, 130, 246);
        let from = Format::Hex { digits: 3, upper: false };
        let to = |t: Target, from: &Format, legacy| t.format(from, legacy).write(&blue);
        assert_eq!(to(Target::Rgb, &from, false), "rgb(59 130 246)");
        assert_eq!(to(Target::Rgb, &from, true), "rgb(59, 130, 246)");
        assert_eq!(to(Target::Hsl, &from, false), "hsl(217.2 91.2% 59.8%)");
        assert_eq!(to(Target::Oklch, &from, false), "oklch(62.31% 0.188 259.81)");
        let legacy = Format::Rgb { name: "rgba".into(), legacy: true, percent: false, alpha: Alpha::Number };
        assert_eq!(to(Target::Hsl, &legacy, false), "hsla(217.2, 91.2%, 59.8%, 1)", "its syntax and alpha kept");
        assert_eq!(Target::Hex.format(&Format::Hex { digits: 3, upper: true }, false).write(&blue), "#3B82F6");
        assert_eq!(Target::from_name("oklch"), Some(Target::Oklch));
    }

    #[test]
    fn the_literal_at_the_cursor() {
        let found = find("a: #fff, b: #abc", CODE);
        assert_eq!(at_column(&found, 4).map(|l| l.start), Some(3));
        assert_eq!(at_column(&found, 7).map(|l| l.start), Some(3), "just after it");
        assert_eq!(at_column(&found, 10).map(|l| l.start), Some(12), "the nearest");
        assert_eq!(at_column(&[], 0), None);
    }

    #[test]
    fn numbers_are_written_short() {
        assert_eq!(num(1.0, 3), "1");
        assert_eq!(num(0.5, 3), "0.5");
        assert_eq!(num(-0.0001, 2), "0");
        assert_eq!(num(217.2222, 2), "217.22");
        assert_eq!(num(100.0, 0), "100");
    }
}
