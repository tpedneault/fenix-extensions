//! How cells are coloured.

/// A colour from the theme: its roles, or a syntax capture's colour
/// (`keyword`, `text.title`), as the API's `role`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub enum Role {
    Title,
    #[default]
    Text,
    Muted,
    Accent,
    Good,
    Warn,
    Bad,
    Syntax(String),
}

/// A cell's background: one of the theme's tints, by what it means, or a
/// colour of its own -- the API's `shade`.
///
/// A page sent as 0.8.0 has it ([`Canvas::to_api`](crate::Canvas::to_api))
/// draws only [`Fill::Panel`] (as `panels`); one sent with its styles
/// (`Canvas::to_styled`, 0.8.2) draws them all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Fill {
    /// Tinted as a panel: a field being typed in, the active tab.
    Panel,
    /// The focused cell of a table.
    Focus,
    /// Selected text.
    Selection,
    Accent,
    Good,
    Warn,
    Bad,
    /// A colour: a swatch, a preview.
    Rgb(u8, u8, u8),
}

/// How a cell is drawn: its colour, a background, an underline (in its
/// colour). 0.8.0 leaves out all backgrounds but panels, and underlines.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Style {
    pub fg: Role,
    pub bg: Option<Fill>,
    pub underline: bool,
}

impl Style {
    pub fn new(fg: Role) -> Style {
        Style { fg, bg: None, underline: false }
    }

    pub fn bg(mut self, fill: Fill) -> Style {
        self.bg = Some(fill);
        self
    }

    pub fn underline(mut self) -> Style {
        self.underline = true;
        self
    }

    /// Plain text: drawn without a span.
    pub fn is_plain(&self) -> bool {
        *self == Style::default()
    }
}

impl From<Role> for Style {
    fn from(fg: Role) -> Style {
        Style::new(fg)
    }
}

impl From<&Style> for Style {
    fn from(style: &Style) -> Style {
        style.clone()
    }
}
