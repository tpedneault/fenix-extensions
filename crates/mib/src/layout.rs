//! A telecommand's layout, as the ICD's `CDF` table gives it: its
//! elements in packet order -- fixed areas (`A`), fixed parameters (`F`)
//! and editable ones (`E`), `CDF_ELTYPE` -- and its repeated groups: a
//! repeater (`CDF_GRPSIZE` > 0) repeats the elements after it as many
//! times as its value. The group's size counts every element after the
//! repeater, fixed and spare ones included, and the elements of a group
//! nested in it.
//!
//! Positions (`CDF_BIT`) are given as if every group were there once:
//! a packet with a group repeated `n` times moves what follows by the
//! extra repetitions, one with no repetition at all by the group's
//! length; a variable-length value moves it by how much longer it is
//! than `CDF_ELLEN`.
//!
//! Everything that asks for, checks, builds or reads a telecommand's
//! arguments goes through this: the insert form, the scripts' checks,
//! completion, the generators, the packet builder and decoder.

use fenix_ccsds::bits;
use fenix_ccsds::pus::Profile;

use crate::packets::Raw;
use crate::row::Row;
use crate::set::{DefRef, Kind, MibSet};
use crate::types::parse_int;
use crate::values::{self, Domain, Interp, ParamType};

/// The most repetitions a group is built or read with: a counter read
/// from a corrupt packet, or mistyped, mustn't build a million fields.
pub const MAX_REPEAT: usize = 4096;

/// `CDF_ELTYPE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementKind {
    /// `A`: a fixed area -- spare bits, or a constant with no parameter.
    Area,
    /// `F`: a parameter whose value the definition fixes.
    Fixed,
    /// `E`: a parameter someone gives a value, `CDF_VALUE` its default.
    Editable,
}

impl ElementKind {
    pub fn of(flag: &str, has_param: bool) -> ElementKind {
        match flag.trim() {
            "A" | "a" => ElementKind::Area,
            "F" | "f" => ElementKind::Fixed,
            "E" | "e" => ElementKind::Editable,
            // Not one of the ICD's: a parameter is asked for, else it's an area.
            _ if has_param => ElementKind::Editable,
            _ => ElementKind::Area,
        }
    }
}

/// One `CDF` row.
#[derive(Debug, Clone)]
pub struct Element {
    pub kind: ElementKind,
    /// `CDF_PNAME`; empty for an area.
    pub name: String,
    pub description: String,
    /// `CDF_BIT`: from the start of the application data, every group
    /// there once.
    pub bit: Option<usize>,
    /// `CDF_ELLEN`, else the type's width.
    pub bits: usize,
    /// `CDF_VALUE`: a fixed element's value, an editable one's default.
    pub value: String,
    /// `CDF_INTER`: how `value` reads.
    pub inter: Interp,
    /// `CDF_TMID`: the TM parameter a dynamic default comes from.
    pub tmid: String,
    /// `CDF_GRPSIZE`: the elements after it it repeats.
    pub group: usize,
    /// Its `CPC` definition.
    pub param: Option<DefRef>,
    pub ty: Option<ParamType>,
    /// Its type with its calibrations, to turn values raw and back.
    pub domain: Option<Domain>,
    /// What someone is offered (`Layout::default`), worked out once.
    pub offered: String,
    /// A fixed repeater's count.
    pub fixed_count: Option<usize>,
    pub cdf: Row,
}

impl Element {
    pub fn is_repeater(&self) -> bool {
        self.group > 0 && self.kind != ElementKind::Area
    }

    /// Whether someone gives it a value.
    pub fn asked(&self) -> bool {
        self.kind == ElementKind::Editable && !self.name.is_empty()
    }

    /// What it's called on a page: its name, `spare` for an area.
    pub fn label(&self) -> &str {
        if self.name.is_empty() {
            "spare"
        } else {
            &self.name
        }
    }

    pub fn fixed_value(&self) -> Option<&str> {
        (self.kind != ElementKind::Editable).then_some(self.value.as_str())
    }
}

/// The element tree: an element, or a repeater with the elements it
/// repeats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Leaf(usize),
    Group { repeater: usize, body: Vec<Node> },
}

/// A telecommand's elements and groups.
#[derive(Debug, Clone)]
pub struct Layout {
    pub tc: DefRef,
    pub elements: Vec<Element>,
    pub nodes: Vec<Node>,
    /// What's wrong with the definition, for the checks.
    pub problems: Vec<String>,
}

/// One value someone gives: an editable element, in a repetition of
/// each group around it (`path`, 1-based, outermost first).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Field {
    pub element: usize,
    pub path: Vec<usize>,
}

