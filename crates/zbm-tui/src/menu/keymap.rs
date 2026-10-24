//! The key map of the menu: one table turns key presses into intents,
//! writes the footer and the help view (`docs/dev/menu-design/keymap.md`).
//!
//! One key has one meaning in every view. The classic `MOD+key` bindings of
//! the `fzf` menu (`MOD` is `Alt` or `Ctrl`) are aliases with their old
//! meaning in their old view ([`CLASSIC`]); they never collide with the new
//! keys, which hold no modifier.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::text::{self, Id, id, text};

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

/// A key binding.
#[derive(Clone, Copy, Debug)]
pub struct Binding {
    /// The key.
    pub code: KeyCode,
    /// The modifiers that must be held.
    pub modifiers: KeyModifiers,
    /// Where it applies.
    pub scope: Scope,
    /// What it asks for.
    pub intent: Intent,
    /// How the toolbar shows it.
    pub footer: Footer,
}

const NONE: KeyModifiers = KeyModifiers::NONE;
const CTRL: KeyModifiers = KeyModifiers::CONTROL;

/// A binding; a function so that the table stays readable.
const fn bind(
    code: KeyCode,
    modifiers: KeyModifiers,
    scope: Scope,
    intent: Intent,
    footer: Footer,
) -> Binding {
    Binding {
        code,
        modifiers,
        scope,
        intent,
        footer,
    }
}

/// A plain key bound in some views, shown in the footer as itself.
const fn key(c: char, contexts: &'static [Context], intent: Intent) -> Binding {
    bind(
        KeyCode::Char(c),
        NONE,
        Scope::Only(contexts),
        intent,
        Footer::Key,
    )
}

mod classic;

use self::classic::classic;
pub use self::classic::{CLASSIC, classic_bindings};

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
        KeyCode::Enter,
        NONE,
        Scope::Everywhere,
        Intent::Primary,
        Footer::Key,
    ),
    bind(
        KeyCode::Char('b'),
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
        KeyCode::Right,
        NONE,
        Scope::Only(CHANGES),
        Intent::Segment,
        Footer::Keys(id!("keys-left-right")),
    ),
    bind(
        KeyCode::Left,
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
        KeyCode::Char('/'),
        NONE,
        Scope::Only(LISTS),
        Intent::Filter,
        Footer::Key,
    ),
    bind(
        KeyCode::Char(':'),
        NONE,
        Scope::Only(ALL_VIEWS),
        Intent::Palette,
        Footer::Key,
    ),
    key('o', &[Main, Snapshots], Intent::Sort),
    key('l', ALL_VIEWS, Intent::Log),
    key('!', ALL_VIEWS, Intent::Recovery),
    key('x', ALL_VIEWS, Intent::Power),
    bind(
        KeyCode::Char('?'),
        NONE,
        Scope::Only(ALL_VIEWS),
        Intent::Help,
        Footer::Key,
    ),
    bind(
        KeyCode::F(1),
        NONE,
        Scope::Everywhere,
        Intent::Help,
        Footer::Hidden,
    ),
    bind(
        KeyCode::Esc,
        NONE,
        Scope::Everywhere,
        Intent::Back,
        Footer::Key,
    ),
    bind(
        KeyCode::Char('c'),
        CTRL,
        Scope::Everywhere,
        Intent::Back,
        Footer::Hidden,
    ),
    bind(
        KeyCode::Char(' '),
        NONE,
        Scope::Only(&[Snapshots, Diff]),
        Intent::Mark,
        Footer::Key,
    ),
    bind(
        KeyCode::Tab,
        NONE,
        Scope::Only(&[Snapshots, Diff]),
        Intent::Mark,
        Footer::Hidden,
    ),
    bind(
        KeyCode::Up,
        NONE,
        Scope::Everywhere,
        Intent::Up,
        Footer::Keys(id!("keys-up-down")),
    ),
    bind(
        KeyCode::Down,
        NONE,
        Scope::Everywhere,
        Intent::Down,
        Footer::Hidden,
    ),
    bind(
        KeyCode::Char('p'),
        CTRL,
        Scope::Everywhere,
        Intent::Up,
        Footer::Hidden,
    ),
    bind(
        KeyCode::Char('n'),
        CTRL,
        Scope::Everywhere,
        Intent::Down,
        Footer::Hidden,
    ),
    bind(
        KeyCode::PageUp,
        NONE,
        Scope::Everywhere,
        Intent::PageUp,
        Footer::Hidden,
    ),
    bind(
        KeyCode::PageDown,
        NONE,
        Scope::Everywhere,
        Intent::PageDown,
        Footer::Hidden,
    ),
    bind(
        KeyCode::Home,
        NONE,
        Scope::Views,
        Intent::First,
        Footer::Hidden,
    ),
    bind(
        KeyCode::End,
        NONE,
        Scope::Views,
        Intent::Last,
        Footer::Hidden,
    ),
    bind(
        KeyCode::Char('l'),
        CTRL,
        Scope::Everywhere,
        Intent::Redraw,
        Footer::Hidden,
    ),
    bind(
        KeyCode::Char('k'),
        CTRL,
        Scope::Only(TEXT),
        Intent::Layout,
        Footer::Hidden,
    ),
    bind(
        KeyCode::Char('r'),
        CTRL,
        Scope::Only(TEXT),
        Intent::Reveal,
        Footer::Hidden,
    ),
];

