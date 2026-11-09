//! The command palette: its query, what it finds and its selection, and
//! the entries chosen in it before.

use super::Then;
use crate::menu::filter::Ranked;
use crate::menu::world::Action;

/// The command palette.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Palette {
    /// The query, as typed.
    pub query: String,
    /// What it finds, made when it opens
    /// ([`palette_items`](crate::menu::update::palette_items)) and again when
    /// the machine sends a snapshot or snapshots, not on each key.
    pub items: Vec<PaletteItem>,
    /// What it matches, best first, as indexes of `items`.
    pub matches: Vec<Ranked>,
    /// The selection in `matches`.
    pub selected: usize,
    /// Whether it shows only the entries chosen before (`Ctrl-R`).
    pub recent: bool,
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

/// The entries chosen in the command palette, newest first, for the
/// menu's life: nothing is written down. Each is kept by its key, what it
/// does and to what, never by what was typed: neither the query nor a
/// command line edited for a boot, which the boot's entry carries. A key
/// finds its entry among those of a later palette, so a boot chosen again
/// takes the line edited by then.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Recent(Vec<Then>);

impl Recent {
    /// The most entries it keeps: the oldest goes.
    pub const MOST: usize = 20;

    /// Keeps `then` as the newest entry chosen, once.
    pub fn chose(&mut self, then: &Then) {
        let key = key_of(then);
        self.0.retain(|kept| *kept != key);
        self.0.insert(0, key);
        self.0.truncate(Recent::MOST);
    }

    /// The keys kept, newest first.
    #[cfg(test)]
    pub(crate) fn keys(&self) -> &[Then] {
        &self.0
    }

    /// The indexes of `items` whose entries were chosen, newest first:
    /// the first entry each key finds.
    pub fn found_in(&self, items: &[PaletteItem]) -> Vec<usize> {
        self.0
            .iter()
            .filter_map(|key| items.iter().position(|item| is_key_of(key, &item.then)))
            .collect()
    }
}

/// The key of `then`: itself, but a boot's without its command line.
fn key_of(then: &Then) -> Then {
    match then {
        Then::Act(Action::Boot {
            environment,
            kernel,
            ..
        }) => Then::Act(Action::Boot {
            environment: environment.clone(),
            kernel: kernel.clone(),
            command_line: None,
            countdown: false,
        }),
        other => other.clone(),
    }
}

/// Whether `key` is the key of `then`, without building it: entries are
/// matched against every key each time the palette opens.
fn is_key_of(key: &Then, then: &Then) -> bool {
    match (key, then) {
        (
            Then::Act(Action::Boot {
                environment,
                kernel,
                ..
            }),
            Then::Act(Action::Boot {
                environment: other,
                kernel: other_kernel,
                ..
            }),
        ) => environment == other && kernel == other_kernel,
        _ => key == then,
    }
}
