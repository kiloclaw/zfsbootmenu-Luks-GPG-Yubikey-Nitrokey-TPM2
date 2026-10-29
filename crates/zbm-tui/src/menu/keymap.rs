//! The key map of the menu: one table turns key presses into intents,
//! writes the footer and the help view (`docs/dev/menu-design/keymap.md`).
//!
//! One key has one meaning in every view. The classic `MOD+key` bindings of
//! the `fzf` menu (`MOD` is `Alt` or `Ctrl`) are aliases with their old
//! meaning in their old view ([`CLASSIC`]); they never collide with the new
//! keys, which hold no modifier.

use zbm_display::keys::{Key, KeyPress};

use crate::text::{Id, id};

/// Where the focus is: which bindings apply.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Context {
    /// The start-up view.
    Startup,
    /// The main view.
    Main,
    /// The snapshots of an environment.
    Snapshots,
    /// The kernels of an environment.
    Kernels,
    /// Pools and LUKS volumes.
    Pools,
    /// Recovery tools.
    Recovery,
    /// The log.
    Log,
    /// Help.
    Help,
    /// The files changed since a snapshot.
    Diff,
    /// The packages changed since a snapshot.
    Packages,
    /// A choice among actions (the snapshot actions, power).
    Choose,
    /// A field that takes text: the filter, the palette, the command line,
    /// a secret, a typed confirmation.
    Text,
}

/// What a bound key asks for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Intent {
    /// The primary action of the view.
    Primary,
    /// Back: close the dialog, end the filter, return to the main view.
    Back,
    /// Select the entry above.
    Up,
    /// Select the entry below.
    Down,
    /// A page up.
    PageUp,
    /// A page down.
    PageDown,
    /// The first entry.
    First,
    /// The last entry.
    Last,
    /// Filter the list.
    Filter,
    /// The command palette.
    Palette,
    /// Help.
    Help,
    /// Redraw the screen.
    Redraw,
    /// Boot the selection.
    Boot,
    /// The kernels of the selected environment.
    Kernels,
    /// The snapshots of the selected environment.
    Snapshots,
    /// Edit the command line once.
    Edit,
    /// Make the selection the default; inherit when it is.
    SetDefault,
    /// Pools and volumes.
    Pools,
    /// Import the pool of the selection read/write.
    ReadWrite,
    /// Chroot into the selection.
    Chroot,
    /// The next sort order.
    Sort,
    /// The log.
    Log,
    /// Recovery tools.
    Recovery,
    /// Power off, reboot, firmware setup.
    Power,
    /// Roll back to the selected snapshot.
    Rollback,
    /// Clone the selected snapshot.
    Clone,
    /// Clone and promote.
    CloneAndPromote,
    /// Duplicate the selected snapshot.
    Duplicate,
    /// A new snapshot.
    NewSnapshot,
    /// Differences.
    Diff,
    /// Mark the selection.
    Mark,
    /// Unlock the selected volume.
    Unlock,
    /// Import the selected pool.
    Import,
    /// Rewind the pool to its checkpoint.
    Rewind,
    /// Revert the edited command line (classic `MOD+T`).
    RevertCommandLine,
    /// The recovery shell (classic `MOD+R` of the main screen).
    Shell,
    /// Inherit the default kernel (classic `MOD+U`).
    InheritDefault,
    /// The next keyboard layout, for what is typed.
    Layout,
    /// Show or hide the secret being typed.
    Reveal,
    /// The other segment of the changes: files or packages.
    Segment,
    /// This tab of the workspace.
    Tab(Tab),
    /// The next tab of the workspace.
    NextTab,
    /// The previous tab of the workspace.
    PreviousTab,
}

/// What a key press means in a context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Input {
    /// A bound intent.
    Intent(Intent),
    /// A character typed into a text field.
    Type(char),
    /// Remove the last character of a text field.
    Erase,
    /// Clear a text field.
    Clear,
}

/// The views where a binding applies: every view, or some.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Scope {
    /// Every view, text fields too.
    Everywhere,
    /// Every view but text fields (letters type there).
    Views,
    /// These views only.
    Only(&'static [Context]),
}

impl Scope {
    fn contains(self, context: Context) -> bool {
        match self {
            Scope::Everywhere => true,
            Scope::Views => context != Context::Text,
            Scope::Only(contexts) => contexts.contains(&context),
        }
    }
}

/// How a binding shows in the toolbar.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Footer {
    /// Not at all: an alternative key, which the help view still lists.
    Hidden,
    /// As the name of its key.
    Key,
    /// As this text, for a pair of keys (`Up/Down`).
    Keys(Id),
}

/// The modifiers held with a key, as the key map counts them.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Modifiers(u8);

