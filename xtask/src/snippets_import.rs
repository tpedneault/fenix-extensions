//! `cargo xtask snippets-import <snippets.json> --language <id> [--into <extension dir>] [--approximate] [--indent <n>]`:
//! VS Code snippets as Fenix's `.snippet` files.
//!
//! **What's read.** A VS Code snippet file (`rust.json`,
//! `x.code-snippets`), comments and trailing commas and all: each entry's
//! `prefix` (one, or a list: a file per prefix), `body` (a string, or a
//! list of lines), `scope` (in a global snippets file: the languages it's
//! for), and its name, which becomes the `# name:`.
//!
//! **What's written.** One file per prefix, `snippets/<language>/<prefix>.snippet`
//! under `--into` (an extension's folder), each with `# name:`, `# key:`,
//! `# scope: <language>`, `# --` and the body. Without `--into`, the files
//! are printed and nothing is written. Every body written is checked
//! against Fenix's own snippet parser (mirrored here, `fenix_render`):
//! what Fenix would refuse is never written.
//!
//! **What carries over exactly**: text, `$1` and `${1}` stops, `${1:text}`
//! placeholders, `$0`, the same stop again (a mirror, as in VS Code),
//! `TM_FILENAME`, `TM_FILENAME_BASE`, `TM_FILEPATH`, `TM_DIRECTORY`,
//! `CURRENT_YEAR`, `CURRENT_YEAR-CURRENT_MONTH-CURRENT_DATE` (Fenix's
//! `DATE`), `CURRENT_HOUR:CURRENT_MINUTE:CURRENT_SECOND` (`TIME`), and a
//! transform that upper- or lower-cases the whole of a stop or variable
//! (`${1/(.*)/${1:/upcase}/}` is `${1|upper}`), and `TM_SELECTED_TEXT`,
//! which is empty -- or its default, placeholders and all -- whenever a
//! snippet is expanded from its prefix, the only way Fenix expands one.
//!
//! **What Fenix's format can't say** is refused -- the snippet isn't
//! written, and the report says why -- unless `--approximate` is given,
//! and then each approximation is said:
//!
//! | VS Code | Refused, or with `--approximate` |
//! |---|---|
//! | a choice, `${1\|a,b\|}` | its first option as the placeholder's text: no list to pick from |
//! | a placeholder inside a placeholder, `${1:a ${2:b}}` | refused either way: Fenix fields hold text only |
//! | `${0:text}` | the text as the last field, then the end |
//! | the same stop with two defaults | the first default for both |
//! | `CLIPBOARD`, `UUID`, `TM_CURRENT_LINE`, ... (no Fenix variable) | its default text if it has one, else nothing |
//! | a variable's default, `${TM_FILENAME:x}` | the variable, without the default (Fenix's are empty when there's no file) |
//! | `${TM_FILENAME/(.*)\..+$/$1/}` | `${FILE_STEM}`: the same, but for a name starting with a dot |
//! | any other regex transform | the value, untransformed |
//! | an unknown `$name` (VS Code makes it a placeholder) | a placeholder with the name as its text |
//!
//! A prefix with a space in it (Fenix's triggers have none), a snippet
//! with no prefix (VS Code only offers it from Insert Snippet), and one
//! scoped to other languages are left out and said. Tabs that indent a
//! body are inserted as tabs by Fenix, where VS Code indents them as you
//! do: `--indent 4` writes them as four spaces.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

const USAGE: &str = "cargo xtask snippets-import <snippets.json> --language <id> [--into <extension dir>] [--approximate] [--indent <n>]";

/// Fenix's snippet variables (`fenix-snippets`' `Template::parse`).
const FENIX_VARIABLES: &[&str] = &["DATE", "TIME", "YEAR", "DATETIME", "USER_NAME", "FILENAME", "FILE_STEM", "FILEPATH", "DIRECTORY"];

/// VS Code's variables that are exactly one of Fenix's.
const SAME_VARIABLES: &[(&str, &str)] = &[("TM_FILENAME", "FILENAME"), ("TM_FILENAME_BASE", "FILE_STEM"), ("TM_FILEPATH", "FILEPATH"), ("TM_DIRECTORY", "DIRECTORY"), ("CURRENT_YEAR", "YEAR")];

/// VS Code's language ids that Fenix calls something else.
const LANGUAGES: &[(&str, &str)] = &[("shellscript", "bash"), ("bat", "batch"), ("typescriptreact", "tsx"), ("plaintext", "text")];

#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    pub input: PathBuf,
    pub language: String,
    pub into: Option<PathBuf>,
    pub approximate: bool,
    pub indent: Option<usize>,
}

pub fn options(args: &[String]) -> Result<Options, String> {
    let (mut input, mut language, mut into, mut approximate, mut indent) = (None, None, None, false, None);
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f, Some(v.to_string())),
            _ => (arg.as_str(), None),
        };
        let mut value = |what: &str| inline.clone().or_else(|| args.next().cloned()).ok_or(format!("{what} needs a value -- {USAGE}"));
        match flag {
            "--language" => language = Some(value("--language")?),
            "--into" => into = Some(PathBuf::from(value("--into")?)),
            "--approximate" => approximate = true,
            "--indent" => indent = Some(value("--indent")?.parse::<usize>().ok().filter(|n| (1..=16).contains(n)).ok_or("--indent: a number of spaces, 1 to 16")?),
            f if f.starts_with("--") => return Err(format!("snippets-import: {f}? -- {USAGE}")),
            _ if input.is_none() => input = Some(PathBuf::from(arg)),
            _ => return Err(format!("snippets-import: one file at a time -- {USAGE}")),
        }
    }
    let language = language.ok_or(format!("--language: the Fenix language they're for (rust, python, bash, ...) -- {USAGE}"))?;
    if language.is_empty() || !language.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '+')) {
        return Err(format!("--language {language:?}: a language as Fenix names it, in lower case (rust, cpp, bash)"));
    }
    Ok(Options { input: input.ok_or(USAGE)?, language, into, approximate, indent })
}