impl Field {
    /// `NAME`, `NAME[2]`, `NAME[2.1]`.
    pub fn label(&self, layout: &Layout) -> String {
        let name = layout.elements[self.element].label();
        if self.path.is_empty() {
            name.to_string()
        } else {
            format!("{name}[{}]", self.path.iter().map(|n| n.to_string()).collect::<Vec<_>>().join("."))
        }
    }

    /// The key a value is kept under.
    pub fn key(&self, layout: &Layout) -> String {
        let path: Vec<String> = self.path.iter().map(|n| n.to_string()).collect();
        format!("{}@{}", layout.elements[self.element].name, path.join("."))
    }
}

/// A value read from a packet, or why it couldn't be: (element, path,
/// reason).
pub type Read = Result<Placed, (usize, Vec<usize>, String)>;

/// A value where a packet has it.
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    pub element: usize,
    pub path: Vec<usize>,
    /// From the start of the application data.
    pub bit: usize,
    pub bits: usize,
    pub raw: Raw,
}

fn build(elements: &[Element], start: usize, end: usize, problems: &mut Vec<String>) -> Vec<Node> {
    let mut out = Vec::new();
    let mut i = start;
    while i < end {
        let el = &elements[i];
        if el.is_repeater() {
            let mut body_end = i + 1 + el.group;
            if body_end > end {
                problems.push(format!("{} repeats the next {} elements, but only {} follow it{}", el.label(), el.group, end - i - 1, if end < elements.len() { " in its group" } else { "" }));
                body_end = end;
            }
            let body = build(elements, i + 1, body_end, problems);
            out.push(Node::Group { repeater: i, body });
            i = body_end;
        } else {
            if el.group > 0 {
                problems.push(format!("a fixed area has a group size of {}: only a parameter repeats a group", el.group));
            }
            out.push(Node::Leaf(i));
            i += 1;
        }
    }
    out
}

/// The leaves of `nodes`, repeaters included, in order.
fn leaves(nodes: &[Node], out: &mut Vec<usize>) {
    for n in nodes {
        match n {
            Node::Leaf(i) => out.push(*i),
            Node::Group { repeater, body } => {
                out.push(*repeater);
                leaves(body, out);
            }
        }
    }
}

impl Layout {
    /// `tc`'s layout, from its `CDF` rows in its own MIB.
    pub fn of(set: &MibSet, tc: DefRef) -> Layout {
        let e = set.get(tc);
        let rows: Vec<&Row> = set.index().rows_by_field("cdf", "CDF_CNAME", &e.name, Some(e.root));
        let mut elements: Vec<Element> = rows
            .into_iter()
            .map(|cdf| {
                let name = cdf.clean("CDF_PNAME").to_string();
                let param = (!name.is_empty()).then(|| set.find(Kind::TcParam, e.root, &name)).flatten();
                let ty = param.map(|p| ParamType::of(set, p));
                let description = match cdf.clean("CDF_DESCR") {
                    "" => param.map(|p| set.get(p).row.clean("CPC_DESCR").to_string()).unwrap_or_default(),
                    d => d.to_string(),
                };
                let bits = parse_int(cdf.clean("CDF_ELLEN")).filter(|n| *n >= 0).map(|n| n as usize).or_else(|| ty.as_ref().and_then(ParamType::bits)).unwrap_or(0);
                Element {
                    kind: ElementKind::of(cdf.clean("CDF_ELTYPE"), !name.is_empty()),
                    name,
                    description,
                    bit: parse_int(cdf.clean("CDF_BIT")).filter(|n| *n >= 0).map(|n| n as usize),
                    bits,
                    value: cdf.clean("CDF_VALUE").to_string(),
                    inter: Interp::of(cdf.clean("CDF_INTER")),
                    tmid: cdf.clean("CDF_TMID").to_string(),
                    group: parse_int(cdf.clean("CDF_GRPSIZE")).unwrap_or(0).max(0) as usize,
                    param,
                    domain: ty.as_ref().map(|t| Domain::of(set, t)),
                    ty,
                    offered: String::new(),
                    fixed_count: None,
                    cdf: cdf.clone(),
                }
            })
            .collect();
        // In packet order: the file's, unless every element says where it is.
        if elements.iter().all(|el| el.bit.is_some()) {
            elements.sort_by_key(|el| el.bit);
        }
        let mut problems = Vec::new();
        let nodes = build(&elements, 0, elements.len(), &mut problems);
        for el in elements.iter_mut() {
            el.offered = offered(set, el);
            el.fixed_count = fixed_count(el);
        }
        for el in &elements {
            if el.kind != ElementKind::Area && el.name.is_empty() {
                problems.push(format!("a {} element names no parameter", if el.kind == ElementKind::Fixed { "fixed" } else { "editable" }));
            }
            if el.kind == ElementKind::Fixed && el.value.is_empty() {
                problems.push(format!("{} is fixed but has no value", el.label()));
            }
            if el.is_repeater() {
                if let Some(t) = &el.ty {
                    if !matches!(t.ptc, 2 | 3) {
                        problems.push(format!("{} repeats a group, but is a {} -- a repeater is an unsigned integer", el.name, t.type_name()));
                    }
                }
            }
        }
        Layout { tc, elements, nodes, problems }
    }

