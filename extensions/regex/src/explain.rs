//! The pattern explained piece by piece, from `regex-syntax`'s AST: each
//! piece's text and what it matches, nested as the pattern nests. What
//! the AST can't hold -- fancy-regex's lookaround and backreferences --
//! is read through [`syntax::readable`] and named by where it starts.
//! On the way it notes what the pattern uses, for the flavour notes.

use std::collections::HashMap;
use std::ops::Range;

use regex_syntax::ast::{self, Ast, ClassSet, ClassSetItem, GroupKind, RepetitionKind, RepetitionRange};

use crate::engine::{Flags, Flavour};
use crate::syntax::{self, Special};

/// One row of the explanation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    /// How deep it's nested.
    pub depth: usize,
    /// Where it is in the pattern, in characters.
    pub at: Range<usize>,
    /// Its text in the pattern.
    pub code: String,
    pub what: String,
    /// The group it opens, for its colour.
    pub group: Option<u32>,
}

/// What a pattern uses that flavours treat differently.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Features {
    pub lookahead: bool,
    pub lookbehind: bool,
    pub atomic: bool,
    pub backref: bool,
    /// `(?P<name>` and `(?<name>`.
    pub named_python: bool,
    pub named_angle: bool,
    /// `\d`, `\w`, `\s`, `\b`: Unicode-aware here.
    pub perl_class: bool,
    pub word_boundary: bool,
    /// `\b{start}`, `\<`, `\>`, `\b{start-half}`.
    pub word_edges: bool,
    /// `\p{...}`.
    pub unicode_class: bool,
    /// `[[:alpha:]]`.
    pub posix_class: bool,
    /// `[a[b]]`, `[a&&b]`, `[a--b]`, `[a~~b]`.
    pub nested_class: bool,
    pub class_ops: bool,
    /// `\A`, `\z`.
    pub text_anchors: bool,
    /// `$`.
    pub dollar: bool,
    pub lazy: bool,
    /// `(?i)`, `(?i:...)`.
    pub inline_flags: bool,
    /// `U`, `R` inline: only Rust's.
    pub rust_only_flags: bool,
    /// `\x{...}`, `\u{...}`.
    pub hex_brace: bool,
}

/// The explanation, and what the pattern uses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Explained {
    pub pieces: Vec<Piece>,
    pub features: Features,
}

/// The flags in force where the walk is.
#[derive(Debug, Clone, Copy)]
struct Ctx {
    i: bool,
    m: bool,
    s: bool,
    swap_greed: bool,
    crlf: bool,
}

struct Walk<'a> {
    pattern: &'a str,
    chars: Vec<usize>,
    /// fancy-regex's constructs, by the byte they start at.
    special: HashMap<usize, Special>,
    pieces: Vec<Piece>,
    features: Features,
}

/// `pattern` explained, with `flags` as its starting flags. With the
/// fancy flavour lookaround and backreferences are explained too; an
/// error is for the engine to place, so it gives none here.
pub fn explain(pattern: &str, flags: Flags, flavour: Flavour) -> Option<Explained> {
    let found = syntax::scan(pattern, flags.x);
    let mut features = Features::default();
    for f in &found {
        match f.what {
            Special::Named { python: true } => features.named_python = true,
            Special::Named { python: false } => features.named_angle = true,
            _ => {}
        }
    }
    let readable = match flavour {
        Flavour::Fancy => syntax::readable(pattern, &found),
        Flavour::Rust => pattern.to_string(),
    };
    let ast = ast::parse::ParserBuilder::new().ignore_whitespace(flags.x).build().parse(&readable).ok()?;
    let special = match flavour {
        Flavour::Fancy => found.into_iter().filter(|f| !matches!(f.what, Special::Named { .. })).map(|f| (f.at.start, f.what)).collect(),
        Flavour::Rust => HashMap::new(),
    };
    let mut walk = Walk { pattern, chars: pattern.char_indices().map(|(b, _)| b).collect(), special, pieces: Vec::new(), features };
    let mut ctx = Ctx { i: flags.i, m: flags.m, s: flags.s, swap_greed: false, crlf: false };
    walk.node(&ast, 0, &mut ctx);
    Some(Explained { pieces: walk.pieces, features: walk.features })
}