/// What `event` means in `context`, if anything.
pub fn input(context: Context, event: KeyEvent) -> Option<Input> {
    if event.kind == KeyEventKind::Release {
        return None;
    }
    let bound = KEYMAP.iter().find(|binding| {
        binding.code == event.code
            && binding.modifiers == event.modifiers
            && binding.scope.contains(context)
    });
    if let Some(binding) = bound {
        return Some(Input::Intent(binding.intent));
    }
    if context == Context::Text {
        return text(event);
    }
    classic(context, event).map(Input::Intent)
}

/// What a key means in a text field.
fn text(event: KeyEvent) -> Option<Input> {
    match event.code {
        KeyCode::Backspace => Some(Input::Erase),
        KeyCode::Char('u') if event.modifiers == CTRL => Some(Input::Clear),
        KeyCode::Char(c)
            if !event
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            Some(Input::Type(c))
        }
        _ => None,
    }
}

/// What a key does for an intent in a context, as the toolbar and the
/// help view say it: a message of the catalogue with a `.title`.
pub fn describe(context: Context, intent: Intent) -> Id {
    match (context, intent) {
        (Main | Kernels, Intent::Primary) | (_, Intent::Boot) => id!("action-boot"),
        (Snapshots | Context::Choose, Intent::Primary) => id!("action-choose"),
        (Pools, Intent::Primary) => id!("action-details"),
        (Diff, Intent::Primary) => id!("action-restore"),
        (Packages, Intent::Primary) => id!("action-show-files"),
        (Recovery, Intent::Primary) => id!("action-run"),
        (Startup | Context::Text, Intent::Primary) => id!("action-confirm"),
        (Context::Choose | Context::Text, Intent::Back) => id!("action-cancel"),
        (_, Intent::Back) => id!("action-back"),
        (_, Intent::Kernels) => id!("action-kernels"),
        (_, Intent::Snapshots) => id!("action-snapshots"),
        (_, Intent::Edit) => id!("action-edit"),
        (_, Intent::SetDefault) => id!("action-default"),
        (_, Intent::Pools) => id!("action-pools"),
        (_, Intent::ReadWrite) => id!("action-read-write"),
        (_, Intent::Chroot) => id!("action-chroot"),
        (_, Intent::Sort) => id!("action-sort"),
        (_, Intent::Log) => id!("action-log"),
        (_, Intent::Recovery) => id!("action-recovery"),
        (_, Intent::Power) => id!("action-power"),
        (_, Intent::Rollback) => id!("action-roll-back"),
        (_, Intent::Clone) => id!("action-clone"),
        (_, Intent::CloneAndPromote) => id!("action-clone-promote"),
        (_, Intent::Duplicate) => id!("action-duplicate"),
        (_, Intent::NewSnapshot) => id!("action-snapshot"),
        (_, Intent::Diff) => id!("action-diff"),
        (_, Intent::Mark) => id!("action-mark"),
        (_, Intent::Unlock) => id!("action-unlock"),
        (_, Intent::Import) => id!("action-import"),
        (_, Intent::Rewind) => id!("action-rewind"),
        (_, Intent::Filter) => id!("action-filter"),
        (_, Intent::Palette) => id!("action-commands"),
        (_, Intent::Help) => id!("action-help"),
        (Context::Help | Context::Log, Intent::Up | Intent::Down) => id!("action-scroll"),
        (_, Intent::Primary | Intent::Up | Intent::Down) => id!("action-select"),
        (_, Intent::PageUp | Intent::PageDown | Intent::First | Intent::Last) => {
            id!("action-move")
        }
        (_, Intent::Redraw) => id!("action-redraw"),
        (_, Intent::RevertCommandLine) => id!("action-revert"),
        (_, Intent::Shell) => id!("action-shell"),
        (_, Intent::InheritDefault) => id!("action-inherit"),
        (_, Intent::Layout) => id!("action-keyboard-layout"),
        (_, Intent::Reveal) => id!("action-show-typed"),
        (_, Intent::Segment) => id!("action-segment"),
    }
}