/// A piece of a VS Code snippet's body.
#[derive(Debug, Clone, PartialEq)]
enum Node {
    Text(String),
    Stop(u32),
    Placeholder(u32, Vec<Node>),
    Choice(u32, Vec<String>),
    Variable(String, Option<Vec<Node>>),
    Transform { of: Box<Node>, regex: String, format: String },
    /// One of Fenix's variables, made from VS Code's parts (`dates`).
    Fenix(&'static str),
}

/// VS Code's snippet grammar, read as leniently as VS Code reads it:
/// what isn't one of its forms is text.
struct Parser {
    chars: Vec<char>,
    at: usize,
}

impl Parser {
    fn peek(&self, n: usize) -> Option<char> {
        self.chars.get(self.at + n).copied()
    }

    /// Nodes until the end, or (inside a placeholder) an unescaped `}`.
    fn nodes(&mut self, nested: bool) -> Vec<Node> {
        let mut out: Vec<Node> = Vec::new();
        let mut text = String::new();
        while let Some(c) = self.peek(0) {
            if c == '\\' && self.peek(1).is_some_and(|n| matches!(n, '$' | '}' | '\\')) {
                text.push(self.peek(1).unwrap());
                self.at += 2;
                continue;
            }
            if nested && c == '}' {
                break;
            }
            if c == '$' {
                let start = self.at;
                if let Some(node) = self.dollar() {
                    if !text.is_empty() {
                        out.push(Node::Text(std::mem::take(&mut text)));
                    }
                    out.push(node);
                    continue;
                }
                self.at = start + 1;
                text.push('$');
                continue;
            }
            text.push(c);
            self.at += 1;
        }
        if !text.is_empty() {
            out.push(Node::Text(text));
        }
        out
    }

    fn int(&mut self) -> Option<u32> {
        let start = self.at;
        while self.peek(0).is_some_and(|c| c.is_ascii_digit()) {
            self.at += 1;
        }
        (self.at > start).then(|| self.chars[start..self.at].iter().collect::<String>().parse().ok()).flatten()
    }

    fn var(&mut self) -> Option<String> {
        let first = self.peek(0)?;
        if !(first == '_' || first.is_ascii_alphabetic()) {
            return None;
        }
        let start = self.at;
        while self.peek(0).is_some_and(|c| c == '_' || c.is_ascii_alphanumeric()) {
            self.at += 1;
        }
        Some(self.chars[start..self.at].iter().collect())
    }

    /// What follows a `$`, if it's one of the forms; the cursor is left
    /// anywhere when it isn't.
    fn dollar(&mut self) -> Option<Node> {
        self.at += 1;
        if let Some(n) = self.int() {
            return Some(Node::Stop(n));
        }
        if let Some(v) = self.var() {
            return Some(Node::Variable(v, None));
        }
        if self.peek(0) != Some('{') {
            return None;
        }
        self.at += 1;
        if let Some(n) = self.int() {
            match self.peek(0)? {
                '}' => {
                    self.at += 1;
                    Some(Node::Stop(n))
                }
                ':' => {
                    self.at += 1;
                    let inner = self.nodes(true);
                    (self.peek(0)? == '}').then(|| {
                        self.at += 1;
                        Node::Placeholder(n, inner)
                    })
                }
                '|' => {
                    self.at += 1;
                    let mut options = vec![String::new()];
                    loop {
                        match self.peek(0)? {
                            '\\' if self.peek(1).is_some_and(|c| matches!(c, '$' | '}' | '\\' | ',' | '|')) => {
                                options.last_mut().unwrap().push(self.peek(1).unwrap());
                                self.at += 2;
                            }
                            ',' => {
                                options.push(String::new());
                                self.at += 1;
                            }
                            '|' if self.peek(1) == Some('}') => {
                                self.at += 2;
                                return Some(Node::Choice(n, options));
                            }
                            c => {
                                options.last_mut().unwrap().push(c);
                                self.at += 1;
                            }
                        }
                    }
                }
                '/' => self.transform(Node::Stop(n)),
                _ => None,
            }
        } else {
            let v = self.var()?;
            match self.peek(0)? {
                '}' => {
                    self.at += 1;
                    Some(Node::Variable(v, None))
                }
                ':' => {
                    self.at += 1;
                    let inner = self.nodes(true);
                    (self.peek(0)? == '}').then(|| {
                        self.at += 1;
                        Node::Variable(v, Some(inner))
                    })
                }
                '/' => self.transform(Node::Variable(v, None)),
                _ => None,
            }
        }
    }

    /// `/regex/format/options}`, after a stop or a variable.
    fn transform(&mut self, of: Node) -> Option<Node> {
        self.at += 1;
        let regex = self.until_slash(false)?;
        self.at += 1;
        let format = self.until_slash(true)?;
        self.at += 1;
        while let Some(c) = self.peek(0) {
            self.at += 1;
            if c == '}' {
                return Some(Node::Transform { of: Box::new(of), regex, format });
            }
        }
        None
    }