impl Modifiers {
    /// None.
    pub const NONE: Modifiers = Modifiers(0);
    /// Shift.
    pub const SHIFT: Modifiers = Modifiers(1);
    /// Ctrl.
    pub const CTRL: Modifiers = Modifiers(2);
    /// Alt.
    pub const ALT: Modifiers = Modifiers(4);
    /// The logo key (Super).
    pub const LOGO: Modifiers = Modifiers(8);

    /// The modifiers of `press`. A character says itself whether Shift was
    /// held (`A`, `?`), and so does Shift-Tab (`BackTab`): for them Shift is
    /// left out, whatever the terminal or the keyboard reported.
    pub fn of(press: &KeyPress) -> Modifiers {
        let shift = press.shift && !matches!(press.key, Key::Char(_) | Key::BackTab);
        [
            (shift, Modifiers::SHIFT),
            (press.ctrl, Modifiers::CTRL),
            (press.alt, Modifiers::ALT),
            (press.logo, Modifiers::LOGO),
        ]
        .into_iter()
        .filter(|(held, _)| *held)
        .fold(Modifiers::NONE, |all, (_, modifier)| all | modifier)
    }

    /// Whether every modifier of `other` is held.
    pub fn contains(self, other: Modifiers) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether a modifier of `other` is held.
    pub fn has_any(self, other: Modifiers) -> bool {
        self.0 & other.0 != 0
    }
}

impl std::ops::BitOr for Modifiers {
    type Output = Modifiers;

    fn bitor(self, other: Modifiers) -> Modifiers {
        Modifiers(self.0 | other.0)
    }
}

impl std::ops::BitOrAssign for Modifiers {
    fn bitor_assign(&mut self, other: Modifiers) {
        self.0 |= other.0;
    }
}

/// `key` pressed with `modifiers`.
pub fn press(key: Key, modifiers: Modifiers) -> KeyPress {
    KeyPress {
        shift: modifiers.contains(Modifiers::SHIFT),
        ctrl: modifiers.contains(Modifiers::CTRL),
        alt: modifiers.contains(Modifiers::ALT),
        logo: modifiers.contains(Modifiers::LOGO),
        ..KeyPress::new(key)
    }
}

/// A key binding.
#[derive(Clone, Copy, Debug)]
pub struct Binding {
    /// The key.
    pub key: Key,
    /// The modifiers that must be held.
    pub modifiers: Modifiers,
    /// Where it applies.
    pub scope: Scope,
    /// What it asks for.
    pub intent: Intent,
    /// How the toolbar shows it.
    pub footer: Footer,
}

const NONE: Modifiers = Modifiers::NONE;
const CTRL: Modifiers = Modifiers::CTRL;

/// A binding; a function so that the table stays readable.
const fn bind(
    key: Key,
    modifiers: Modifiers,
    scope: Scope,
    intent: Intent,
    footer: Footer,
) -> Binding {
    Binding {
        key,
        modifiers,
        scope,
        intent,
        footer,
    }
}

/// A plain key bound in some views, shown in the footer as itself.
const fn key(c: char, contexts: &'static [Context], intent: Intent) -> Binding {
    bind(
        Key::Char(c),
        NONE,
        Scope::Only(contexts),
        intent,
        Footer::Key,
    )
}

mod classic;
mod labels;
mod workspace;

use self::classic::classic;
pub use self::classic::{CLASSIC, classic_bindings};
pub use self::labels::{TOOLBAR, TOOLBAR_GAP, Tool, bindings, describe, footer, help, key_name};
pub use self::workspace::{Map, Tab, WORKSPACE};

use Context::{Diff, Kernels, Log, Main, Packages, Pools, Recovery, Snapshots, Startup};

/// Views with a list.
const LISTS: &[Context] = &[Main, Snapshots, Kernels, Pools, Recovery, Diff, Packages];

/// Every view but help, dialogs and text fields.
const ALL_VIEWS: &[Context] = &[
    Startup, Main, Snapshots, Kernels, Pools, Recovery, Log, Diff, Packages,
];

/// The two segments of the changes since a snapshot.
const CHANGES: &[Context] = &[Diff, Packages];

/// Text fields.
const TEXT: &[Context] = &[Context::Text];

