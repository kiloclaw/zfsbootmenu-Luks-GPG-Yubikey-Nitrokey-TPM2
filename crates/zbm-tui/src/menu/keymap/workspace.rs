//! The key map of the workspace (ADR 0007, `docs/dev/menu-design/keymap.md`),
//! which the usability sessions compare with the current one (decision 8):
//! `j` `k` `h` `l` move as the arrows do, `h` and `l` out of and into a
//! column, the digits and `Tab` choose a tab. Its bindings come before the
//! current map's, whose bindings of the same keys it drops.

use zbm_display::keys::Key;

use super::{Binding, Context, Footer, Intent, KEYMAP, NONE, Scope, bind, key};
use Context::{Diff, Help, Kernels, Log, Main, Packages, Pools, Recovery, Snapshots};

/// The tabs of the workspace.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tab {
    /// The boot environments and their kernels.
    Boot,
    /// The snapshots of the selected environment.
    Snapshots,
    /// Pools and LUKS volumes.
    Pools,
    /// The log.
    Log,
}

impl Tab {
    /// Every tab, in the order of their digits.
    pub const ALL: [Tab; 4] = [Tab::Boot, Tab::Snapshots, Tab::Pools, Tab::Log];

    /// The tab after this one, after the last the first.
    #[must_use]
    pub fn next(self) -> Tab {
        self.moved(1)
    }

    /// The tab before this one, before the first the last.
    #[must_use]
    pub fn previous(self) -> Tab {
        self.moved(Tab::ALL.len() - 1)
    }

    /// The tab `steps` to the right, around.
    fn moved(self, steps: usize) -> Tab {
        let at = Tab::ALL.iter().position(|tab| *tab == self).unwrap_or(0);
        Tab::ALL
            .get((at + steps) % Tab::ALL.len())
            .copied()
            .unwrap_or(Tab::Boot)
    }
}

/// Which key map the menu reads keys with.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Map {
    /// The current one (`docs/dev/menu-design/keymap.md`, the current map).
    #[default]
    Classic,
    /// The workspace's (ADR 0007).
    Workspace,
}

/// The views with the tab bar.
const TABBED: &[Context] = &[
    Main, Snapshots, Kernels, Pools, Recovery, Log, Diff, Packages, Help,
];

/// The views of the Boot tab's columns, which the arrows go into and out of.
const COLUMNS: &[Context] = &[Main, Kernels];

/// A digit that chooses `tab`.
const fn digit(c: char, tab: Tab) -> Binding {
    bind(
        Key::Char(c),
        NONE,
        Scope::Only(TABBED),
        Intent::Tab(tab),
        Footer::Hidden,
    )
}

/// A letter that moves as `intent` does, shown in the help view only.
const fn mover(c: char, intent: Intent) -> Binding {
    bind(Key::Char(c), NONE, Scope::Views, intent, Footer::Hidden)
}

/// The bindings of the workspace that the current map lacks or binds to
/// something else, in the order of the toolbar.
pub const WORKSPACE: &[Binding] = &[
    bind(
        Key::Enter,
        NONE,
        Scope::Everywhere,
        Intent::Primary,
        Footer::Key,
    ),
    // Into the kernels column of the selected environment.
    key('l', &[Main], Intent::Kernels),
    mover('j', Intent::Down),
    mover('k', Intent::Up),
    // Out of a column, as Escape.
    mover('h', Intent::Back),
    // The arrows go into the kernels column and out of it, as `l` and `h`;
    // in the changes they still go from one segment to the other.
    bind(
        Key::Right,
        NONE,
        Scope::Only(COLUMNS),
        Intent::Kernels,
        Footer::Hidden,
    ),
    bind(
        Key::Left,
        NONE,
        Scope::Only(COLUMNS),
        Intent::Back,
        Footer::Hidden,
    ),
    digit('1', Tab::Boot),
    digit('2', Tab::Snapshots),
    digit('3', Tab::Pools),
    digit('4', Tab::Log),
    bind(
        Key::Tab,
        NONE,
        Scope::Only(TABBED),
        Intent::NextTab,
        Footer::Hidden,
    ),
    bind(
        Key::BackTab,
        NONE,
        Scope::Only(TABBED),
        Intent::PreviousTab,
        Footer::Hidden,
    ),
];

/// The bindings of the workspace that come after the current map's in the
/// toolbar, as they do less than its keys of the same view: `z` folds the
/// group of the timeline the selection is in.
const WORKSPACE_LAST: &[Binding] = &[key('z', &[Snapshots], Intent::Fold)];

/// Letters of the current map that the workspace leaves unbound: the pools
/// are a tab.
const FREED: &[char] = &['p'];

/// Whether the workspace drops `binding` of the current map: it binds the
/// same key in a view where `binding` works too, or frees it.
fn is_taken(binding: &Binding) -> bool {
    let freed =
        binding.modifiers == NONE && matches!(binding.key, Key::Char(c) if FREED.contains(&c));
    freed
        || WORKSPACE.iter().chain(WORKSPACE_LAST).any(|own| {
            own.key == binding.key
                && own.modifiers == binding.modifiers
                && own.scope.has_context_in_common(binding.scope)
        })
}

impl Map {
    /// Its bindings, in the order of the toolbar.
    pub fn table(self) -> Vec<&'static Binding> {
        match self {
            Map::Classic => KEYMAP.iter().collect(),
            Map::Workspace => WORKSPACE
                .iter()
                .chain(KEYMAP.iter().filter(|binding| !is_taken(binding)))
                .chain(WORKSPACE_LAST)
                .collect(),
        }
    }

    /// The first of its bindings that asks for `intent`, if one does: the
    /// key a label or a word of the line view names, such as the digit of a
    /// tab.
    pub(crate) fn binding(self, intent: Intent) -> Option<&'static Binding> {
        self.table()
            .into_iter()
            .find(|binding| binding.intent == intent)
    }

    /// The first of its bindings that asks for `intent` in `context`, if
    /// one does: the key a word of the line view types there.
    pub(crate) fn binding_in(self, intent: Intent, context: Context) -> Option<&'static Binding> {
        self.table()
            .into_iter()
            .find(|binding| binding.intent == intent && binding.scope.contains(context))
    }
}
