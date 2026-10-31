//! Flowchart edges routed the way flowcharts are drawn by hand: a line
//! straight down where two boxes line up, a decision's branches leaving
//! from its side corners, a loop going round the side, and no line
//! turning more than twice. The renderer's router finds a path around
//! everything; this keeps its layout (where the boxes are, a lone child
//! nudged under its parent) and redraws the lines between them, then puts
//! each label beside where its line starts -- "Yes" by the corner it
//! leaves from. An edge that can't be drawn so without crossing a box or
//! running along another line keeps the router's path.

use std::collections::{HashMap, HashSet};

use mermaid_rs_renderer::{Direction, Layout, NodeLayout, NodeShape};

/// How far apart two centres may be and still count as lined up.
const ALIGN: f32 = 8.0;
/// Room kept around a box a line passes.
const CLEAR: f32 = 4.0;
/// The shortest last run, so its arrowhead shows.
const LAST_RUN: f32 = 12.0;
/// How far out a loop goes past what it passes.
const LOOP_OUT: f32 = 24.0;

/// A box seen as if the flow went down the page, whatever its direction.
#[derive(Clone, Copy)]
struct Box_ {
    cx: f32,
    cy: f32,
    hw: f32,
    hh: f32,
    /// How far in from its box a slanted shape's side is, halfway down.
    inset: f32,
    decision: bool,
}

impl Box_ {
    fn top(&self) -> f32 {
        self.cy - self.hh
    }
    fn bottom(&self) -> f32 {
        self.cy + self.hh
    }
    /// The middle of its side facing `toward` (left when negative).
    fn side(&self, toward: f32) -> (f32, f32) {
        let reach = self.hw - self.inset;
        (if toward < 0.0 { self.cx - reach } else { self.cx + reach }, self.cy)
    }
    /// How far across its flat top or bottom reaches from its centre: a
    /// decision meets a line only at its point.
    fn flat(&self) -> f32 {
        if self.decision { 0.0 } else { (self.hw - self.inset - CLEAR).max(0.0) }
    }
    /// Whether a level or plumb run from `a` to `b` passes through it.
    fn crossed_by(&self, a: (f32, f32), b: (f32, f32)) -> bool {
        let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
        let (y0, y1) = (a.1.min(b.1), a.1.max(b.1));
        x1 > self.cx - self.hw - CLEAR && x0 < self.cx + self.hw + CLEAR && y1 > self.top() - CLEAR && y0 < self.bottom() + CLEAR
    }
    fn overlaps(&self, other: &Box_, room: f32) -> bool {
        (self.cx - other.cx).abs() < self.hw + other.hw + room && (self.cy - other.cy).abs() < self.hh + other.hh + room
    }
}

/// Down the page, as the direction has it.
#[derive(Clone, Copy)]
struct Frame(Direction);

impl Frame {
    fn to(self, (x, y): (f32, f32)) -> (f32, f32) {
        match self.0 {
            Direction::TopDown => (x, y),
            Direction::BottomTop => (x, -y),
            Direction::LeftRight => (y, x),
            Direction::RightLeft => (y, -x),
        }
    }
    fn from(self, (a, b): (f32, f32)) -> (f32, f32) {
        match self.0 {
            Direction::TopDown => (a, b),
            Direction::BottomTop => (a, -b),
            Direction::LeftRight => (b, a),
            Direction::RightLeft => (-b, a),
        }
    }
    fn across(self) -> bool {
        matches!(self.0, Direction::LeftRight | Direction::RightLeft)
    }
    fn place(self, n: &NodeLayout) -> Box_ {
        let (cx, cy) = self.to((n.x + n.width / 2.0, n.y + n.height / 2.0));
        let (hw, hh) = if self.across() { (n.height / 2.0, n.width / 2.0) } else { (n.width / 2.0, n.height / 2.0) };
        // A parallelogram's or trapezoid's slanted sides are in from its
        // box (by 18% of its width at the top or bottom, so 9% halfway);
        // its top and bottom, the sides a sideways flow meets, are not.
        let slanted = matches!(n.shape, NodeShape::Parallelogram | NodeShape::ParallelogramAlt | NodeShape::Trapezoid | NodeShape::TrapezoidAlt);
        let inset = if slanted && !self.across() { n.width * 0.09 } else { 0.0 };
        Box_ { cx, cy, hw, hh, inset, decision: n.shape == NodeShape::Diamond }
    }
    /// The real top-left of a box of `n`'s size centred on `centre`.
    fn corner(self, n: &NodeLayout, centre: (f32, f32)) -> (f32, f32) {
        let (x, y) = self.from(centre);
        (x - n.width / 2.0, y - n.height / 2.0)
    }
}

