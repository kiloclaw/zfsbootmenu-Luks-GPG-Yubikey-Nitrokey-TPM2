//! The command palette: its query, what it finds and its selection.

use super::Then;
use crate::menu::filter::Ranked;

/// The command palette.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Palette {
    /// The query.
    pub query: String,
    /// What it finds, made when it opens
    /// ([`palette_items`](crate::menu::update::palette_items)) and again when
    /// the machine sends a snapshot or snapshots, not on each key.
    pub items: Vec<PaletteItem>,
    /// What it matches, best first, as indexes of `items`.
    pub matches: Vec<Ranked>,
    /// The selection in `matches`.
    pub selected: usize,
}

/// One entry of the command palette.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaletteItem {
    /// What the palette shows and matches.
    pub label: String,
    /// Its English label, which the palette matches too, when the menu's
    /// language is another (rule 4 of decision 18).
    pub english: Option<String>,
    /// What it does.
    pub then: Then,
}
