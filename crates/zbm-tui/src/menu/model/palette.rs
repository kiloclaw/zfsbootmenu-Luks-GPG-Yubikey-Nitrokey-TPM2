//! The command palette: its query, what it finds and its selection, its
//! sections, and the entries chosen in it before.

use zbm_types::{DatasetName, PoolName, Untrusted};

use super::pools::Row;
use super::{Model, Screen, Then};
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
    /// What it matches, as indexes of `items`: by section, in the order of
    /// [`Section`], and best first in each.
    pub matches: Vec<Ranked>,
    /// How many of `matches`, from the first, are entries chosen before,
    /// which show under [`Section::Recent`]: those found with the query
    /// empty, and every match while it shows only those (`Ctrl-R`).
    pub chosen: usize,
    /// The selection in `matches`.
    pub selected: usize,
    /// Whether it shows only the entries chosen before (`Ctrl-R`).
    pub recent: bool,
}

impl Palette {
    /// The section the match at `at` shows under.
    pub fn section_at(&self, at: usize) -> Option<Section> {
        if at < self.chosen {
            return Some(Section::Recent);
        }
        let ranked = self.matches.get(at)?;
        Some(self.items.get(ranked.index)?.section)
    }

    /// The heading drawn before the match at `at`: its section's, when it
    /// is the first match of its section. A heading is never selected.
    pub fn heading_at(&self, at: usize) -> Option<Section> {
        let section = self.section_at(at)?;
        let before = at.checked_sub(1).and_then(|at| self.section_at(at));
        (before != Some(section)).then_some(section)
    }
}

/// The sections of the command palette, in their order (ADR 0007, point
/// 5): the entries chosen before, what changes or runs something, where
/// to go, and help.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Section {
    /// The entries chosen before, newest first.
    Recent,
    /// What changes or runs something.
    Actions,
    /// Where to go: a view, or a thing on its tab.
    Items,
    /// The help of a view, or of the command line.
    Help,
}

/// One entry of the command palette.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaletteItem {
    /// What the palette shows and matches.
    pub label: String,
    /// What the palette matches too, one name a line (rule 4 of decision
    /// 18): first its command's name and target in the menu's language, as
    /// `Tab` completes it; in another language than English, then the
    /// English name and target, and the English label. It is not drawn. A
    /// name is left empty where the label says it, as a clone's boot's in
    /// English, of which there are thousands (B5).
    pub also: String,
    /// The section it shows under, unless it was chosen before.
    pub section: Section,
    /// What it does.
    pub then: Then,
    /// The keys that do it in the view under the palette, as the help
    /// view names them, those the toolbar leaves out too; empty when none
    /// does. Drawn on the right, not matched.
    pub keys: String,
    /// What it costs ([`Costs`]); not matched.
    pub costs: Costs,
}

/// What an entry of the command palette costs before it acts (ADR 0007,
/// point 5; ADR 0010): each is drawn as a word on the right, the PIN as
/// the lock's mark. Made from the entry's action, never from its plan.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each is a cost of its own, drawn as its word"
)]
pub struct Costs {
    /// The locked menu keeps it: the PIN is asked first.
    pub pin: bool,
    /// It writes a pool imported read-only, which is imported read/write
    /// first.
    pub writes: bool,
    /// The environment it changes, boots or opens a shell in is locked:
    /// its key may be asked for.
    pub key: bool,
    /// A review page shows what it loses and asks for its target's name.
    pub review: bool,
}

/// A thing the command palette goes to: its tab opens, with it selected.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Place {
    /// A boot environment, on the Boot tab.
    Environment(DatasetName),
    /// A pool, by its name and identifier, on the Pools tab.
    Pool(PoolName, Option<u64>),
    /// A LUKS volume, by its mapping's name, on the Pools tab.
    Volume(Untrusted<String>),
}

impl Model {
    /// Opens the tab of `place` with it selected, the filter of its list
    /// emptied so that it shows; the Boot tab ends the marks and the
    /// changes, as its key does.
    pub(crate) fn go(&mut self, place: &Place) {
        let row = match place {
            Place::Environment(name) => {
                self.open_boot_tab();
                self.environments.filter.clear();
                self.refresh();
                self.select_environment(name);
                return;
            }
            Place::Pool(name, id) => Row::Pool(name.clone(), *id),
            Place::Volume(name) => Row::Volume(name.get().clone()),
        };
        self.screen = Screen::Pools;
        self.pools.filter.clear();
        self.refilter_pools(Some(row));
    }
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