/// How a route left its start: from a box's side, its bottom, or its
/// bottom along a bus shared with its siblings -- where its label goes
/// after the turn, so siblings' labels don't sit on each other.
#[derive(Clone, Copy, PartialEq)]
enum Start {
    Side,
    Bottom,
    Bus,
}

/// A route: its points, how it starts, which sides of its ends it uses
/// (`(from's, to's)`, -1 left, 1 right, 0 neither).
struct Route {
    points: Vec<(f32, f32)>,
    start: Start,
    sides: (i8, i8),
}

fn sign(v: f32) -> i8 {
    if v < 0.0 { -1 } else { 1 }
}

/// A run of a placed line, and the edge's ends.
struct Run<'a> {
    a: (f32, f32),
    b: (f32, f32),
    from: &'a str,
    to: &'a str,
}

/// Whether two level or plumb runs lie along each other.
fn along(a0: (f32, f32), a1: (f32, f32), b0: (f32, f32), b1: (f32, f32)) -> bool {
    let level = |p: (f32, f32), q: (f32, f32)| (p.1 - q.1).abs() < 0.5;
    let plumb = |p: (f32, f32), q: (f32, f32)| (p.0 - q.0).abs() < 0.5;
    let overlap = |p0: f32, p1: f32, q0: f32, q1: f32| p0.max(p1).min(q0.max(q1)) - p0.min(p1).max(q0.min(q1)) > 1.0;
    (level(a0, a1) && level(b0, b1) && (a0.1 - b0.1).abs() < CLEAR && overlap(a0.0, a1.0, b0.0, b1.0))
        || (plumb(a0, a1) && plumb(b0, b1) && (a0.0 - b0.0).abs() < CLEAR && overlap(a0.1, a1.1, b0.1, b1.1))
}

