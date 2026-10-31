//! Where the pets can be: floors, and spots on them. A floor is a
//! level edge on screen -- the modeline's top, the foot of a pane, a
//! pane's top edge where it meets the one above, the margin beside the
//! line you're on -- and a spot is a column on one. Fenix's layout says
//! where each is; it moves when the panes do.

use fenix_extension_api::{Anchor, Layout, Place};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Floor {
    /// The modeline's top edge.
    Modeline,
    /// The foot of a pane, by id.
    Foot(u64),
    /// A pane's top edge, where it meets the one above.
    Ledge(u64),
    /// The right margin of the focused pane, on the caret's row (the row
    /// it was when they came).
    Margin(u32),
    /// The caret itself: where a thrown ball starts.
    Caret,
}

/// A column on a floor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spot {
    pub floor: Floor,
    pub x: f32,
}

impl Spot {
    pub fn new(floor: Floor, x: f32) -> Spot {
        Spot { floor, x }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Pane {
    pub id: u64,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub focused: bool,
}

/// What Fenix last said of the screen, in cells.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Screen {
    pub columns: u32,
    pub rows: u32,
    pub modeline: u32,
    pub panes: Vec<Pane>,
    /// The caret's cell: column, row.
    pub caret: Option<(u32, u32)>,
}

/// The margin is this many columns wide, and only there when the caret
/// is at least this far from it.
const MARGIN: f32 = 6.0;
const CLEAR_OF_CARET: f32 = 10.0;

impl Screen {
    pub fn from_layout(layout: &Layout) -> Screen {
        Screen {
            columns: layout.columns,
            rows: layout.rows,
            modeline: layout.modeline,
            panes: layout.panes.iter().map(|p| Pane { id: p.id, x: p.x, y: p.y, width: p.width, height: p.height, focused: p.focused }).collect(),
            caret: layout.caret,
        }
    }

    pub fn focused(&self) -> Option<&Pane> {
        self.panes.iter().find(|p| p.focused)
    }

    fn pane(&self, id: u64) -> Option<&Pane> {
        self.panes.iter().find(|p| p.id == id)
    }

    /// Where `floor` runs, left to right, in window columns; none when
    /// it isn't on screen now.
    pub fn span(&self, floor: Floor) -> Option<(f32, f32)> {
        match floor {
            Floor::Modeline => (self.columns > 0).then_some((0.0, self.columns as f32)),
            Floor::Foot(id) | Floor::Ledge(id) => self.pane(id).map(|p| (p.x as f32, (p.x + p.width) as f32)),
            Floor::Margin(row) => {
                let pane = self.focused()?;
                let (col, caret_row) = self.caret?;
                let right = (pane.x + pane.width) as f32;
                (caret_row == row && (col as f32) < right - MARGIN - CLEAR_OF_CARET && row + 1 < pane.y + pane.height).then_some((right - MARGIN, right - 1.0))
            }
            Floor::Caret => self.caret.map(|(c, _)| (c as f32, c as f32)),
        }
    }

    /// Whether `floor` is on screen now.
    pub fn has(&self, floor: Floor) -> bool {
        self.span(floor).is_some()
    }

    /// `spot` kept on its floor, `margin` columns in from each end.
    pub fn clamp(&self, spot: Spot, margin: f32) -> Spot {
        match self.span(spot.floor) {
            Some((l, r)) if r - l > margin * 2.0 => Spot::new(spot.floor, spot.x.clamp(l + margin, r - margin)),
            Some((l, r)) => Spot::new(spot.floor, (l + r) / 2.0),
            None => spot,
        }
    }

    /// The margin beside the caret's line, when there's room for one.
    pub fn margin(&self) -> Option<Floor> {
        let floor = Floor::Margin(self.caret?.1);
        self.has(floor).then_some(floor)
    }

    /// Ledges they can hop up to: panes' top edges with a pane above.
    pub fn ledges(&self) -> Vec<Floor> {
        self.panes.iter().filter(|p| p.y > 2 && p.width > 12).map(|p| Floor::Ledge(p.id)).collect()
    }

    /// Where `spot` is, for Fenix: what a layer item stands on. `lift`
    /// raises it, in lines.
    pub fn place(&self, spot: Spot, lift: f32) -> Place {
        let window = |y: f32| Place::new(Anchor::Window, spot.x, y - lift);
        match spot.floor {
            Floor::Modeline => Place::new(Anchor::Modeline, spot.x, -lift),
            Floor::Foot(id) => match self.pane(id) {
                Some(p) => window((p.y + p.height) as f32),
                None => Place::new(Anchor::Modeline, spot.x, -lift),
            },
            Floor::Ledge(id) => match self.pane(id) {
                Some(p) => window(p.y as f32),
                None => Place::new(Anchor::Modeline, spot.x, -lift),
            },
            Floor::Margin(row) => window(row as f32 + 1.0),
            Floor::Caret => Place::new(Anchor::Caret, 0.0, 1.0 - lift),
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// 100 columns, a pane over another (ids 1 and 2, 2 focused), the
    /// modeline on row 30, the caret at column 20 of row 22.
    pub fn screen() -> Screen {
        Screen {
            columns: 100,
            rows: 31,
            modeline: 30,
            panes: vec![Pane { id: 1, x: 0, y: 1, width: 100, height: 14, focused: false }, Pane { id: 2, x: 0, y: 16, width: 100, height: 14, focused: true }],
            caret: Some((20, 22)),
        }
    }

    #[test]
    fn floors_are_where_the_panes_are() {
        let s = screen();
        assert_eq!(s.span(Floor::Modeline), Some((0.0, 100.0)));
        assert_eq!(s.ledges(), [Floor::Ledge(2)], "the top pane has nothing above it");
        assert_eq!(s.margin(), Some(Floor::Margin(22)));
        assert_eq!(s.span(Floor::Margin(22)), Some((94.0, 99.0)));
        assert_eq!(s.place(Spot::new(Floor::Ledge(2), 10.0), 0.0), Place::new(Anchor::Window, 10.0, 16.0));
        assert_eq!(s.place(Spot::new(Floor::Margin(22), 96.0), 0.5), Place::new(Anchor::Window, 96.0, 22.5));
        assert_eq!(s.place(Spot::new(Floor::Modeline, 4.0), 1.0), Place::new(Anchor::Modeline, 4.0, -1.0));
        assert_eq!(s.place(Spot::new(Floor::Foot(2), 4.0), 0.0), Place::new(Anchor::Window, 4.0, 30.0));
    }

    #[test]
    fn the_margin_keeps_clear_of_the_caret() {
        let mut s = screen();
        s.caret = Some((88, 22));
        assert_eq!(s.margin(), None, "the caret's in the way");
        s.caret = Some((20, 23));
        assert!(!s.has(Floor::Margin(22)), "the caret's moved on");
        assert!(s.has(Floor::Margin(23)));
    }

    #[test]
    fn a_spot_is_kept_on_its_floor() {
        let s = screen();
        assert_eq!(s.clamp(Spot::new(Floor::Modeline, -5.0), 2.0).x, 2.0);
        assert_eq!(s.clamp(Spot::new(Floor::Modeline, 500.0), 2.0).x, 98.0);
        assert_eq!(s.clamp(Spot::new(Floor::Margin(22), 0.0), 2.0).x, 96.0);
        assert_eq!(s.clamp(Spot::new(Floor::Margin(22), 0.0), 3.0).x, 96.5, "too narrow: its middle");
    }
}