    /// Up to the next unescaped `/`; in a format, `${1:/upcase}` is one
    /// piece, `/` and all.
    fn until_slash(&mut self, format: bool) -> Option<String> {
        let mut s = String::new();
        loop {
            match self.peek(0)? {
                '\\' => {
                    s.push('\\');
                    s.push(self.peek(1)?);
                    self.at += 2;
                }
                '/' => return Some(s),
                '$' if format && self.peek(1) == Some('{') => {
                    let mut depth = 0;
                    loop {
                        let c = self.peek(0)?;
                        s.push(c);
                        self.at += 1;
                        if c == '{' {
                            depth += 1;
                        } else if c == '}' {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                    }
                }
                c => {
                    s.push(c);
                    self.at += 1;
                }
            }
        }
    }
}

fn parse_body(body: &str) -> Vec<Node> {
    Parser { chars: body.chars().collect(), at: 0 }.nodes(false)
}

/// Text as Fenix reads it: `$` always escaped, a backslash only where
/// Fenix would otherwise take it as an escape.
fn escape_text(text: &str, followed_by_dollar: bool) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    for (i, &c) in chars.iter().enumerate() {
        match c {
            '$' => out.push_str("\\$"),
            '\\' => {
                let next = chars.get(i + 1).copied();
                let special = match next {
                    Some(n) => matches!(n, '$' | '}' | '\\'),
                    None => followed_by_dollar,
                };
                out.push_str(if special { "\\\\" } else { "\\" });
            }
            c => out.push(c),
        }
    }
    out
}

/// A placeholder's default as Fenix reads it: inside `${n:...}`, where
/// `}`, `$` and `\` are escaped.
fn escape_default(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '$' | '}' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// What became of one snippet.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Its body for Fenix, and what was approximated.
    Converted(String, Vec<String>),
    /// Why it can't be said in Fenix's format.
    Refused(String),
}

/// A VS Code body as a Fenix one.
pub fn convert_body(body: &str, approximate: bool) -> Outcome {
    let nodes = dates(selected(parse_body(body)));
    let mut out = String::new();
    let mut approx = Vec::new();
    let mut defaults: BTreeMap<u32, String> = BTreeMap::new();
    let next_free = std::cell::Cell::new(max_stop(&nodes) + 1);
    // Stops typed into somewhere: Fenix transforms only a copy of one.
    let typed: BTreeSet<u32> = nodes.iter().filter_map(|n| match n {
        Node::Stop(n) | Node::Placeholder(n, _) | Node::Choice(n, _) => Some(*n),
        _ => None,
    }).collect();
    // Refused, or approximated and said, as asked.
    macro_rules! cant {
        ($why:expr, $approx:expr) => {{
            if !approximate {
                return Outcome::Refused(format!("{} (--approximate: {})", $why, $approx));
            }
            approx.push(format!("{}: {}", $why, $approx));
        }};
    }
    macro_rules! never {
        ($why:expr) => {
            return Outcome::Refused($why.to_string())
        };
    }
    let text_only = |nodes: &[Node]| -> Option<String> {
        nodes.iter().map(|n| if let Node::Text(t) = n { Some(t.as_str()) } else { None }).collect::<Option<Vec<_>>>().map(|v| v.concat())
    };
    for (i, node) in nodes.iter().enumerate() {
        let next_is_digit = matches!(nodes.get(i + 1), Some(Node::Text(t)) if t.starts_with(|c: char| c.is_ascii_digit()));
        let stop = |n: u32| if next_is_digit { format!("${{{n}}}") } else { format!("${n}") };
        match node {
            Node::Text(t) => out.push_str(&escape_text(t, i + 1 < nodes.len())),
            Node::Stop(n) => out.push_str(&stop(*n)),
            Node::Placeholder(0, inner) => {
                let Some(text) = text_only(inner) else { never!("${0:...} holding a placeholder") };
                cant!(format!("${{0:{text}}} (the end, with text selected there)"), "the text as a last field, then the end");
                let n = next_free.get();
                next_free.set(n + 1);
                let _ = write!(out, "${{{n}:{}}}$0", escape_default(&text));
            }
            Node::Placeholder(n, inner) => {
                let Some(text) = text_only(inner) else { never!(format!("a placeholder inside placeholder {n}: Fenix's fields hold text only")) };
                match defaults.get(n) {
                    Some(first) if *first == text => out.push_str(&stop(*n)),
                    Some(first) => {
                        cant!(format!("stop {n} has two defaults ({first:?}, {text:?})"), "the first for both");
                        out.push_str(&stop(*n));
                    }
                    None => {
                        defaults.insert(*n, text.clone());
                        let _ = write!(out, "${{{n}:{}}}", escape_default(&text));
                    }
                }
            }
            Node::Choice(n, options) => {
                let first = options.first().cloned().unwrap_or_default();
                cant!(format!("a choice for stop {n} ({})", options.join(" | ")), format!("{first:?}, the first, as its text: no list to pick from"));
                if defaults.contains_key(n) {
                    out.push_str(&stop(*n));
                } else {
                    defaults.insert(*n, first.clone());
                    let _ = write!(out, "${{{n}:{}}}", escape_default(&first));
                }
            }
            Node::Fenix(name) => {
                let _ = write!(out, "${{{name}}}");
            }
            Node::Variable(name, default) => {
                match (SAME_VARIABLES.iter().find(|(v, _)| v == name), default) {
                    (Some((_, fenix)), None) => {
                        let _ = write!(out, "${{{fenix}}}");
                    }
                    (Some((_, fenix)), Some(_)) => {
                        cant!(format!("${{{name}:...}}, a variable with a default"), format!("${{{fenix}}} without it: empty where there's no file"));
                        let _ = write!(out, "${{{fenix}}}");
                    }
                    (None, d) if known_vs_variable(name) => {
                        let text = d.as_deref().and_then(text_only);
                        if d.is_some() && text.is_none() {
                            never!(format!("${{{name}:...}} holding a placeholder"));
                        }
                        cant!(format!("{name}, which Fenix's snippets don't have"), match &text {
                            Some(t) => format!("{t:?}, its default, as text"),
                            None => "nothing in its place".to_string(),
                        });
                        out.push_str(&escape_text(&text.unwrap_or_default(), i + 1 < nodes.len()));
                    }
                    (None, _) => {
                        cant!(format!("${name}, a variable VS Code doesn't know: it makes it a placeholder"), format!("a placeholder reading {name:?}"));
                        let n = next_free.get();
                        next_free.set(n + 1);
                        let _ = write!(out, "${{{n}:{}}}", escape_default(name));
                    }
                }
            }
            Node::Transform { of, regex, format } => {
                let target = match of.as_ref() {
                    Node::Stop(n) => Some(n.to_string()),
                    Node::Variable(v, _) => SAME_VARIABLES.iter().find(|(x, _)| x == v).map(|(_, f)| f.to_string()),
                    _ => None,
                };
                let shown = match of.as_ref() {
                    Node::Stop(n) => format!("${{{n}/{regex}/{format}/}}"),
                    Node::Variable(v, _) => format!("${{{v}/{regex}/{format}/}}"),
                    _ => String::new(),
                };
                let Some(target) = target else {
                    never!(format!("{shown}: a transform of a variable Fenix doesn't have"));
                };
                let whole = matches!(regex.as_str(), "(.*)" | "^(.*)$" | ".*" | "^.*$");
                let group = if regex.contains('(') { "1" } else { "0" };
                let case = if whole && *format == format!("${{{group}:/upcase}}") {
                    Some("upper")
                } else if whole && *format == format!("${{{group}:/downcase}}") {
                    Some("lower")
                } else {
                    None
                };
                let untyped = matches!(of.as_ref(), Node::Stop(n) if !typed.contains(n));
                if untyped {
                    cant!(format!("{shown}: stop {target} is only ever seen transformed, and Fenix transforms a copy of a field, never the one you type in"), format!("${target}, untransformed"));
                    let _ = write!(out, "${target}");
                } else if let Some(case) = case {
                    let _ = write!(out, "${{{target}|{case}}}");
                } else if target == "FILENAME" && (regex == "(.*)\\..+$" || regex == "(.*)\\.[^.]+$") && format == "$1" {
                    cant!(format!("{shown}, the file name without its extension"), "${FILE_STEM}: the same, but for a name that starts with a dot");
                    out.push_str("${FILE_STEM}");
                } else {
                    cant!(format!("{shown}, a regular expression transform"), "the value, untransformed");
                    let _ = write!(out, "${{{target}}}");
                }
            }
        }
    }
    // What Fenix itself would make of it.
    match fenix_render(&out) {
        Ok(_) => Outcome::Converted(out, approx),
        Err(e) => Outcome::Refused(format!("Fenix wouldn't read what it makes ({e}): {out:?}")),
    }
}

/// Variables VS Code has (its snippet docs) that aren't one of Fenix's.
fn known_vs_variable(name: &str) -> bool {
    matches!(
        name,
        "TM_CURRENT_LINE" | "TM_CURRENT_WORD" | "TM_LINE_INDEX" | "TM_LINE_NUMBER" | "RELATIVE_FILEPATH" | "CLIPBOARD" | "WORKSPACE_NAME" | "WORKSPACE_FOLDER" | "CURSOR_INDEX" | "CURSOR_NUMBER"
            | "CURRENT_YEAR_SHORT" | "CURRENT_MONTH" | "CURRENT_MONTH_NAME" | "CURRENT_MONTH_NAME_SHORT" | "CURRENT_DATE" | "CURRENT_DAY_NAME" | "CURRENT_DAY_NAME_SHORT" | "CURRENT_HOUR" | "CURRENT_MINUTE" | "CURRENT_SECOND" | "CURRENT_SECONDS_UNIX" | "CURRENT_TIMEZONE_OFFSET"
            | "RANDOM" | "RANDOM_HEX" | "UUID" | "BLOCK_COMMENT_START" | "BLOCK_COMMENT_END" | "LINE_COMMENT"
    )
}

/// `TM_SELECTED_TEXT` is empty whenever a snippet is expanded from its
/// prefix -- the only way Fenix expands one -- so it's nothing, or its
/// default in its place, exactly.
fn selected(nodes: Vec<Node>) -> Vec<Node> {
    let mut out = Vec::new();
    for n in nodes {
        match n {
            Node::Variable(v, default) if v == "TM_SELECTED_TEXT" => out.extend(selected(default.unwrap_or_default())),
            Node::Placeholder(i, inner) => out.push(Node::Placeholder(i, selected(inner))),
            Node::Variable(v, Some(inner)) => out.push(Node::Variable(v, Some(selected(inner)))),
            n => out.push(n),
        }
    }
    // Text runs joined again.
    let mut joined: Vec<Node> = Vec::new();
    for n in out {
        match (joined.last_mut(), n) {
            (Some(Node::Text(a)), Node::Text(b)) => a.push_str(&b),
            (_, n) => joined.push(n),
        }
    }
    joined
}

/// `CURRENT_YEAR-CURRENT_MONTH-CURRENT_DATE` is Fenix's `DATE`, and
/// `CURRENT_HOUR:CURRENT_MINUTE:CURRENT_SECOND` its `TIME`, exactly.
fn dates(nodes: Vec<Node>) -> Vec<Node> {
    let var = |n: &Node, name: &str| matches!(n, Node::Variable(v, None) if v == name);
    let mut out: Vec<Node> = Vec::new();
    let mut i = 0;
    // Text is split around the separators so a run can be found inside it.
    let mut flat: Vec<Node> = Vec::new();
    for n in nodes {
        match n {
            Node::Text(t) => {
                let mut run = String::new();
                for c in t.chars() {
                    if c == '-' || c == ':' {
                        if !run.is_empty() {
                            flat.push(Node::Text(std::mem::take(&mut run)));
                        }
                        flat.push(Node::Text(c.to_string()));
                    } else {
                        run.push(c);
                    }
                }
                if !run.is_empty() {
                    flat.push(Node::Text(run));
                }
            }
            n => flat.push(n),
        }
    }
    let sep = |n: &Node, s: &str| matches!(n, Node::Text(t) if t == s);
    while i < flat.len() {
        let five = &flat[i..(i + 5).min(flat.len())];
        if five.len() == 5 && var(&five[0], "CURRENT_YEAR") && sep(&five[1], "-") && var(&five[2], "CURRENT_MONTH") && sep(&five[3], "-") && var(&five[4], "CURRENT_DATE") {
            out.push(Node::Fenix("DATE"));
            i += 5;
        } else if five.len() == 5 && var(&five[0], "CURRENT_HOUR") && sep(&five[1], ":") && var(&five[2], "CURRENT_MINUTE") && sep(&five[3], ":") && var(&five[4], "CURRENT_SECOND") {
            out.push(Node::Fenix("TIME"));
            i += 5;
        } else {
            out.push(flat[i].clone());
            i += 1;
        }
    }
    // Text runs joined again.
    let mut joined: Vec<Node> = Vec::new();
    for n in out {
        match (joined.last_mut(), n) {
            (Some(Node::Text(a)), Node::Text(b)) => a.push_str(&b),
            (_, n) => joined.push(n),
        }
    }
    joined
}

fn max_stop(nodes: &[Node]) -> u32 {
    nodes
        .iter()
        .map(|n| match n {
            Node::Stop(n) | Node::Choice(n, _) => *n,
            Node::Placeholder(n, inner) => (*n).max(max_stop(inner)),
            Node::Transform { of, .. } => max_stop(std::slice::from_ref(of)),
            Node::Variable(_, Some(inner)) => max_stop(inner),
            _ => 0,
        })
        .max()
        .unwrap_or(0)
}

/// Fenix's own reading of a snippet body (`fenix-snippets`'
/// `Template::parse`, copied), rendered with its defaults and every
/// variable as `<NAME>`: what Tab would insert, or why Fenix refuses it.
pub fn fenix_render(body: &str) -> Result<String, String> {
    #[derive(Debug)]
    enum Part {
        Text(String),
        Field(u32, Vec<String>),
        Variable(String, Vec<String>),
    }
    fn unescape(s: &str) -> String {
        let mut chars = s.chars().peekable();
        let mut out = String::new();
        while let Some(c) = chars.next() {
            if c == '\\' && chars.peek().is_some_and(|c| matches!(c, '$' | '}' | '\\')) {
                out.push(chars.next().unwrap());
            } else {
                out.push(c);
            }
        }
        out
    }
    let transform = |s: &str| -> Result<String, String> {
        let ok = matches!(s, "trim" | "upper" | "lower" | "remove_whitespace") || s.strip_prefix("center:").and_then(|n| n.parse::<usize>().ok()).is_some_and(|n| (1..=4096).contains(&n));
        if ok { Ok(s.to_string()) } else { Err(format!("unknown transform or invalid width: {s}")) }
    };
    let mut parts = Vec::new();
    let mut defaults: BTreeMap<u32, String> = BTreeMap::new();
    let mut defined = BTreeSet::new();
    let mut text = String::new();
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek().is_some_and(|c| matches!(c, '$' | '}' | '\\')) {
            text.push(chars.next().unwrap());
            continue;
        }
        if c != '$' || !chars.peek().is_some_and(|c| *c == '{' || c.is_ascii_digit()) {
            text.push(c);
            continue;
        }
        if !text.is_empty() {
            parts.push(Part::Text(std::mem::take(&mut text)));
        }
        let expr = if chars.peek() == Some(&'{') {
            chars.next();
            let mut expr = String::new();
            let mut closed = false;
            while let Some(c) = chars.next() {
                if c == '}' {
                    closed = true;
                    break;
                }
                expr.push(c);
                if c == '\\' {
                    if let Some(next) = chars.next() {
                        expr.push(next);
                    }
                } else if c == '$' && chars.peek() == Some(&'{') {
                    return Err("nested placeholders are not supported".into());
                }
            }
            if !closed {
                return Err("unclosed ${ expression".into());
            }
            expr
        } else {
            let mut expr = String::new();
            while chars.peek().is_some_and(char::is_ascii_digit) {
                expr.push(chars.next().unwrap());
            }
            expr
        };
        let head_end = expr.find([':', '|']).unwrap_or(expr.len());
        let (head, tail) = (&expr[..head_end], &expr[head_end..]);
        let transforms = match tail.strip_prefix('|') {
            Some(pipeline) => pipeline.split('|').map(transform).collect::<Result<Vec<_>, _>>()?,
            None => Vec::new(),
        };
        if !head.is_empty() && head.chars().all(|c| c.is_ascii_digit()) {
            let id = head.parse::<u32>().map_err(|_| "field number too large")?;
            if id == 0 && !tail.is_empty() {
                return Err("$0 cannot have a default or transform".into());
            }
            if let Some(default) = tail.strip_prefix(':') {
                if !defined.insert(id) {
                    return Err(format!("duplicate default for field {id}"));
                }
                defaults.insert(id, unescape(default));
            }
            parts.push(Part::Field(id, transforms));
        } else {
            if !FENIX_VARIABLES.contains(&head) {
                return Err(format!("unknown variable: {head}"));
            }
            if tail.starts_with(':') {
                return Err("variables do not accept defaults".into());
            }
            parts.push(Part::Variable(head.to_string(), transforms));
        }
    }
    if !text.is_empty() {
        parts.push(Part::Text(text));
    }
    let mut editable = BTreeSet::new();
    let mut exits = 0;
    for part in &parts {
        if let Part::Field(id, transforms) = part {
            if *id == 0 {
                exits += 1;
            } else if transforms.is_empty() {
                editable.insert(*id);
            }
        }
    }
    if exits > 1 {
        return Err("only one $0 is allowed".into());
    }
    for part in &parts {
        if let Part::Field(id, transforms) = part {
            if !transforms.is_empty() && !editable.contains(id) {
                return Err(format!("transform refers to missing editable field {id}"));
            }
        }
    }
    let apply = |mut v: String, transforms: &[String]| {
        for t in transforms {
            v = match t.as_str() {
                "upper" => v.to_uppercase(),
                "lower" => v.to_lowercase(),
                "trim" => v.trim().to_string(),
                "remove_whitespace" => v.chars().filter(|c| !c.is_whitespace()).collect(),
                _ => v,
            };
        }
        v
    };
    Ok(parts
        .iter()
        .map(|p| match p {
            Part::Text(t) => t.clone(),
            Part::Field(id, transforms) => apply(defaults.get(id).cloned().unwrap_or_default(), transforms),
            Part::Variable(name, transforms) => apply(format!("<{name}>"), transforms),
        })
        .collect())
}