/// Redraws `layout`'s flowchart edges.
pub fn flowchart(layout: &mut Layout, direction: Direction) {
    let frame = Frame(direction);
    let grouped: HashSet<String> = layout.subgraphs.iter().flat_map(|g| g.nodes.iter().cloned()).collect();
    let mut boxes: HashMap<String, Box_> = layout.nodes.iter().filter(|(_, n)| !n.hidden && n.anchor_subgraph.is_none()).map(|(id, n)| (id.clone(), frame.place(n))).collect();
    let forward = |from: &Box_, to: &Box_| to.top() >= from.bottom() - 1.0;
    let ends: Vec<(String, String)> = layout.edges.iter().map(|e| (e.from.clone(), e.to.clone())).collect();

    // A lone child under a lone parent, a little off to one side, set
    // right under it -- unless it would then touch another box.
    let mut order: Vec<String> = boxes.keys().cloned().collect();
    order.sort_by(|a, b| boxes[a].cy.total_cmp(&boxes[b].cy));
    for id in &order {
        if grouped.contains(id) {
            continue;
        }
        let downward = |boxes: &HashMap<String, Box_>, f: &str, t: &str| f != t && matches!((boxes.get(f), boxes.get(t)), (Some(s), Some(t)) if forward(s, t));
        let parents: Vec<&str> = ends.iter().filter(|(f, t)| t == id && downward(&boxes, f, t)).map(|(f, _)| f.as_str()).collect();
        let [parent] = parents[..] else { continue };
        if ends.iter().filter(|(f, t)| f == parent && downward(&boxes, f, t)).count() != 1 {
            continue;
        }
        let (s, t) = (boxes[parent], boxes[id]);
        let dx = s.cx - t.cx;
        if dx.abs() <= 0.5 || dx.abs() > s.hw.max(t.hw) * 0.6 {
            continue;
        }
        let moved = Box_ { cx: s.cx, ..t };
        if boxes.iter().any(|(other, b)| other != id && moved.overlaps(b, 2.0 * CLEAR)) {
            continue;
        }
        boxes.insert(id.clone(), moved);
        let n = layout.nodes.get_mut(id).unwrap();
        (n.x, n.y) = frame.corner(n, (moved.cx, moved.cy));
    }

    // A decision's side corner goes to the branch farthest out that way;
    // a nearer one on the same side leaves from below, so two branches
    // never share a corner.
    let mut corner: HashSet<usize> = HashSet::new();
    let mut farthest: HashMap<(&str, bool), (usize, f32)> = HashMap::new();
    for (i, (f, t)) in ends.iter().enumerate() {
        let (Some(s), Some(t)) = (boxes.get(f), boxes.get(t)) else { continue };
        let dx = t.cx - s.cx;
        if s.decision && forward(s, t) && dx.abs() > s.hw - s.inset + CLEAR {
            let slot = farthest.entry((f.as_str(), dx > 0.0)).or_insert((i, 0.0));
            if dx.abs() >= slot.1 {
                *slot = (i, dx.abs());
            }
        }
    }
    corner.extend(farthest.values().map(|(i, _)| *i));
    let mut incoming: HashMap<&str, usize> = HashMap::new();
    let (mut children, mut parents): (HashMap<&str, usize>, HashMap<&str, usize>) = (HashMap::new(), HashMap::new());
    for (f, t) in &ends {
        *incoming.entry(t.as_str()).or_default() += 1;
        if let (Some(s), Some(tb)) = (boxes.get(f), boxes.get(t)) {
            if f != t && forward(s, tb) {
                *children.entry(f.as_str()).or_default() += 1;
                *parents.entry(t.as_str()).or_default() += 1;
            }
        }
    }

    // Lines going down first, then the loops back up round them.
    let mut todo: Vec<usize> = (0..ends.len()).collect();
    todo.sort_by_key(|&i| match (boxes.get(&ends[i].0), boxes.get(&ends[i].1)) {
        (Some(s), Some(t)) => !forward(s, t),
        _ => true,
    });
    let mut used: HashSet<(&str, i8)> = HashSet::new();
    let mut placed: Vec<Run> = Vec::new();
    let mut routes: HashMap<usize, Route> = HashMap::new();
    for i in todo {
        let (f, t) = (&ends[i].0, &ends[i].1);
        let (Some(s), Some(tb)) = (boxes.get(f), boxes.get(t)) else { continue };
        if f == t {
            continue;
        }
        let others: Vec<&Box_> = boxes.iter().filter(|(id, _)| *id != f && *id != t).map(|(_, b)| b).collect();
        let fits = |r: &Route| {
            // Inside the picture: it starts at 0, 0.
            r.points.iter().all(|p| {
                let (x, y) = frame.from(*p);
                x >= CLEAR && y >= CLEAR
            }) && (r.sides.0 == 0 || !used.contains(&(f.as_str(), r.sides.0)))
                && (r.sides.1 == 0 || !used.contains(&(t.as_str(), r.sides.1)))
                && r.points.windows(2).all(|w| {
                    others.iter().all(|b| !b.crossed_by(w[0], w[1])) && placed.iter().all(|p| (p.from == f || p.to == t) || !along(w[0], w[1], p.a, p.b))
                })
        };
        let lone = children.get(f.as_str()) == Some(&1) && parents.get(t.as_str()) == Some(&1);
        let options = if forward(s, tb) { down(s, tb, corner.contains(&i), incoming.get(t.as_str()).copied().unwrap_or(0) > 1, lone) } else { back(s, tb, &others) };
        let Some(route) = options.into_iter().find(|r| fits(r)) else { continue };
        if route.sides.0 != 0 {
            used.insert((f.as_str(), route.sides.0));
        }
        if route.sides.1 != 0 {
            used.insert((t.as_str(), route.sides.1));
        }
        placed.extend(route.points.windows(2).map(|w| Run { a: w[0], b: w[1], from: f, to: t }));
        routes.insert(i, route);
    }

    let (mut right, mut bottom) = (layout.width, layout.height);
    for (i, route) in routes {
        let e = &mut layout.edges[i];
        e.points = route.points.into_iter().map(|p| frame.from(p)).collect();
        if let Some(label) = &e.label {
            let at = if route.start == Start::Bus { beside(&e.points[2..], label.width, label.height, Start::Bottom) } else { beside(&e.points, label.width, label.height, route.start) };
            right = right.max(at.0 + label.width / 2.0 + 8.0);
            bottom = bottom.max(at.1 + label.height / 2.0 + 8.0);
            e.label_anchor = Some(at);
        }
        for p in &e.points {
            right = right.max(p.0 + 8.0);
            bottom = bottom.max(p.1 + 8.0);
        }
    }
    // A loop round the outside can reach past the picture's edge.
    layout.width = right;
    layout.height = bottom;
}

