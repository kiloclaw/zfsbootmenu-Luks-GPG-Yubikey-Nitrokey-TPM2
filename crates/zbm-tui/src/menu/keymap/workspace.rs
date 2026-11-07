//! The key map of the workspace (ADR 0007, `docs/dev/menu-design/keymap.md`),
//! which the usability sessions compare with the classic one (decision 8):
//! `j` `k` `h` `l` move as the arrows do, `h` and `l` out of and into a
//! column, the digits and `Tab` choose a tab. Its bindings come before the
//! classic map's, whose bindings of the same keys it drops. Start-up, as the
//! Boot tab, binds no character but `?`: the others wait for the next
//! dialog, as the first keys of a passphrase. The review page of a change,
//! the workspace's alone, takes text, and `Tab` and `Shift-Tab` move among
//! its field and its buttons. `Delete` destroys snapshots, which the classic
//! map does not.

use zbm_display::keys::Key;

use super::{Binding, Context, Footer, Intent, KEYMAP, NONE, Scope, bind, key};
use Context::{
    Diff, Help, Kernels, Log, Main, Packages, Pools, Recovery, Review, Snapshots, Startup,
};

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
    /// The classic one (`docs/dev/menu-design/keymap.md`, "The classic map"),
    /// which `zbm.menu=classic` keeps for one release.
    Classic,
    /// The workspace's (ADR 0007): the menu's.
    #[default]
    Workspace,
}

/// The views with the tab bar, whose keys choose a tab: start-up, as the
/// Boot tab, binds only `Tab` and `Shift-Tab` of them ([`Map::is_bound`]).
const TABBED: &[Context] = &[
    Startup, Main, Snapshots, Kernels, Pools, Recovery, Log, Diff, Packages, Help,
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

/// The bindings of the workspace that the classic map lacks or binds to
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
    // On a review page they go from the field to the buttons and back;
    // `PgUp` and `PgDn`, which every view binds, scroll what it loses.
    bind(
        Key::Tab,
        NONE,
        Scope::Only(&[Review]),
        Intent::NextTab,
        Footer::Key,
    ),
    bind(
        Key::BackTab,
        NONE,
        Scope::Only(&[Review]),
        Intent::PreviousTab,
        Footer::Hidden,
    ),
];

/// The bindings of the workspace that come after the classic map's in the
/// toolbar, as they do less than its keys of the same view: `z` folds the
/// group of the timeline the selection is in; `Delete`, which the toolbar
/// leaves out, destroys the marked snapshots, or the selected one, after
/// its review page.
const WORKSPACE_LAST: &[Binding] = &[
    key('z', &[Snapshots], Intent::Fold),
    bind(
        Key::Delete,
        NONE,
        Scope::Only(&[Snapshots]),
        Intent::Destroy,
        Footer::Hidden,
    ),
];

/// Letters of the classic map that the workspace leaves unbound: the pools
/// are a tab.
const FREED: &[char] = &['p'];

/// Whether the workspace drops `binding` of the classic map: it binds the
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

/// Whether the workspace's start-up view, outside a dialog, binds
/// `binding`: `Tab` and `Shift-Tab` choose a tab, the arrows, `Ctrl-P` and
/// `Ctrl-N` move, `?` and F1 open help. Nothing else is offered there, and
/// a key of something else would say so until the next key. No character
/// is bound but `?`, not the digits nor `j` `k`: a key typed there waits
/// for the next dialog, as the first key of a passphrase.
fn is_bound_at_startup(binding: &Binding) -> bool {
    let types = binding.modifiers == NONE && matches!(binding.key, Key::Char(c) if c != '?');
    !types
        && matches!(
            binding.intent,
            Intent::NextTab | Intent::PreviousTab | Intent::Up | Intent::Down | Intent::Help
        )
}

impl Map {
    /// Whether `binding` of its table works in `context`: where its scope
    /// says, but for the workspace's start-up view, which binds only `Tab`,
    /// `Shift-Tab`, the moves up and down and help, and no character but
    /// `?`.
    pub(crate) fn is_bound(self, binding: &Binding, context: Context) -> bool {
        binding.scope.contains(context)
            && (self == Map::Classic || context != Startup || is_bound_at_startup(binding))
    }

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
            .find(|binding| binding.intent == intent && self.is_bound(binding, context))
    }
}