/// One snippet file to write.
#[derive(Debug, Clone, PartialEq)]
pub struct File {
    pub path: String,
    pub text: String,
}

#[derive(Debug, Default)]
pub struct Report {
    pub snippets: usize,
    pub files: Vec<File>,
    /// A snippet's name and why it wasn't written.
    pub refused: Vec<(String, String)>,
    /// A snippet's name and what was approximated.
    pub approximated: Vec<(String, String)>,
    pub notes: Vec<String>,
}

impl std::fmt::Display for Report {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{} snippet(s) read: {} file(s) made, {} left out", self.snippets, self.files.len(), self.refused.len())?;
        if !self.refused.is_empty() {
            writeln!(f, "\nLeft out ({}):", self.refused.len())?;
            for (name, why) in &self.refused {
                writeln!(f, "  {name}: {why}")?;
            }
        }
        if !self.approximated.is_empty() {
            writeln!(f, "\nApproximated, as --approximate asked ({}):", self.approximated.len())?;
            for (name, what) in &self.approximated {
                writeln!(f, "  {name}: {what}")?;
            }
        }
        for n in &self.notes {
            writeln!(f, "\nNote: {n}")?;
        }
        Ok(())
    }
}

/// A prefix as a file name: letters, digits, `-` and `_`.
fn file_stem(prefix: &str) -> String {
    let s: String = prefix.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c.to_ascii_lowercase() } else { '-' }).collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() { "snippet".into() } else { s }
}