/// The ways down from `s` to `t`, the best first. `lone`: `t` is `s`'s
/// only child and `s` its only parent.
fn down(s: &Box_, t: &Box_, corner: bool, merging: bool, lone: bool) -> Vec<Route> {
    let mut out = Vec::new();
    let dx = t.cx - s.cx;
    // Lined up: straight down -- from under the source's centre to the
    // target's, or, between a lone pair, wherever both flat edges meet
    // (siblings all leave from the centre, so their lines branch from one).
    let shared = if dx.abs() <= ALIGN {
        Some((s.cx + t.cx) / 2.0)
    } else if lone && dx.abs() <= s.flat() {
        Some(t.cx)
    } else if lone && dx.abs() <= t.flat() {
        Some(s.cx)
    } else {
        None
    };
    if let Some(x) = shared {
        out.push(Route { points: vec![(x, s.bottom()), (x, t.top())], start: Start::Bottom, sides: (0, 0) });
    }
    // A decision's branch: out of its corner, across, and down -- one turn.
    let from = s.side(dx);
    if corner && dx.abs() > s.hw - s.inset + CLEAR && (t.cx - from.0).abs() >= LAST_RUN && t.top() - s.cy >= LAST_RUN {
        out.push(Route { points: vec![from, (t.cx, from.1), (t.cx, t.top())], start: Start::Side, sides: (sign(dx), 0) });
    }
    // Down, then into the target's side -- one turn; where lines merge.
    let into_side = (dx.abs() > t.hw + CLEAR && t.cy - s.bottom() >= LAST_RUN).then(|| {
        let to = t.side(-dx);
        Route { points: vec![(s.cx, s.bottom()), (s.cx, to.1), to], start: Start::Bottom, sides: (0, sign(-dx)) }
    });
    // Down, across, down -- two turns, the middle run in the gap above
    // the target (or below the source).
    let mut steps = Vec::new();
    let (low, high) = (s.bottom(), t.top());
    if high - low >= 2.0 * LAST_RUN {
        let mid = (low + high) / 2.0;
        for y in [mid, mid - LAST_RUN, mid + LAST_RUN, high - LAST_RUN * 1.5, low + LAST_RUN * 1.5] {
            if y - low >= LAST_RUN && high - y >= LAST_RUN {
                steps.push(Route { points: vec![(s.cx, low), (s.cx, y), (t.cx, y), (t.cx, high)], start: Start::Bus, sides: (0, 0) });
            }
        }
    }
    if merging {
        out.extend(into_side);
        out.extend(steps);
    } else {
        out.extend(steps);
        out.extend(into_side);
    }
    out
}