fn quoted(c: char) -> String {
    match c {
        '\n' => "a newline".into(),
        '\t' => "a tab".into(),
        '\r' => "a carriage return".into(),
        ' ' => "a space".into(),
        '\u{7}' => "the bell character".into(),
        '\u{c}' => "a form feed".into(),
        '\u{b}' => "a vertical tab".into(),
        c if c.is_control() => format!("the character U+{:04X}", c as u32),
        c => format!("'{c}'"),
    }
}

/// A Unicode class's name, said as what it is.
fn unicode_name(name: &str) -> String {
    let said = match name {
        "L" | "Letter" => "a letter",
        "Lu" | "Uppercase_Letter" | "Uppercase" => "an uppercase letter",
        "Ll" | "Lowercase_Letter" | "Lowercase" => "a lowercase letter",
        "N" | "Number" => "a number",
        "Nd" | "Decimal_Number" | "Digit" => "a decimal digit",
        "P" | "Punctuation" => "punctuation",
        "S" | "Symbol" => "a symbol",
        "Sc" | "Currency_Symbol" => "a currency symbol",
        "Z" | "Separator" => "a separator",
        "Zs" | "Space_Separator" => "a space separator",
        "M" | "Mark" => "a combining mark",
        "C" | "Other" => "a control or unassigned character",
        "Emoji" => "an emoji",
        "Alphabetic" => "an alphabetic character",
        "White_Space" | "Whitespace" => "whitespace",
        _ => return format!("a {name} character"),
    };
    said.to_string()
}