/// Every snippet in `doc` for Fenix's `language`, as files under
/// `snippets/<language>/`, and what was said.
pub fn convert(doc: &serde_json::Value, o: &Options) -> Result<Report, String> {
    let entries = doc.as_object().ok_or("a snippets file is a JSON object of snippets, by name")?;
    let mut report = Report::default();
    let mut taken: BTreeSet<String> = BTreeSet::new();
    let mut triggers: BTreeMap<String, String> = BTreeMap::new();
    let (mut descriptions, mut tabbed, mut templates, mut selections) = (0, 0, 0, 0);
    for (name, entry) in entries {
        report.snippets += 1;
        let Some(entry) = entry.as_object() else {
            report.refused.push((name.clone(), "not a snippet (an object with a prefix and a body)".into()));
            continue;
        };
        if let Some(scope) = entry.get("scope").and_then(|s| s.as_str()) {
            let languages: Vec<String> = scope.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).map(|s| LANGUAGES.iter().find(|(v, _)| *v == s).map(|(_, f)| f.to_string()).unwrap_or(s.to_string())).collect();
            if !languages.is_empty() && !languages.contains(&o.language) {
                report.refused.push((name.clone(), format!("it's for {scope}, not {}", o.language)));
                continue;
            }
        }
        let prefixes: Vec<String> = match entry.get("prefix") {
            Some(serde_json::Value::String(p)) => vec![p.clone()],
            Some(serde_json::Value::Array(a)) => a.iter().filter_map(|p| p.as_str().map(str::to_string)).collect(),
            _ => Vec::new(),
        };
        if prefixes.is_empty() {
            report.refused.push((name.clone(), "no prefix: VS Code offers it only from Insert Snippet, and a Fenix snippet needs a key".into()));
            continue;
        }
        let body = match entry.get("body") {
            Some(serde_json::Value::String(b)) => b.clone(),
            Some(serde_json::Value::Array(a)) => a.iter().map(|l| l.as_str().unwrap_or_default()).collect::<Vec<_>>().join("\n"),
            _ => {
                report.refused.push((name.clone(), "no body".into()));
                continue;
            }
        };
        if entry.get("description").is_some() {
            descriptions += 1;
        }
        if entry.get("isFileTemplate").and_then(|b| b.as_bool()) == Some(true) {
            templates += 1;
        }
        let body = if body.lines().any(|l| l.starts_with('\t')) {
            tabbed += 1;
            match o.indent {
                Some(n) => body.lines().map(|l| {
                    let tabs = l.chars().take_while(|c| *c == '\t').count();
                    format!("{}{}", " ".repeat(n * tabs), &l[tabs..])
                }).collect::<Vec<_>>().join("\n"),
                None => body,
            }
        } else {
            body
        };
        if body.contains("TM_SELECTED_TEXT") {
            selections += 1;
        }
        let (fenix_body, approx) = match convert_body(&body, o.approximate) {
            Outcome::Converted(b, a) => (b, a),
            Outcome::Refused(why) => {
                report.refused.push((name.clone(), why));
                continue;
            }
        };
        if fenix_body.trim().is_empty() {
            report.refused.push((name.clone(), "nothing left of its body".into()));
            continue;
        }
        for a in approx {
            report.approximated.push((name.clone(), a));
        }
        let shown_name: String = name.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>().trim().to_string();
        for prefix in prefixes {
            if prefix.is_empty() || prefix.chars().any(char::is_whitespace) {
                report.refused.push((name.clone(), format!("prefix {prefix:?}: a Fenix key has no spaces")));
                continue;
            }
            if let Some(other) = triggers.get(&prefix) {
                report.refused.push((name.clone(), format!("prefix {prefix:?} is {other:?}'s too: Fenix expands one snippet per key")));
                continue;
            }
            triggers.insert(prefix.clone(), name.clone());
            let mut stem = file_stem(&prefix);
            let mut n = 2;
            while taken.contains(&stem) {
                stem = format!("{}-{n}", file_stem(&prefix));
                n += 1;
            }
            taken.insert(stem.clone());
            let label = if shown_name.is_empty() { prefix.clone() } else { shown_name.clone() };
            let text = format!("# name: {label}\n# key: {prefix}\n# scope: {}\n# --\n{fenix_body}", o.language);
            report.files.push(File { path: format!("snippets/{}/{stem}.snippet", o.language), text });
        }
    }
    if descriptions > 0 {
        report.notes.push(format!("{descriptions} description(s) not kept: a Fenix snippet has a name, and the name is the snippet's own"));
    }
    if tabbed > 0 {
        report.notes.push(match o.indent {
            Some(n) => format!("{tabbed} snippet(s) indent with tabs: written as {n} spaces each (--indent)"),
            None => format!("{tabbed} snippet(s) indent with tabs, which Fenix inserts as tabs where VS Code indents as you do: --indent <n> writes them as spaces"),
        });
    }
    if selections > 0 {
        report.notes.push(format!("{selections} snippet(s) use TM_SELECTED_TEXT: empty (or its default) as when typed from the prefix -- Fenix has no way to wrap a selection in a snippet"));
    }
    if templates > 0 {
        report.notes.push(format!("{templates} file template(s) (isFileTemplate): imported as ordinary snippets"));
    }
    Ok(report)
}