    /// Whether any group repeats.
    pub fn grouped(&self) -> bool {
        self.nodes.iter().any(|n| matches!(n, Node::Group { .. }))
    }

    /// Every element, repeaters included, in order.
    pub fn order(&self) -> Vec<usize> {
        let mut out = Vec::new();
        leaves(&self.nodes, &mut out);
        out
    }

    /// The elements someone gives values, once each.
    pub fn asked(&self) -> impl Iterator<Item = &Element> {
        self.elements.iter().filter(|el| el.asked())
    }

    /// The group `element` repeats in, innermost first: the repeaters.
    pub fn enclosing(&self, element: usize) -> Vec<usize> {
        fn walk(nodes: &[Node], target: usize, stack: &mut Vec<usize>) -> bool {
            for n in nodes {
                match n {
                    Node::Leaf(i) if *i == target => return true,
                    Node::Leaf(_) => {}
                    Node::Group { repeater, body } => {
                        if *repeater == target {
                            return true;
                        }
                        stack.push(*repeater);
                        if walk(body, target, stack) {
                            return true;
                        }
                        stack.pop();
                    }
                }
            }
            false
        }
        let mut stack = Vec::new();
        walk(&self.nodes, element, &mut stack);
        stack.reverse();
        stack
    }

    /// An editable element's default, as someone types it: `CDF_VALUE`,
    /// else `CPC_DEFVAL`, each read as its interpretation says. Empty
    /// for a dynamic default (`dynamic_default` says where from).
    pub fn default(&self, _set: &MibSet, element: usize) -> String {
        self.elements[element].offered.clone()
    }

    /// The TM parameter a dynamic default is read from.
    pub fn dynamic_default(&self, element: usize) -> Option<&str> {
        let el = &self.elements[element];
        (el.inter == Interp::Dynamic && !el.tmid.is_empty()).then_some(el.tmid.as_str())
    }

    /// Every value someone gives, as `count` (a repeater's field, read
    /// from what's been given) expands the groups. A fixed repeater
    /// repeats as its value says.
    pub fn fields(&self, _set: &MibSet, count: impl FnMut(&Field) -> usize) -> Vec<Field> {
        self.fields_by(count)
    }

    /// `fields`, without the set: everything it needs is read already.
    pub fn fields_by(&self, mut count: impl FnMut(&Field) -> usize) -> Vec<Field> {
        let mut out = Vec::new();
        self.expand(&self.nodes, &[], &mut count, &mut out);
        out
    }

    fn expand(&self, nodes: &[Node], path: &[usize], count: &mut dyn FnMut(&Field) -> usize, out: &mut Vec<Field>) {
        for n in nodes {
            if out.len() >= MAX_REPEAT {
                return;
            }
            match n {
                Node::Leaf(i) => {
                    if self.elements[*i].asked() {
                        out.push(Field { element: *i, path: path.to_vec() });
                    }
                }
                Node::Group { repeater, body } => {
                    let times = match self.elements[*repeater].fixed_count {
                        Some(n) => n,
                        None if self.elements[*repeater].asked() => {
                            let f = Field { element: *repeater, path: path.to_vec() };
                            let n = count(&f).min(MAX_REPEAT);
                            out.push(f);
                            n
                        }
                        None => 0,
                    };
                    for rep in 1..=times {
                        let mut p = path.to_vec();
                        p.push(rep);
                        self.expand(body, &p, count, out);
                    }
                }
            }
        }
    }

    /// The bits a node takes as the definition places it (every group
    /// once): from its first element's start to its last's end.
    fn static_span(&self, nodes: &[Node]) -> Option<(usize, usize)> {
        let mut ids = Vec::new();
        leaves(nodes, &mut ids);
        let start = ids.iter().filter_map(|&i| self.elements[i].bit).min()?;
        let end = ids.iter().filter_map(|&i| Some(self.elements[i].bit? + self.elements[i].bits)).max()?;
        Some((start, end))
    }