/// Every binding, in the order of the footer: most important first.
pub const KEYMAP: &[Binding] = &[
    bind(
        Key::Enter,
        NONE,
        Scope::Everywhere,
        Intent::Primary,
        Footer::Key,
    ),
    bind(
        Key::Char('b'),
        NONE,
        Scope::Only(&[Main, Kernels, Snapshots]),
        Intent::Boot,
        Footer::Hidden,
    ),
    key('k', &[Main], Intent::Kernels),
    key('s', &[Main], Intent::Snapshots),
    key('e', &[Main, Kernels], Intent::Edit),
    key('r', &[Snapshots], Intent::Rollback),
    key('c', &[Snapshots], Intent::Clone),
    key('u', &[Pools], Intent::Unlock),
    key('i', &[Pools], Intent::Import),
    key('d', &[Main, Kernels], Intent::SetDefault),
    key('p', &[Main], Intent::Pools),
    key('f', &[Snapshots], Intent::Diff),
    bind(
        Key::Right,
        NONE,
        Scope::Only(CHANGES),
        Intent::Segment,
        Footer::Keys(id!("keys-left-right")),
    ),
    bind(
        Key::Left,
        NONE,
        Scope::Only(CHANGES),
        Intent::Segment,
        Footer::Hidden,
    ),
    key('y', &[Snapshots], Intent::Duplicate),
    key('m', &[Snapshots], Intent::CloneAndPromote),
    key('n', &[Main, Snapshots], Intent::NewSnapshot),
    key('w', &[Main, Pools], Intent::ReadWrite),
    key('h', &[Pools], Intent::Rewind),
    key('j', &[Main, Snapshots], Intent::Chroot),
    bind(
        Key::Char('/'),
        NONE,
        Scope::Only(LISTS),
        Intent::Filter,
        Footer::Key,
    ),
    bind(
        Key::Char(':'),
        NONE,
        Scope::Only(ALL_VIEWS),
        Intent::Palette,
        Footer::Key,
    ),
    key('o', &[Snapshots], Intent::Sort),
    key('l', ALL_VIEWS, Intent::Log),
    key('!', ALL_VIEWS, Intent::Recovery),
    key('x', ALL_VIEWS, Intent::Power),
    bind(
        Key::Char('?'),
        NONE,
        Scope::Only(ALL_VIEWS),
        Intent::Help,
        Footer::Key,
    ),
    bind(
        Key::F(1),
        NONE,
        Scope::Everywhere,
        Intent::Help,
        Footer::Hidden,
    ),
    bind(
        Key::Escape,
        NONE,
        Scope::Everywhere,
        Intent::Back,
        Footer::Key,
    ),
    bind(
        Key::Char('c'),
        CTRL,
        Scope::Everywhere,
        Intent::Back,
        Footer::Hidden,
    ),
    bind(
        Key::Char(' '),
        NONE,
        Scope::Only(&[Snapshots, Diff]),
        Intent::Mark,
        Footer::Key,
    ),
    bind(
        Key::Tab,
        NONE,
        Scope::Only(&[Snapshots, Diff]),
        Intent::Mark,
        Footer::Hidden,
    ),
    bind(
        Key::Up,
        NONE,
        Scope::Everywhere,
        Intent::Up,
        Footer::Keys(id!("keys-up-down")),
    ),
    bind(
        Key::Down,
        NONE,
        Scope::Everywhere,
        Intent::Down,
        Footer::Hidden,
    ),
    bind(
        Key::Char('p'),
        CTRL,
        Scope::Everywhere,
        Intent::Up,
        Footer::Hidden,
    ),
    bind(
        Key::Char('n'),
        CTRL,
        Scope::Everywhere,
        Intent::Down,
        Footer::Hidden,
    ),
    bind(
        Key::PageUp,
        NONE,
        Scope::Everywhere,
        Intent::PageUp,
        Footer::Hidden,
    ),
    bind(
        Key::PageDown,
        NONE,
        Scope::Everywhere,
        Intent::PageDown,
        Footer::Hidden,
    ),
    bind(Key::Home, NONE, Scope::Views, Intent::First, Footer::Hidden),
    bind(Key::End, NONE, Scope::Views, Intent::Last, Footer::Hidden),
    bind(
        Key::Char('l'),
        CTRL,
        Scope::Everywhere,
        Intent::Redraw,
        Footer::Hidden,
    ),
    bind(
        Key::Char('k'),
        CTRL,
        Scope::Only(TEXT),
        Intent::Layout,
        Footer::Hidden,
    ),
    bind(
        Key::Char('r'),
        CTRL,
        Scope::Only(TEXT),
        Intent::Reveal,
        Footer::Hidden,
    ),
];

/// What `press` means in `context` with the key map `map`, if anything.
pub fn input(map: Map, context: Context, press: KeyPress) -> Option<Input> {
    let modifiers = Modifiers::of(&press);
    let bound = map.table().into_iter().find(|binding| {
        binding.key == press.key
            && binding.modifiers == modifiers
            && binding.scope.contains(context)
    });
    if let Some(binding) = bound {
        return Some(Input::Intent(binding.intent));
    }
    if context == Context::Text {
        return text(press);
    }
    classic(context, press).map(Input::Intent)
}

/// What a key means in a text field.
fn text(press: KeyPress) -> Option<Input> {
    let modifiers = Modifiers::of(&press);
    match press.key {
        Key::Backspace => Some(Input::Erase),
        Key::Char('u') if modifiers == CTRL => Some(Input::Clear),
        Key::Char(c) if !modifiers.has_any(Modifiers::CTRL | Modifiers::ALT) => {
            Some(Input::Type(c))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests;