pub fn run(args: &[String]) -> Result<(), String> {
    let o = options(args)?;
    let text = std::fs::read_to_string(&o.input).map_err(|e| format!("{}: {e}", o.input.display()))?;
    let doc = crate::jsonc::parse(&text).map_err(|e| format!("{}: {e}", o.input.display()))?;
    let report = convert(&doc, &o)?;
    match &o.into {
        Some(dir) => write_into(dir, &report)?,
        None => {
            for f in &report.files {
                println!("==> {} <==\n{}\n", f.path, f.text);
            }
            println!("(nothing written: --into <extension dir> writes them)\n");
        }
    }
    print!("{report}");
    Ok(())
}

/// The files, into an extension's folder: none if any is there already.
fn write_into(dir: &Path, report: &Report) -> Result<(), String> {
    if !dir.join("extension.toml").is_file() {
        return Err(format!("{}: not an extension's folder (no extension.toml)", dir.display()));
    }
    let there: Vec<String> = report.files.iter().filter(|f| dir.join(&f.path).exists()).map(|f| f.path.clone()).collect();
    if !there.is_empty() {
        return Err(format!("{} already has {}: nothing was written", dir.display(), there.join(", ")));
    }
    for f in &report.files {
        let path = dir.join(&f.path);
        std::fs::create_dir_all(path.parent().expect("snippets/<language>")).map_err(|e| format!("{}: {e}", path.display()))?;
        std::fs::write(&path, &f.text).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("wrote {}", path.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exact(body: &str) -> String {
        match convert_body(body, false) {
            Outcome::Converted(b, a) => {
                assert!(a.is_empty(), "{body}: {a:?}");
                b
            }
            Outcome::Refused(why) => panic!("{body}: {why}"),
        }
    }

    fn refused(body: &str) -> String {
        match convert_body(body, false) {
            Outcome::Refused(why) => why,
            other => panic!("{body}: {other:?}"),
        }
    }

    fn approximated(body: &str) -> (String, Vec<String>) {
        match convert_body(body, true) {
            Outcome::Converted(b, a) => {
                assert!(!a.is_empty(), "{body}: nothing said");
                (b, a)
            }
            Outcome::Refused(why) => panic!("{body}: {why}"),
        }
    }

    #[test]
    fn what_fenix_can_say_carries_over_exactly() {
        assert_eq!(exact("fn ${1:name}($2) {\n\t$0\n}"), "fn ${1:name}($2) {\n\t$0\n}");
        assert_eq!(exact("${1}x"), "$1x");
        assert_eq!(exact("$1 2"), "$1 2");
        assert_eq!(exact("${1:a}${2}3"), "${1:a}${2}3", "a stop before a digit stays braced");
        assert_eq!(exact("${1:a} and ${1:a} and $1"), "${1:a} and $1 and $1", "a stop again is a mirror");
        assert_eq!(exact("${1:x\\}y} \\$HOME"), "${1:x\\}y} \\$HOME");
        assert_eq!(exact("cost: $ 5, $$"), "cost: \\$ 5, \\$\\$");
        assert_eq!(exact("a\\\\b \\\\$1"), "a\\b \\\\$1", "a backslash doubled only before an escape");
        assert_eq!(exact("${TM_FILENAME_BASE} $TM_FILENAME ${TM_DIRECTORY}"), "${FILE_STEM} ${FILENAME} ${DIRECTORY}");
        assert_eq!(exact("${CURRENT_YEAR}-${CURRENT_MONTH}-${CURRENT_DATE} $CURRENT_HOUR:$CURRENT_MINUTE:$CURRENT_SECOND, $CURRENT_YEAR"), "${DATE} ${TIME}, ${YEAR}");
        assert_eq!(exact("$1 ${1/(.*)/${1:/upcase}/} ${TM_FILENAME_BASE/(.*)/${1:/downcase}/g}"), "$1 ${1|upper} ${FILE_STEM|lower}");
        assert_eq!(exact("x {} y}"), "x {} y}");
        assert_eq!(exact("if (${1:c}) {\n\t$TM_SELECTED_TEXT$0\n}"), "if (${1:c}) {\n\t$0\n}", "nothing is selected when a prefix expands");
        assert_eq!(exact("{ ${TM_SELECTED_TEXT:${1:body}} }"), "{ ${1:body} }", "its default in its place");
    }

    #[test]
    fn what_it_cant_say_is_refused_and_says_what_approximate_would_do() {
        for (body, says) in [
            ("${1|one,two|}", "a choice"),
            ("${0:end}", "${0:end}"),
            ("${1:a} ${1:b}", "two defaults"),
            ("<b>$CLIPBOARD</b>", "CLIPBOARD"),
            ("${TM_FILENAME:x}", "a variable with a default"),
            ("${TM_FILENAME/(.*)\\..+$/$1/}", "without its extension"),
            ("$1 ${1/(a)/b/g}", "regular expression"),
            ("${1/(.*)/${1:/upcase}/}", "only ever seen transformed"),
            ("$whatever", "doesn't know"),
        ] {
            let why = refused(body);
            assert!(why.contains(says) && why.contains("--approximate"), "{body}: {why}");
        }
        assert!(refused("${1:a ${2:b}}").contains("inside placeholder 1"));
        assert!(!refused("${1:a ${2:b}}").contains("--approximate"), "no approximation for that");
        assert!(refused("${CLIPBOARD/(.*)/${1:/upcase}/}").contains("doesn't have"));
    }

    #[test]
    fn approximations_are_made_and_said() {
        assert_eq!(approximated("${1|one,two\\,three|}").0, "${1:one}");
        assert_eq!(approximated("fn ${0:body}").0, "fn ${1:body}$0");
        assert_eq!(approximated("${1:a} ${1:b}").0, "${1:a} $1");
        assert_eq!(approximated("<b>${CLIPBOARD:text}</b>$0").0, "<b>text</b>$0");
        assert_eq!(approximated("${TM_FILENAME/(.*)\\..+$/$1/}").0, "${FILE_STEM}");
        assert_eq!(approximated("${2:x} $name").0, "${2:x} ${3:name}");
        let (_, said) = approximated("${1|a,b|}");
        assert!(said[0].contains("no list to pick from"), "{said:?}");
    }

    /// What Fenix inserts is what VS Code would, defaults and all.
    #[test]
    fn fenix_inserts_what_vs_code_would() {
        for (vs, inserted) in [
            ("fn ${1:name}(${2:args}) {\n    ${0}\n}", "fn name(args) {\n    \n}"),
            ("price: \\$${1:5} \\\\o/ ${1}", "price: $5 \\o/ 5"),
            ("${1:a\\}b} $1", "a}b a}b"),
            ("// ${TM_FILENAME} ${CURRENT_YEAR}", "// <FILENAME> <YEAR>"),
            ("${1:word} ${1/(.*)/${1:/upcase}/}", "word WORD"),
        ] {
            let fenix = exact(vs);
            assert_eq!(fenix_render(&fenix).unwrap(), inserted, "{vs} -> {fenix}");
        }
        assert!(fenix_render("${1:a ${2:b}}").unwrap_err().contains("nested"));
        assert!(fenix_render("${NOPE}").unwrap_err().contains("unknown variable"));
        assert!(fenix_render("$0 $0").unwrap_err().contains("only one $0"));
    }

    fn opts(language: &str, approximate: bool, indent: Option<usize>) -> Options {
        Options { input: PathBuf::from("x.json"), language: language.into(), into: None, approximate, indent }
    }

    fn fixture() -> serde_json::Value {
        let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/snippets-import/sample.code-snippets")).unwrap();
        crate::jsonc::parse(&text).unwrap()
    }

    #[test]
    fn a_snippets_file_becomes_a_file_per_prefix() {
        let report = convert(&fixture(), &opts("rust", false, None)).unwrap();
        let paths: Vec<&str> = report.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["snippets/rust/fn.snippet", "snippets/rust/func.snippet", "snippets/rust/test.snippet", "snippets/rust/hdr.snippet"]);
        assert_eq!(report.files[0].text, "# name: Function\n# key: fn\n# scope: rust\n# --\nfn ${1:name}($2) {\n\t$0\n}");
        assert_eq!(report.files[1].text.lines().nth(1), Some("# key: func"));
        let refused: BTreeMap<&str, &str> = report.refused.iter().map(|(n, w)| (n.as_str(), w.as_str())).collect();
        assert!(refused["Pick"].contains("a choice"));
        assert!(refused["Python only"].contains("not rust"));
        assert!(refused["No prefix"].contains("no prefix"));
        assert!(refused["Spaced"].contains("no spaces"));
        assert!(refused["Again"].contains("Fenix expands one snippet per key"));
        assert!(report.notes.iter().any(|n| n.contains("description")));
        assert!(report.notes.iter().any(|n| n.contains("--indent")));
        for f in &report.files {
            let (header, body) = f.text.split_once("# --\n").unwrap();
            assert!(header.contains("# key: ") && !body.trim().is_empty());
            fenix_render(body).unwrap();
        }

        let report = convert(&fixture(), &opts("rust", true, Some(4))).unwrap();
        assert!(report.files.iter().any(|f| f.path == "snippets/rust/pick.snippet"));
        assert!(report.approximated.iter().any(|(n, a)| n == "Pick" && a.contains("first")));
        assert!(report.files[0].text.ends_with("{\n    $0\n}"), "{}", report.files[0].text);
        assert!(report.to_string().contains("Approximated"));
    }

    #[test]
    fn they_are_written_into_an_extension_and_never_over_a_file() {
        let dir = std::env::temp_dir().join(format!("xtask-snippets-import-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let report = convert(&fixture(), &opts("rust", false, None)).unwrap();
        assert!(write_into(&dir, &report).unwrap_err().contains("not an extension"));
        std::fs::write(dir.join("extension.toml"), "[extension]\n").unwrap();
        write_into(&dir, &report).unwrap();
        assert!(dir.join("snippets/rust/fn.snippet").is_file());
        let err = write_into(&dir, &report).unwrap_err();
        assert!(err.contains("already has") && err.contains("nothing was written"), "{err}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_command_line_is_read() {
        let o = options(&["s.json".into(), "--language".into(), "rust".into(), "--into=extensions/rust".into(), "--approximate".into(), "--indent".into(), "4".into()]).unwrap();
        assert_eq!(o, Options { input: "s.json".into(), language: "rust".into(), into: Some("extensions/rust".into()), approximate: true, indent: Some(4) });
        assert!(options(&["s.json".into()]).unwrap_err().contains("--language"));
        assert!(options(&["s.json".into(), "--language".into(), "Rust".into()]).is_err());
        assert!(options(&["s.json".into(), "--language".into(), "rust".into(), "--indent".into(), "0".into()]).is_err());
    }
}