    /// The raw value of a fixed element.
    fn fixed_raw(&self, _set: &MibSet, el: &Element, profile: &Profile) -> Result<Raw, String> {
        match &el.domain {
            Some(d) => d.mib_raw(&el.value, &el.inter, profile),
            None if el.value.is_empty() => Ok(Raw::Unsigned(0)),
            None => values::parse_raw_int(&el.value, values::Radix::Decimal)
                .map(|n| if n < 0 { Raw::Signed(n as i64) } else { Raw::Unsigned(n as u64) })
                .ok_or_else(|| format!("{}'s fixed value {} isn't a number", el.label(), el.value)),
        }
    }

    /// The application data for `args` -- `(parameter, value)` for each
    /// field in `fields`' order -- and where each value went.
    pub fn encode(&self, set: &MibSet, args: &[(String, String)], profile: &Profile) -> Result<(Vec<u8>, Vec<Placed>), String> {
        let mut st = Writer { set, profile, args, next: 0, out: Vec::new(), placed: Vec::new(), cursor: 0, referenced: None };
        self.write(&mut st, &self.nodes, &[], 0)?;
        if st.next < args.len() {
            let extra: Vec<&str> = args[st.next..].iter().map(|(n, _)| n.as_str()).collect();
            return Err(format!("more values than {} takes: {}", set.get(self.tc).name, extra.join(", ")));
        }
        let end = st.placed.iter().map(|p| p.bit + p.bits).max().unwrap_or(0);
        let mut bytes = st.out;
        bytes.resize(end.div_ceil(8), 0);
        Ok((bytes, st.placed))
    }

    /// Writes `nodes` with every static position moved by `shift`;
    /// returns the shift after them.
    fn write(&self, st: &mut Writer, nodes: &[Node], path: &[usize], mut shift: i64) -> Result<i64, String> {
        for n in nodes {
            match n {
                Node::Leaf(i) => shift = self.write_element(st, *i, path, shift)?.0,
                Node::Group { repeater, body } => {
                    let (s, raw) = self.write_element(st, *repeater, path, shift)?;
                    shift = s;
                    let times = raw.as_f64().unwrap_or(0.0).max(0.0) as usize;
                    if times > MAX_REPEAT {
                        return Err(format!("{} repeats {times} times, more than {MAX_REPEAT}", self.elements[*repeater].name));
                    }
                    let len = self.static_span(body).map(|(a, b)| (b - a) as i64).unwrap_or(0);
                    if times == 0 {
                        shift -= len;
                    }
                    for rep in 1..=times {
                        let mut p = path.to_vec();
                        p.push(rep);
                        let after = self.write(st, body, &p, shift)?;
                        shift = if rep < times { after + len } else { after };
                    }
                }
            }
        }
        Ok(shift)
    }

    fn write_element(&self, st: &mut Writer, i: usize, path: &[usize], shift: i64) -> Result<(i64, Raw), String> {
        let el = &self.elements[i];
        let mut deduced_bits = None;
        let raw = if el.asked() {
            let (name, value) = st.args.get(st.next).ok_or_else(|| format!("no value for {}", Field { element: i, path: path.to_vec() }.label(self)))?;
            if name != &el.name {
                return Err(format!("expected {}, got {name}", Field { element: i, path: path.to_vec() }.label(self)));
            }
            st.next += 1;
            let d = self.domain_for(st, el)?;
            if d.ty.deduced() || el.ty.as_ref().is_some_and(ParamType::deduced) {
                deduced_bits = d.ty.bits();
            }
            d.to_raw(value, st.profile)?
        } else {
            self.fixed_raw(st.set, el, st.profile)?
        };
        if el.ty.as_ref().is_some_and(ParamType::names_a_parameter) {
            st.referenced = Some(raw.clone());
        }
        let at = match el.bit {
            Some(b) => (b as i64 + shift).max(0) as usize,
            None => st.cursor,
        };
        let t = el.ty.as_ref();
        let variable = t.is_some_and(ParamType::variable_length);
        let bytes_of = |raw: &Raw| -> Option<Vec<u8>> {
            match raw {
                Raw::Bytes(b) => Some(b.clone()),
                Raw::Text(s) => Some(s.as_bytes().to_vec()),
                _ => None,
            }
        };
        let width = match (bytes_of(&raw), variable) {
            (Some(b), true) => {
                let n = b.len().min(255);
                bits::put(&mut st.out, at, 8, n as u64);
                put_bytes(&mut st.out, at + 8, &b[..n]);
                8 + n * 8
            }
            (Some(b), false) => {
                // Fixed length: padded or cut to it.
                let len = if el.bits > 0 { el.bits.div_ceil(8) } else { b.len() };
                let mut b = b;
                b.resize(len, 0);
                put_bytes(&mut st.out, at, &b);
                el.bits.max(len * 8)
            }
            (None, _) => {
                // A deduced value is as wide as the type it takes.
                let width = deduced_bits.unwrap_or(el.bits);
                let v = match raw {
                    Raw::Unsigned(v) => v,
                    Raw::Signed(v) => v as u64,
                    Raw::Real(f) if width == 32 => (f as f32).to_bits() as u64,
                    Raw::Real(f) => f.to_bits(),
                    _ => 0,
                };
                bits::put(&mut st.out, at, width.min(64), v & mask(width));
                width
            }
        };
        st.cursor = at + width;
        st.placed.push(Placed { element: i, path: path.to_vec(), bit: at, bits: width, raw: raw.clone() });
        let shift = if el.bit.is_some() { shift + width as i64 - el.bits as i64 } else { shift };
        Ok((shift, raw))
    }