impl Walk<'_> {
    fn chars(&self, span: &ast::Span) -> Range<usize> {
        let at = |b: usize| self.chars.partition_point(|&c| c < b);
        at(span.start.offset)..at(span.end.offset)
    }

    fn code(&self, span: &ast::Span) -> String {
        self.pattern.get(span.start.offset..span.end.offset).unwrap_or("").to_string()
    }

    fn push(&mut self, depth: usize, span: &ast::Span, what: String, group: Option<u32>) {
        let (at, code) = (self.chars(span), self.code(span));
        self.pieces.push(Piece { depth, at, code, what, group });
    }

    /// What a single-character piece matches, as a noun ("a digit"); none
    /// for a piece of several (a group, a sequence).
    fn atom(&mut self, ast: &Ast, ctx: &Ctx) -> Option<String> {
        if let Some(Special::Backref(name)) = self.special.get(&ast.span().start.offset) {
            self.features.backref = true;
            let name = name.clone();
            return Some(match name.parse::<u32>() {
                Ok(n) => format!("the text group {n} matched, again"),
                Err(_) => format!("the text group '{name}' matched, again"),
            });
        }
        Some(match ast {
            Ast::Literal(l) => self.literal(l, ctx),
            Ast::Dot(_) => match (ctx.s, ctx.crlf) {
                (true, _) => "any character".into(),
                (false, true) => "any character but \\r or \\n".into(),
                (false, false) => "any character but a newline".into(),
            },
            Ast::ClassPerl(c) => self.perl(c),
            Ast::ClassUnicode(c) => self.unicode(c),
            Ast::ClassBracketed(c) => {
                let set = self.set(&c.kind);
                if c.negated {
                    format!("any character but {set}")
                } else {
                    format!("one of {set}")
                }
            }
            _ => return None,
        })
    }

    fn literal(&mut self, l: &ast::Literal, ctx: &Ctx) -> String {
        if matches!(l.kind, ast::LiteralKind::HexBrace(_)) {
            self.features.hex_brace = true;
        }
        let any_case = ctx.i && l.c.to_lowercase().ne(l.c.to_uppercase());
        format!("{}{}", quoted(l.c), if any_case { " in either case" } else { "" })
    }

    fn perl(&mut self, c: &ast::ClassPerl) -> String {
        self.features.perl_class = true;
        let what = match c.kind {
            ast::ClassPerlKind::Digit => "a digit",
            ast::ClassPerlKind::Space => "whitespace",
            ast::ClassPerlKind::Word => "a word character (letter, digit, _)",
        };
        if c.negated {
            format!("anything but {what}")
        } else {
            what.to_string()
        }
    }

    fn unicode(&mut self, c: &ast::ClassUnicode) -> String {
        self.features.unicode_class = true;
        let what = match &c.kind {
            ast::ClassUnicodeKind::OneLetter(l) => unicode_name(&l.to_string()),
            ast::ClassUnicodeKind::Named(n) => unicode_name(n),
            ast::ClassUnicodeKind::NamedValue { name, value, .. } => format!("a character whose {name} is {value}"),
        };
        if c.is_negated() {
            format!("anything but {what}")
        } else {
            what
        }
    }

    /// A bracketed class's members: "a-z, '_' or a digit".
    fn set(&mut self, set: &ClassSet) -> String {
        match set {
            ClassSet::Item(item) => {
                let items = self.items(item);
                match items.len() {
                    0 => "nothing".into(),
                    1 => items[0].clone(),
                    n => format!("{} or {}", items[..n - 1].join(", "), items[n - 1]),
                }
            }
            ClassSet::BinaryOp(op) => {
                self.features.class_ops = true;
                let (l, r) = (self.set(&op.lhs), self.set(&op.rhs));
                match op.kind {
                    ast::ClassSetBinaryOpKind::Intersection => format!("[{l}] that is also [{r}]"),
                    ast::ClassSetBinaryOpKind::Difference => format!("[{l}] but not [{r}]"),
                    ast::ClassSetBinaryOpKind::SymmetricDifference => format!("[{l}] or [{r}] but not both"),
                }
            }
        }
    }

    fn items(&mut self, item: &ClassSetItem) -> Vec<String> {
        match item {
            ClassSetItem::Empty(_) => vec![],
            ClassSetItem::Literal(l) => vec![quoted(l.c)],
            ClassSetItem::Range(r) => vec![format!("{}-{}", r.start.c, r.end.c)],
            ClassSetItem::Ascii(a) => {
                self.features.posix_class = true;
                let name = format!("{:?}", a.kind).to_lowercase();
                vec![if a.negated { format!("not [:{name}:]") } else { format!("[:{name}:]") }]
            }
            ClassSetItem::Unicode(u) => vec![self.unicode(u)],
            ClassSetItem::Perl(p) => vec![self.perl(p)],
            ClassSetItem::Bracketed(b) => {
                self.features.nested_class = true;
                let set = self.set(&b.kind);
                vec![if b.negated { format!("anything but [{set}]") } else { format!("[{set}]") }]
            }
            ClassSetItem::Union(u) => u.items.iter().flat_map(|i| self.items(i)).collect(),
        }
    }

    fn flags_said(&mut self, flags: &ast::Flags) -> String {
        self.features.inline_flags = true;
        let mut on = Vec::new();
        let mut off = Vec::new();
        let mut negated = false;
        for item in &flags.items {
            let flag = match item.kind {
                ast::FlagsItemKind::Negation => {
                    negated = true;
                    continue;
                }
                ast::FlagsItemKind::Flag(f) => f,
            };
            let name = match flag {
                ast::Flag::CaseInsensitive => "case-insensitive",
                ast::Flag::MultiLine => "multi-line",
                ast::Flag::DotMatchesNewLine => ". matches newlines",
                ast::Flag::SwapGreed => "lazy by default",
                ast::Flag::Unicode => "Unicode",
                ast::Flag::CRLF => "CRLF lines",
                ast::Flag::IgnoreWhitespace => "extended",
            };
            if matches!(flag, ast::Flag::SwapGreed | ast::Flag::CRLF) {
                self.features.rust_only_flags = true;
            }
            if negated { off.push(name) } else { on.push(name) }
        }
        let mut said = Vec::new();
        if !on.is_empty() {
            said.push(format!("{} on", on.join(", ")));
        }
        if !off.is_empty() {
            said.push(format!("{} off", off.join(", ")));
        }
        said.join("; ")
    }

    fn set_flags(flags: &ast::Flags, ctx: &mut Ctx) {
        for (flag, state) in [ast::Flag::CaseInsensitive, ast::Flag::MultiLine, ast::Flag::DotMatchesNewLine, ast::Flag::SwapGreed, ast::Flag::CRLF].map(|f| (f, flags.flag_state(f))) {
            let Some(on) = state else { continue };
            match flag {
                ast::Flag::CaseInsensitive => ctx.i = on,
                ast::Flag::MultiLine => ctx.m = on,
                ast::Flag::DotMatchesNewLine => ctx.s = on,
                ast::Flag::SwapGreed => ctx.swap_greed = on,
                ast::Flag::CRLF => ctx.crlf = on,
                _ => {}
            }
        }
    }

    fn assertion(&mut self, a: &ast::Assertion, ctx: &Ctx) -> String {
        use ast::AssertionKind as K;
        match a.kind {
            K::StartLine => if ctx.m { "the start of a line" } else { "the start of the text" }.into(),
            K::EndLine => {
                self.features.dollar = true;
                if ctx.m { "the end of a line" } else { "the end of the text" }.into()
            }
            K::StartText => {
                self.features.text_anchors = true;
                "the very start of the text, whatever the flags".into()
            }
            K::EndText => {
                self.features.text_anchors = true;
                "the very end of the text, whatever the flags".into()
            }
            K::WordBoundary => {
                self.features.word_boundary = true;
                "a word boundary".into()
            }
            K::NotWordBoundary => {
                self.features.word_boundary = true;
                "anywhere but a word boundary".into()
            }
            K::WordBoundaryStart | K::WordBoundaryStartAngle => {
                self.features.word_edges = true;
                "the start of a word".into()
            }
            K::WordBoundaryEnd | K::WordBoundaryEndAngle => {
                self.features.word_edges = true;
                "the end of a word".into()
            }
            K::WordBoundaryStartHalf => {
                self.features.word_edges = true;
                "where a word could start (not after a word character)".into()
            }
            K::WordBoundaryEndHalf => {
                self.features.word_edges = true;
                "where a word could end (not before a word character)".into()
            }
        }
    }

    fn times(&mut self, rep: &ast::Repetition, ctx: &Ctx) -> String {
        let n = match &rep.op.kind {
            RepetitionKind::ZeroOrOne => "optionally".to_string(),
            RepetitionKind::ZeroOrMore => "0 or more times".to_string(),
            RepetitionKind::OneOrMore => "1 or more times".to_string(),
            RepetitionKind::Range(RepetitionRange::Exactly(n)) => format!("exactly {n} time{}", if *n == 1 { "" } else { "s" }),
            RepetitionKind::Range(RepetitionRange::AtLeast(n)) => format!("{n} or more times"),
            RepetitionKind::Range(RepetitionRange::Bounded(a, b)) => format!("{a} to {b} times"),
        };
        let exact = matches!(rep.op.kind, RepetitionKind::Range(RepetitionRange::Exactly(_)));
        let greedy = rep.greedy != ctx.swap_greed;
        if !greedy {
            self.features.lazy = true;
        }
        if greedy || exact { n } else { format!("{n}, as few as can be (lazy)") }
    }

    fn node(&mut self, ast: &Ast, depth: usize, ctx: &mut Ctx) {
        if let Some(what) = self.atom(ast, ctx) {
            return self.push(depth, ast.span(), what, None);
        }
        match ast {
            Ast::Empty(_) => {}
            Ast::Flags(f) => {
                let said = self.flags_said(&f.flags);
                Self::set_flags(&f.flags, ctx);
                self.push(depth, &f.span, format!("from here on: {said}"), None);
            }
            Ast::Assertion(a) => {
                let what = self.assertion(a, ctx);
                self.push(depth, &a.span, what, None);
            }
            Ast::Repetition(rep) => {
                let times = self.times(rep, ctx);
                match self.atom(&rep.ast, ctx) {
                    Some(what) => self.push(depth, &rep.span, format!("{what}, {times}"), None),
                    None => {
                        self.push(depth, &rep.span, format!("{times}:"), None);
                        self.node(&rep.ast, depth + 1, ctx);
                    }
                }
            }
            Ast::Group(g) => self.group(g, depth, ctx),
            Ast::Alternation(alt) => {
                self.push(depth, &alt.span, format!("one of {} alternatives:", alt.asts.len()), None);
                for (k, branch) in alt.asts.iter().enumerate() {
                    match branch {
                        Ast::Concat(c) if c.asts.len() > 1 => {
                            self.push(depth + 1, &c.span, format!("alternative {}:", k + 1), None);
                            let head = self.pieces.len() - 1;
                            self.concat(&c.asts, depth + 2, &mut ctx.clone());
                            // A run of text is one piece: no heading.
                            if self.pieces.len() == head + 2 {
                                self.pieces.remove(head);
                                self.pieces[head].depth = depth + 1;
                            }
                        }
                        Ast::Empty(span) => self.push(depth + 1, span, format!("alternative {}: nothing (an empty match)", k + 1), None),
                        other => self.node(other, depth + 1, &mut ctx.clone()),
                    }
                }
            }
            Ast::Concat(c) => self.concat(&c.asts, depth, ctx),
            _ => {}
        }
    }

    /// A sequence: literals in a row said as one piece of text.
    fn concat(&mut self, asts: &[Ast], depth: usize, ctx: &mut Ctx) {
        let mut i = 0;
        while i < asts.len() {
            let run = asts[i..].iter().take_while(|a| matches!(a, Ast::Literal(_)) && !self.special.contains_key(&a.span().start.offset)).count();
            if run > 1 {
                let text: String = asts[i..i + run].iter().filter_map(|a| if let Ast::Literal(l) = a { Some(l.c) } else { None }).collect();
                let span = ast::Span::new(asts[i].span().start, asts[i + run - 1].span().end);
                let case = if ctx.i { ", in any case" } else { "" };
                self.push(depth, &span, format!("the text \"{}\"{case}", text.escape_debug()), None);
                i += run;
            } else {
                self.node(&asts[i], depth, ctx);
                i += 1;
            }
        }
    }

    fn group(&mut self, g: &ast::Group, depth: usize, ctx: &mut Ctx) {
        let mut inner = *ctx;
        let (what, group) = match (self.special.get(&g.span.start.offset).cloned(), &g.kind) {
            (Some(Special::Lookahead { negative }), _) => {
                self.features.lookahead = true;
                (if negative { "not followed by:" } else { "followed by (not taken):" }.to_string(), None)
            }
            (Some(Special::Lookbehind { negative }), _) => {
                self.features.lookbehind = true;
                (if negative { "not preceded by:" } else { "preceded by (not taken):" }.to_string(), None)
            }
            (Some(Special::Atomic), _) => {
                self.features.atomic = true;
                ("an atomic group, never backtracked into:".to_string(), None)
            }
            (_, GroupKind::CaptureIndex(n)) => (format!("group {n}:"), Some(*n)),
            (_, GroupKind::CaptureName { name, .. }) => (format!("group {} '{}':", name.index, name.name), Some(name.index)),
            (_, GroupKind::NonCapturing(flags)) if flags.items.is_empty() => ("a group, not captured:".to_string(), None),
            (_, GroupKind::NonCapturing(flags)) => {
                let said = self.flags_said(flags);
                Self::set_flags(flags, &mut inner);
                (format!("a group with {said}:"), None)
            }
        };
        self.push(depth, &g.span, what, group);
        self.node(&g.ast, depth + 1, &mut inner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(p: &str, flags: Flags, flavour: Flavour) -> Vec<String> {
        explain(p, flags, flavour).unwrap().pieces.iter().map(|p| format!("{}{} = {}", "  ".repeat(p.depth), p.code, p.what)).collect()
    }

    #[test]
    fn a_date_piece_by_piece() {
        assert_eq!(
            rows(r"^(?P<y>\d{4})-(\d\d)?$", Flags::default(), Flavour::Rust),
            [
                "^ = the start of the text",
                r"(?P<y>\d{4}) = group 1 'y':",
                r"  \d{4} = a digit, exactly 4 times",
                "- = '-'",
                r"(\d\d)? = optionally:",
                r"  (\d\d) = group 2:",
                r"    \d = a digit",
                r"    \d = a digit",
                "$ = the end of the text",
            ]
        );
    }

    #[test]
    fn literals_in_a_row_are_one_piece() {
        assert_eq!(rows("error: ", Flags::default(), Flavour::Rust), [r#"error:  = the text "error: ""#]);
        assert_eq!(rows("ab", Flags { i: true, ..Flags::default() }, Flavour::Rust), [r#"ab = the text "ab", in any case"#]);
    }

    #[test]
    fn classes_alternatives_and_lazy() {
        assert_eq!(rows("[a-z_]+?", Flags::default(), Flavour::Rust), ["[a-z_]+? = one of a-z or '_', 1 or more times, as few as can be (lazy)"]);
        assert_eq!(rows(r"[^\s,]", Flags::default(), Flavour::Rust), [r"[^\s,] = any character but whitespace or ','"]);
        assert_eq!(
            rows("cat|dog|", Flags::default(), Flavour::Rust),
            ["cat|dog| = one of 3 alternatives:", r#"  cat = the text "cat""#, r#"  dog = the text "dog""#, "   = alternative 3: nothing (an empty match)"]
        );
    }

    #[test]
    fn flags_follow_the_pattern() {
        let r = rows("(?m)^a(?s:.)", Flags::default(), Flavour::Rust);
        assert_eq!(r[0], "(?m) = from here on: multi-line on");
        assert_eq!(r[1], "^ = the start of a line");
        assert_eq!(r[3], "(?s:.) = a group with . matches newlines on:");
        assert_eq!(r[4], "  . = any character");
        assert_eq!(rows(".", Flags::default(), Flavour::Rust), [". = any character but a newline"]);
    }

    #[test]
    fn lookaround_and_backrefs_with_fancy() {
        let r = rows(r"(?<=\$)(\d+)(?!\.)\1", Flags::default(), Flavour::Fancy);
        assert_eq!(
            r,
            [
                r"(?<=\$) = preceded by (not taken):",
                r"  \$ = '$'",
                r"(\d+) = group 1:",
                r"  \d+ = a digit, 1 or more times",
                r"(?!\.) = not followed by:",
                r"  \. = '.'",
                r"\1 = the text group 1 matched, again",
            ]
        );
        let e = explain(r"(?<=\$)(\d+)(?!\.)\1", Flags::default(), Flavour::Fancy).unwrap();
        assert!(e.features.lookbehind && e.features.lookahead && e.features.backref && e.features.perl_class);
        assert!(explain(r"(?<=\$)", Flags::default(), Flavour::Rust).is_none(), "Rust's regex can't read it");
    }

    #[test]
    fn positions_are_characters() {
        let e = explain("é(x)", Flags::default(), Flavour::Rust).unwrap();
        assert_eq!(e.pieces[1].at, 1..4);
        assert_eq!(e.pieces[1].group, Some(1));
    }

    #[test]
    fn what_a_pattern_uses_is_noted() {
        let f = explain(r"(?<n>\p{L}+)[[:alpha:]&&[a-z]]\A\z\x{41}", Flags::default(), Flavour::Rust).unwrap().features;
        assert!(f.named_angle && f.unicode_class && f.posix_class && f.class_ops && f.text_anchors && f.hex_brace);
        assert!(!f.named_python && !f.lookahead);
    }
}