/// The ways back up from `s` to `t`: across between them when they're
/// side by side, else out of the source's side, round everything
/// between, and into the target's side -- two turns.
fn back(s: &Box_, t: &Box_, others: &[&Box_]) -> Vec<Route> {
    let mut out = Vec::new();
    let dx = t.cx - s.cx;
    if (t.cy - s.cy).abs() <= ALIGN && dx.abs() > s.hw + t.hw {
        let y = (s.cy + t.cy) / 2.0;
        out.push(Route { points: vec![(s.side(dx).0, y), (t.side(-dx).0, y)], start: Start::Side, sides: (sign(dx), sign(-dx)) });
        return out;
    }
    // The way up runs just outside some box -- any box: the shortest that
    // crosses nothing is taken.
    let xs: Vec<f32> = others.iter().copied().chain([s, t]).flat_map(|b| [b.cx - b.hw - LOOP_OUT, b.cx + b.hw + LOOP_OUT]).collect();
    let mut ways: Vec<(f32, Route)> = Vec::new();
    for a in [1.0_f32, -1.0] {
        for b in [1.0_f32, -1.0] {
            let (from, to) = (s.side(a), t.side(b));
            for &x in &xs {
                if (x - from.0) * a < LAST_RUN || (x - to.0) * b < LAST_RUN {
                    continue;
                }
                let length = (x - from.0).abs() + (x - to.0).abs();
                ways.push((length, Route { points: vec![from, (x, from.1), (x, to.1), to], start: Start::Side, sides: (sign(a), sign(b)) }));
            }
        }
    }
    ways.sort_by(|a, b| a.0.total_cmp(&b.0));
    out.extend(ways.into_iter().map(|(_, r)| r));
    // Both its sides taken (lines merging into it): out of its bottom,
    // round, and up -- three turns, for a loop only.
    let below = s.bottom() + LOOP_OUT;
    let mut ways: Vec<(f32, Route)> = Vec::new();
    for b in [1.0_f32, -1.0] {
        let to = t.side(b);
        for &x in &xs {
            if (x - to.0) * b < LAST_RUN || (x - s.cx).abs() < LAST_RUN {
                continue;
            }
            let length = (x - s.cx).abs() + (x - to.0).abs();
            ways.push((length, Route { points: vec![(s.cx, s.bottom()), (s.cx, below), (x, below), (x, to.1), to], start: Start::Bottom, sides: (0, sign(b)) }));
        }
    }
    ways.sort_by(|a, b| a.0.total_cmp(&b.0));
    out.extend(ways.into_iter().map(|(_, r)| r));
    out
}

/// Where a label goes: beside the start of its line -- above a level run,
/// right of a plumb one -- clear of the box it leaves.
fn beside(points: &[(f32, f32)], w: f32, h: f32, start: Start) -> (f32, f32) {
    let (a, b) = (points[0], points.get(1).copied().unwrap_or(points[0]));
    let level = (b.1 - a.1).abs() < (b.0 - a.0).abs();
    if level {
        let along = (b.0 - a.0).signum();
        let reach = if start != Start::Bottom { w / 2.0 + 6.0 } else { ((b.0 - a.0).abs() / 2.0).max(w / 2.0 + 6.0) };
        (a.0 + along * reach, a.1 - h / 2.0 - 3.0)
    } else {
        let along = (b.1 - a.1).signum();
        (a.0 + w / 2.0 + 6.0, a.1 + along * (h / 2.0 + 6.0))
    }
}