    /// A deduced parameter's type: the TM parameter's the last argument
    /// naming one gave.
    fn domain_for(&self, st: &Writer, el: &Element) -> Result<Domain, String> {
        let d = el.domain.clone().ok_or_else(|| format!("{} has no CPC definition", el.name))?;
        if !d.ty.deduced() {
            return Ok(d);
        }
        let named = st.referenced.as_ref().ok_or_else(|| format!("{} is deduced, but no argument before it names a parameter", el.name))?;
        deduced_type(st.set, d.ty.root, named).map(Domain::plain).ok_or_else(|| format!("{} is deduced, but {} names no parameter the MIB has", el.name, named.display()))
    }

    /// The values of the application data `app`, as `encode` would have
    /// placed them; a repeater's count is read from the packet.
    pub fn decode(&self, set: &MibSet, app: &[u8], profile: &Profile) -> Vec<Read> {
        let _ = profile;
        let mut st = Reader { set, app, out: Vec::new(), cursor: 0, referenced: None };
        self.read(&mut st, &self.nodes, &[], 0);
        st.out
    }

    fn read(&self, st: &mut Reader, nodes: &[Node], path: &[usize], mut shift: i64) -> i64 {
        for n in nodes {
            match n {
                Node::Leaf(i) => match self.read_element(st, *i, path, shift) {
                    Some((s, _)) => shift = s,
                    None => return shift,
                },
                Node::Group { repeater, body } => {
                    let Some((s, raw)) = self.read_element(st, *repeater, path, shift) else { return shift };
                    shift = s;
                    let times = (raw.as_f64().unwrap_or(0.0).max(0.0) as usize).min(MAX_REPEAT);
                    let len = self.static_span(body).map(|(a, b)| (b - a) as i64).unwrap_or(0);
                    if times == 0 {
                        shift -= len;
                    }
                    for rep in 1..=times {
                        let mut p = path.to_vec();
                        p.push(rep);
                        let after = self.read(st, body, &p, shift);
                        shift = if rep < times { after + len } else { after };
                        if st.out.last().is_some_and(Result::is_err) {
                            return shift;
                        }
                    }
                }
            }
        }
        shift
    }

    fn read_element(&self, st: &mut Reader, i: usize, path: &[usize], shift: i64) -> Option<(i64, Raw)> {
        let el = &self.elements[i];
        let at = match el.bit {
            Some(b) => (b as i64 + shift).max(0) as usize,
            None => st.cursor,
        };
        let mut t = el.ty.clone();
        if t.as_ref().is_some_and(ParamType::deduced) {
            t = st.referenced.as_ref().and_then(|r| deduced_type(st.set, el.ty.as_ref().unwrap().root, r));
        }
        let fail = |st: &mut Reader, why: &str| {
            st.out.push(Err((i, path.to_vec(), why.to_string())));
            None
        };
        let (raw, width) = match t.as_ref() {
            Some(t) if t.variable_length() => {
                let Some(n) = bits::get(st.app, at, 8) else { return fail(st, "past the end of the packet") };
                let n = n as usize;
                let Some(b) = get_bytes(st.app, at + 8, n) else { return fail(st, "past the end of the packet") };
                (if t.ptc == 8 { Raw::Text(String::from_utf8_lossy(&b).to_string()) } else { Raw::Bytes(b) }, 8 + n * 8)
            }
            Some(t) if matches!(t.ptc, 7..=10) => {
                let n = el.bits.div_ceil(8);
                let Some(b) = get_bytes(st.app, at, n) else { return fail(st, "past the end of the packet") };
                (if t.ptc == 8 { Raw::Text(String::from_utf8_lossy(&b).trim_end_matches('\0').to_string()) } else { Raw::Bytes(b) }, el.bits)
            }
            _ => {
                let width = if el.ty.as_ref().is_some_and(ParamType::deduced) { t.as_ref().and_then(ParamType::bits).unwrap_or(el.bits) } else { el.bits };
                let Some(v) = bits::get(st.app, at, width.min(64)) else { return fail(st, "past the end of the packet") };
                let raw = match t.as_ref().map(|t| t.ptc) {
                    Some(4) => Raw::Signed(bits::signed(v, width)),
                    Some(5) if width == 32 => Raw::Real(f32::from_bits(v as u32) as f64),
                    Some(5) if width == 64 => Raw::Real(f64::from_bits(v)),
                    _ => Raw::Unsigned(v),
                };
                (raw, width)
            }
        };
        if el.ty.as_ref().is_some_and(ParamType::names_a_parameter) {
            st.referenced = Some(raw.clone());
        }
        st.cursor = at + width;
        st.out.push(Ok(Placed { element: i, path: path.to_vec(), bit: at, bits: width, raw: raw.clone() }));
        let shift = if el.bit.is_some() { shift + width as i64 - el.bits as i64 } else { shift };
        Some((shift, raw))
    }