/// The bindings of `context` shown in the toolbar, in order: (intent, key,
/// what it does).
pub fn bindings(context: Context) -> Vec<(Intent, String, Id)> {
    let idle = matches!(context, Context::Startup | Context::Help | Context::Log);
    KEYMAP
        .iter()
        .filter(|binding| binding.scope.contains(context))
        .filter(|binding| !(idle && binding.intent == Intent::Primary))
        .filter_map(|binding| {
            let key = match binding.footer {
                Footer::Hidden => return None,
                Footer::Key => key_name(binding.code, binding.modifiers),
                Footer::Keys(id) => text::get(id),
            };
            Some((binding.intent, key, describe(context, binding.intent)))
        })
        .collect()
}

/// The most actions the toolbar shows (`apple-hig.md`, rule 9).
pub const TOOLBAR: usize = 5;

/// The space between toolbar items.
pub const TOOLBAR_GAP: usize = 3;

/// An action of the toolbar.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Tool {
    /// Its key.
    pub key: String,
    /// What it does, as a title.
    pub label: String,
    /// Whether it is `? More`, which opens help with every key.
    pub is_more: bool,
}

impl Tool {
    /// The columns it takes: its key, a space and its label.
    pub fn width(&self) -> usize {
        self.key.chars().count() + 1 + self.label.chars().count()
    }
}

/// The toolbar of `context` for a screen `width` columns wide: at most
/// [`TOOLBAR`] actions that fit whole, as `Enter Boot`, the most important
/// first; then `? More`, which opens help with every key, wherever help can
/// be opened.
pub fn footer(context: Context, width: usize) -> Vec<Tool> {
    let all = bindings(context);
    let more = all
        .iter()
        .find(|(intent, ..)| *intent == Intent::Help)
        .map(|(_, key, _)| Tool {
            key: key.clone(),
            label: text::title(id!("action-more")),
            is_more: true,
        });
    let mut used = more.as_ref().map_or(0, |more| more.width() + TOOLBAR_GAP);
    let mut shown = Vec::new();
    for (_, key, what) in all
        .into_iter()
        .filter(|(intent, ..)| *intent != Intent::Help)
    {
        let tool = Tool {
            key,
            label: text::title(what),
            is_more: false,
        };
        let gap = if shown.is_empty() { 0 } else { TOOLBAR_GAP };
        if shown.len() == TOOLBAR || used + gap + tool.width() > width {
            break;
        }
        used += gap + tool.width();
        shown.push(tool);
    }
    shown.extend(more.filter(|more| more.width() <= width));
    shown
}

/// The name of a key, as the help view shows it.
pub fn key_name(code: KeyCode, modifiers: KeyModifiers) -> String {
    let name = match code {
        KeyCode::Enter => text!("key-enter"),
        KeyCode::Esc => text!("key-esc"),
        KeyCode::Tab => text!("key-tab"),
        KeyCode::Up => text!("key-up"),
        KeyCode::Down => text!("key-down"),
        KeyCode::Left => text!("key-left"),
        KeyCode::Right => text!("key-right"),
        KeyCode::PageUp => text!("key-page-up"),
        KeyCode::PageDown => text!("key-page-down"),
        KeyCode::Home => text!("key-home"),
        KeyCode::End => text!("key-end"),
        KeyCode::F(number) => text!("key-function", number = number),
        KeyCode::Char(' ') => text!("key-space"),
        KeyCode::Char(c) => c.to_string(),
        other => format!("{other:?}"),
    };
    if modifiers.contains(KeyModifiers::CONTROL) {
        text!("key-ctrl", key = name.to_uppercase())
    } else {
        name
    }
}

/// Every binding of `context` for the help view, the keys of one intent
/// together: (keys, what they do).
pub fn help(context: Context) -> Vec<(String, Id)> {
    let mut lines: Vec<(Intent, Vec<String>)> = Vec::new();
    for binding in KEYMAP
        .iter()
        .filter(|binding| binding.scope.contains(context))
    {
        let name = key_name(binding.code, binding.modifiers);
        match lines
            .iter_mut()
            .find(|(intent, _)| *intent == binding.intent)
        {
            Some((_, keys)) => keys.push(name),
            None => lines.push((binding.intent, vec![name])),
        }
    }
    lines
        .into_iter()
        .map(|(intent, keys)| (keys.join(", "), describe(context, intent)))
        .collect()
}

#[cfg(test)]
mod tests;