    /// Elements whose place doesn't depend on any value: before the first
    /// group or variable-length value.
    pub fn static_prefix(&self) -> Vec<usize> {
        let mut out = Vec::new();
        for n in &self.nodes {
            match n {
                Node::Leaf(i) => {
                    let el = &self.elements[*i];
                    out.push(*i);
                    if el.ty.as_ref().is_some_and(|t| t.variable_length() || t.deduced()) || el.bit.is_none() {
                        break;
                    }
                }
                Node::Group { repeater, .. } => {
                    out.push(*repeater);
                    break;
                }
            }
        }
        out
    }

    /// Where `nodes` sit with every group there once: their first
    /// element's start and their last one's end, in bits.
    pub fn static_extent(&self, nodes: &[Node]) -> Option<(usize, usize)> {
        self.static_span(nodes)
    }

    /// The value a fixed element must have, raw, for identification.
    pub fn fixed_raw_value(&self, set: &MibSet, element: usize) -> Option<u64> {
        let el = &self.elements[element];
        if el.kind == ElementKind::Editable {
            return None;
        }
        if el.kind == ElementKind::Area && el.value.is_empty() {
            return None;
        }
        match self.fixed_raw(set, el, &Profile::default()).ok()? {
            Raw::Unsigned(v) => Some(v & mask(el.bits)),
            Raw::Signed(v) => Some(v as u64 & mask(el.bits)),
            _ => None,
        }
    }
}

/// What an editable element offers: its `CDF_VALUE`, else its
/// parameter's `CPC_DEFVAL`, as someone types it.
fn offered(set: &MibSet, el: &Element) -> String {
    if el.kind != ElementKind::Editable {
        return String::new();
    }
    let Some(d) = &el.domain else { return el.value.clone() };
    if !el.value.is_empty() {
        return match el.inter {
            Interp::Dynamic => String::new(),
            ref inter => d.mib_value(&el.value, inter),
        };
    }
    let Some(p) = el.param else { return String::new() };
    let cpc = &set.get(p).row;
    d.mib_value(cpc.clean("CPC_DEFVAL"), &Interp::of(cpc.clean("CPC_INTER")))
}

/// A fixed repeater's count.
fn fixed_count(el: &Element) -> Option<usize> {
    if el.kind == ElementKind::Editable || !el.is_repeater() {
        return None;
    }
    let raw = el.domain.as_ref()?.mib_raw(&el.value, &el.inter, &Profile::default()).ok()?;
    raw.as_f64().map(|n| (n.max(0.0) as usize).min(MAX_REPEAT))
}

/// The count a repeater's value gives: its raw value.
pub fn count_of(el: &Element, value: &str) -> usize {
    let raw = match &el.domain {
        Some(d) => d.to_raw(value, &Profile::default()).ok().and_then(|r| r.as_f64()),
        None => values::parse_number(value),
    };
    raw.map(|n| (n.max(0.0) as usize).min(MAX_REPEAT)).unwrap_or(0)
}

/// A deduced parameter's type, from the value naming the TM parameter:
/// its name, or its `PCF_PID` number.
fn deduced_type(set: &MibSet, root: usize, named: &Raw) -> Option<ParamType> {
    let index = set.index();
    let row = match named {
        Raw::Text(name) => index.first_row_by_field("pcf", "PCF_NAME", name.trim(), Some(root)),
        Raw::Unsigned(n) => index.first_row_by_field("pcf", "PCF_PID", &n.to_string(), Some(root)),
        _ => None,
    }
    .or_else(|| match named {
        Raw::Unsigned(n) => index.first_row_by_field("pcf", "PCF_NAME", &n.to_string(), Some(root)),
        _ => None,
    })?;
    Some(ParamType::plain(row.clean("PCF_NAME"), row.clean("PCF_PTC").parse().ok()?, row.clean("PCF_PFC").parse().ok()?, root))
}

fn mask(bits: usize) -> u64 {
    if bits >= 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    }
}

fn put_bytes(out: &mut Vec<u8>, at: usize, bytes: &[u8]) {
    for (k, b) in bytes.iter().enumerate() {
        bits::put(out, at + k * 8, 8, *b as u64);
    }
}

fn get_bytes(app: &[u8], at: usize, n: usize) -> Option<Vec<u8>> {
    (0..n).map(|k| bits::get(app, at + k * 8, 8).map(|v| v as u8)).collect()
}

struct Writer<'a> {
    set: &'a MibSet,
    profile: &'a Profile,
    args: &'a [(String, String)],
    next: usize,
    out: Vec<u8>,
    placed: Vec<Placed>,
    cursor: usize,
    referenced: Option<Raw>,
}

struct Reader<'a> {
    set: &'a MibSet,
    app: &'a [u8],
    out: Vec<Read>,
    cursor: usize,
    referenced: Option<Raw>,
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::set::tests::{temp_root, write};

    fn line(fields: &[&str], n: usize) -> String {
        let mut v: Vec<&str> = fields.to_vec();
        v.resize(n.max(fields.len()), "");
        v.join("\t")
    }

    /// TC1: a counter N repeating A (editable), a spare and B (fixed 7),
    /// then C after the group; TC2: an editable with its default in
    /// CDF_VALUE, raw, in hex; TC3: a group nested in a group.
    pub(crate) fn groups() -> MibSet {
        let r = temp_root("layout");
        write(&r, "ccf", &[line(&["TC1", "Groups", "", "", "N", "", "8", "1", "100", "4"], 21), line(&["TC2", "Default", "", "", "N", "", "8", "2", "100", "2"], 21), line(&["TC3", "Nested", "", "", "N", "", "8", "3", "100", "4"], 21)].join("\n"));
        write(
            &r,
            "cdf",
            &[
                line(&["TC1", "E", "count", "8", "0", "3", "N"], 10),
                line(&["TC1", "E", "a", "8", "8", "0", "A"], 10),
                line(&["TC1", "A", "spare", "8", "16", "0", "", "", "0"], 10),
                line(&["TC1", "F", "b", "8", "24", "0", "B", "R", "7"], 10),
                line(&["TC1", "E", "c", "8", "32", "0", "C"], 10),
                line(&["TC2", "E", "a", "8", "0", "0", "A", "R", "1F"], 10),
                line(&["TC2", "E", "mode", "8", "8", "0", "M"], 10),
                // Out of order in the file: CDF_BIT puts them right.
                line(&["TC3", "E", "inner", "8", "8", "1", "N2"], 10),
                line(&["TC3", "E", "outer", "8", "0", "2", "N"], 10),
                line(&["TC3", "E", "x", "8", "16", "0", "A"], 10),
                line(&["TC3", "E", "after", "8", "24", "0", "C"], 10),
            ]
            .join("\n"),
        );
        write(
            &r,
            "cpc",
            &[
                line(&["N", "count", "3", "4"], 17),
                line(&["N2", "count", "3", "4"], 17),
                line(&["A", "a", "3", "4", "", "H"], 17),
                line(&["B", "b", "3", "4"], 17),
                line(&["C", "c", "3", "4"], 17),
                line(&["M", "mode", "3", "4", "", "", "", "", "", "", "PAF1", "E", "ON"], 17),
            ]
            .join("\n"),
        );
        write(&r, "paf", "PAF1\tmode\tU\t2\n");
        write(&r, "pas", "PAF1\tOFF\t0\nPAF1\tON\t1\n");
        MibSet::load(vec![r], None)
    }

    fn tc(set: &MibSet, name: &str) -> Layout {
        Layout::of(set, set.find(Kind::Telecommand, 0, name).unwrap())
    }

    fn args(a: &[(&str, &str)]) -> Vec<(String, String)> {
        a.iter().map(|(n, v)| (n.to_string(), v.to_string())).collect()
    }

    #[test]
    fn a_group_counts_every_element_after_its_repeater() {
        let set = groups();
        let l = tc(&set, "TC1");
        assert_eq!(l.nodes, vec![Node::Group { repeater: 0, body: vec![Node::Leaf(1), Node::Leaf(2), Node::Leaf(3)] }, Node::Leaf(4)]);
        assert!(l.problems.is_empty(), "{:?}", l.problems);
        let fields = l.fields(&set, |_| 2);
        let labels: Vec<String> = fields.iter().map(|f| f.label(&l)).collect();
        assert_eq!(labels, vec!["N", "A[1]", "A[2]", "C"]);
    }

    #[test]
    fn an_editable_default_in_cdf_value_is_asked_with_its_default() {
        let set = groups();
        let l = tc(&set, "TC2");
        assert_eq!(l.asked().map(|e| e.name.as_str()).collect::<Vec<_>>(), vec!["A", "M"]);
        assert_eq!(l.default(&set, 0), "31", "raw 1F in hex");
        assert_eq!(l.default(&set, 1), "ON", "CPC_DEFVAL, engineering");
    }

    #[test]
    fn repetitions_move_what_follows_and_decode_back() {
        let set = groups();
        let l = tc(&set, "TC1");
        let p = Profile::default();
        let (bytes, _) = l.encode(&set, &args(&[("N", "2"), ("A", "1"), ("A", "2"), ("C", "9")]), &p).unwrap();
        assert_eq!(bytes, vec![2, 1, 0, 7, 2, 0, 7, 9]);
        let (none, _) = l.encode(&set, &args(&[("N", "0"), ("C", "9")]), &p).unwrap();
        assert_eq!(none, vec![0, 9], "no repetition: C moves back by the group");
        let read = l.decode(&set, &bytes, &p);
        let values: Vec<(String, String)> = read.iter().map(|r| r.as_ref().unwrap()).map(|pl| (Field { element: pl.element, path: pl.path.clone() }.label(&l), pl.raw.display())).collect();
        assert_eq!(values.iter().map(|(n, v)| format!("{n}={v}")).collect::<Vec<_>>(), vec!["N=2", "A[1]=1", "spare[1]=0", "B[1]=7", "A[2]=2", "spare[2]=0", "B[2]=7", "C=9"]);
        assert!(l.encode(&set, &args(&[("N", "1"), ("C", "9")]), &p).is_err(), "A[1] is missing");
    }

    #[test]
    fn nested_groups_expand_inside_each_other() {
        let set = groups();
        let l = tc(&set, "TC3");
        assert_eq!(l.nodes, vec![Node::Group { repeater: 0, body: vec![Node::Group { repeater: 1, body: vec![Node::Leaf(2)] }] }, Node::Leaf(3)]);
        let p = Profile::default();
        let a = args(&[("N", "2"), ("N2", "1"), ("A", "5"), ("N2", "2"), ("A", "6"), ("A", "7"), ("C", "1")]);
        let (bytes, _) = l.encode(&set, &a, &p).unwrap();
        assert_eq!(bytes, vec![2, 1, 5, 2, 6, 7, 1]);
        let mut counts = std::collections::HashMap::new();
        counts.insert("N@".to_string(), 2);
        counts.insert("N2@1".to_string(), 1);
        counts.insert("N2@2".to_string(), 2);
        let labels: Vec<String> = l.fields(&set, |f| counts.get(&f.key(&l)).copied().unwrap_or(0)).iter().map(|f| f.label(&l)).collect();
        assert_eq!(labels, vec!["N", "N2[1]", "A[1.1]", "N2[2]", "A[2.1]", "A[2.2]", "C"]);
        assert_eq!(l.enclosing(2), vec![1, 0]);
    }

    #[test]
    fn a_group_running_past_the_end_is_a_problem() {
        let r = temp_root("layout-bad");
        write(&r, "ccf", &line(&["TC9", "Bad", "", "", "N", "", "8", "1", "100", "2"], 21));
        write(&r, "cdf", &[line(&["TC9", "E", "n", "8", "0", "5", "N"], 10), line(&["TC9", "E", "a", "8", "8", "0", "A"], 10)].join("\n"));
        write(&r, "cpc", &[line(&["N", "n", "5", "1"], 17), line(&["A", "a", "3", "4"], 17)].join("\n"));
        let set = MibSet::load(vec![r], None);
        let l = tc(&set, "TC9");
        assert!(l.problems.iter().any(|p| p.contains("only 1 follow")), "{:?}", l.problems);
        assert!(l.problems.iter().any(|p| p.contains("unsigned integer")), "{:?}", l.problems);
    }
}
