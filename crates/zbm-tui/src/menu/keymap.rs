//! The key map of the menu: one table turns key presses into intents,
//! writes the footer and the help view (`docs/dev/menu-design/keymap.md`).
//!
//! One key has one meaning in every view. The classic `MOD+key` bindings of
//! the `fzf` menu (`MOD` is `Alt` or `Ctrl`) are aliases with their old
//! meaning in their old view ([`CLASSIC`]); they never collide with the new
//! keys, which hold no modifier.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

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
    /// The key as the footer shows it; `None` keeps it out of the footer
    /// (an alternative key), and the help view still lists it.
    pub footer: Option<&'static str>,
}

const NONE: KeyModifiers = KeyModifiers::NONE;
const CTRL: KeyModifiers = KeyModifiers::CONTROL;

/// A binding; a function so that the table stays readable.
const fn bind(
    code: KeyCode,
    modifiers: KeyModifiers,
    scope: Scope,
    intent: Intent,
    footer: Option<&'static str>,
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
const fn key(
    c: char,
    footer: &'static str,
    contexts: &'static [Context],
    intent: Intent,
) -> Binding {
    bind(
        KeyCode::Char(c),
        NONE,
        Scope::Only(contexts),
        intent,
        Some(footer),
    )
}

use Context::{Kernels, Log, Main, Pools, Recovery, Snapshots, Startup};

/// Views with a list.
const LISTS: &[Context] = &[Main, Snapshots, Kernels, Pools, Recovery];

/// Every view but help, dialogs and text fields.
const ALL_VIEWS: &[Context] = &[Startup, Main, Snapshots, Kernels, Pools, Recovery, Log];

/// Every binding, in the order of the footer: most important first.
pub const KEYMAP: &[Binding] = &[
    bind(
        KeyCode::Enter,
        NONE,
        Scope::Everywhere,
        Intent::Primary,
        Some("Enter"),
    ),
    bind(
        KeyCode::Char('b'),
        NONE,
        Scope::Only(&[Main, Kernels, Snapshots]),
        Intent::Boot,
        None,
    ),
    key('k', "k", &[Main], Intent::Kernels),
    key('s', "s", &[Main], Intent::Snapshots),
    key('e', "e", &[Main, Kernels], Intent::Edit),
    key('r', "r", &[Snapshots], Intent::Rollback),
    key('c', "c", &[Snapshots], Intent::Clone),
    key('u', "u", &[Pools], Intent::Unlock),
    key('i', "i", &[Pools], Intent::Import),
    key('d', "d", &[Main, Kernels], Intent::SetDefault),
    key('p', "p", &[Main], Intent::Pools),
    key('f', "f", &[Snapshots], Intent::Diff),
    key('y', "y", &[Snapshots], Intent::Duplicate),
    key('m', "m", &[Snapshots], Intent::CloneAndPromote),
    key('n', "n", &[Main, Snapshots], Intent::NewSnapshot),
    key('w', "w", &[Main, Pools], Intent::ReadWrite),
    key('h', "h", &[Pools], Intent::Rewind),
    key('j', "j", &[Main, Snapshots], Intent::Chroot),
    bind(
        KeyCode::Char('/'),
        NONE,
        Scope::Only(LISTS),
        Intent::Filter,
        Some("/"),
    ),
    bind(
        KeyCode::Char(':'),
        NONE,
        Scope::Only(ALL_VIEWS),
        Intent::Palette,
        Some(":"),
    ),
    key('o', "o", &[Main, Snapshots], Intent::Sort),
    key('l', "l", ALL_VIEWS, Intent::Log),
    key('!', "!", ALL_VIEWS, Intent::Recovery),
    key('x', "x", ALL_VIEWS, Intent::Power),
    bind(
        KeyCode::Char('?'),
        NONE,
        Scope::Only(ALL_VIEWS),
        Intent::Help,
        Some("?"),
    ),
    bind(KeyCode::F(1), NONE, Scope::Everywhere, Intent::Help, None),
    bind(
        KeyCode::Esc,
        NONE,
        Scope::Everywhere,
        Intent::Back,
        Some("Esc"),
    ),
    bind(
        KeyCode::Char('c'),
        CTRL,
        Scope::Everywhere,
        Intent::Back,
        None,
    ),
    bind(
        KeyCode::Char(' '),
        NONE,
        Scope::Only(&[Snapshots]),
        Intent::Mark,
        Some("Space"),
    ),
    bind(
        KeyCode::Tab,
        NONE,
        Scope::Only(&[Snapshots]),
        Intent::Mark,
        None,
    ),
    bind(
        KeyCode::Up,
        NONE,
        Scope::Everywhere,
        Intent::Up,
        Some("Up/Down"),
    ),
    bind(KeyCode::Down, NONE, Scope::Everywhere, Intent::Down, None),
    bind(
        KeyCode::Char('p'),
        CTRL,
        Scope::Everywhere,
        Intent::Up,
        None,
    ),
    bind(
        KeyCode::Char('n'),
        CTRL,
        Scope::Everywhere,
        Intent::Down,
        None,
    ),
    bind(
        KeyCode::PageUp,
        NONE,
        Scope::Everywhere,
        Intent::PageUp,
        None,
    ),
    bind(
        KeyCode::PageDown,
        NONE,
        Scope::Everywhere,
        Intent::PageDown,
        None,
    ),
    bind(KeyCode::Home, NONE, Scope::Views, Intent::First, None),
    bind(KeyCode::End, NONE, Scope::Views, Intent::Last, None),
    bind(
        KeyCode::Char('l'),
        CTRL,
        Scope::Everywhere,
        Intent::Redraw,
        None,
    ),
];

/// The classic bindings of the `fzf` menu: a letter with `Alt` or `Ctrl`,
/// and its meaning in each view where it had one.
pub const CLASSIC: &[(char, &[(Context, Intent)])] = &[
    ('k', &[(Main, Intent::Kernels)]),
    ('s', &[(Main, Intent::Snapshots)]),
    (
        'd',
        &[
            (Main, Intent::SetDefault),
            (Snapshots, Intent::Diff),
            (Kernels, Intent::SetDefault),
        ],
    ),
    ('e', &[(Main, Intent::Edit), (Kernels, Intent::Edit)]),
    ('t', &[(Main, Intent::RevertCommandLine)]),
    ('p', &[(Main, Intent::Pools)]),
    (
        'r',
        &[
            (Main, Intent::Shell),
            (Snapshots, Intent::Rollback),
            (Pools, Intent::Rewind),
        ],
    ),
    ('j', &[(Main, Intent::Chroot), (Snapshots, Intent::Chroot)]),
    ('w', &[(Main, Intent::ReadWrite)]),
    ('o', &[(Main, Intent::Sort), (Snapshots, Intent::Sort)]),
    (
        'l',
        &[
            (Main, Intent::Log),
            (Snapshots, Intent::Log),
            (Kernels, Intent::Log),
            (Pools, Intent::Log),
        ],
    ),
    (
        'x',
        &[(Main, Intent::Power), (Snapshots, Intent::CloneAndPromote)],
    ),
    ('c', &[(Snapshots, Intent::Clone)]),
    ('n', &[(Snapshots, Intent::NewSnapshot)]),
    ('u', &[(Kernels, Intent::InheritDefault)]),
];

/// The intent of a classic binding: `Alt` or `Ctrl` with a letter.
fn classic(context: Context, event: KeyEvent) -> Option<Intent> {
    let KeyCode::Char(c) = event.code else {
        return None;
    };
    let modifier = event
        .modifiers
        .intersects(KeyModifiers::ALT | KeyModifiers::CONTROL);
    if !modifier || context == Context::Text {
        return None;
    }
    CLASSIC
        .iter()
        .find(|(letter, _)| *letter == c.to_ascii_lowercase())
        .and_then(|(_, meanings)| {
            meanings
                .iter()
                .find(|(view, _)| *view == context)
                .map(|(_, intent)| *intent)
        })
}

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

/// What the footer says for an intent in a context.
pub fn describe(context: Context, intent: Intent) -> &'static str {
    match (context, intent) {
        (Main | Kernels, Intent::Primary) | (_, Intent::Boot) => "boot",
        (Snapshots | Context::Choose, Intent::Primary) => "choose",
        (Pools, Intent::Primary) => "details",
        (Recovery, Intent::Primary) => "run",
        (Startup | Context::Text, Intent::Primary) => "confirm",
        (Context::Choose | Context::Text, Intent::Back) => "cancel",
        (_, Intent::Back) => "back",
        (_, Intent::Kernels) => "kernels",
        (_, Intent::Snapshots) => "snapshots",
        (_, Intent::Edit) => "edit",
        (_, Intent::SetDefault) => "default",
        (_, Intent::Pools) => "pools",
        (_, Intent::ReadWrite) => "read/write",
        (_, Intent::Chroot) => "chroot",
        (_, Intent::Sort) => "sort",
        (_, Intent::Log) => "log",
        (_, Intent::Recovery) => "recovery",
        (_, Intent::Power) => "power",
        (_, Intent::Rollback) => "roll back",
        (_, Intent::Clone) => "clone",
        (_, Intent::CloneAndPromote) => "clone+promote",
        (_, Intent::Duplicate) => "duplicate",
        (_, Intent::NewSnapshot) => "snapshot",
        (_, Intent::Diff) => "diff",
        (_, Intent::Mark) => "mark",
        (_, Intent::Unlock) => "unlock",
        (_, Intent::Import) => "import",
        (_, Intent::Rewind) => "rewind",
        (_, Intent::Filter) => "filter",
        (_, Intent::Palette) => "commands",
        (_, Intent::Help) => "help",
        (Context::Help | Context::Log, Intent::Up | Intent::Down) => "scroll",
        (_, Intent::Primary | Intent::Up | Intent::Down) => "select",
        (_, Intent::PageUp | Intent::PageDown | Intent::First | Intent::Last) => "move",
        (_, Intent::Redraw) => "redraw",
        (_, Intent::RevertCommandLine) => "revert",
        (_, Intent::Shell) => "shell",
        (_, Intent::InheritDefault) => "inherit",
    }
}

/// The bindings of `context` shown in the footer, in order: (key, what it
/// does).
pub fn bindings(context: Context) -> Vec<(&'static str, &'static str)> {
    let idle = matches!(context, Context::Startup | Context::Help | Context::Log);
    KEYMAP
        .iter()
        .filter(|binding| binding.scope.contains(context))
        .filter(|binding| !(idle && binding.intent == Intent::Primary))
        .filter_map(|binding| Some((binding.footer?, describe(context, binding.intent))))
        .collect()
}

/// The most actions the toolbar shows (`apple-hig.md`, rule 9).
pub const TOOLBAR: usize = 5;

/// The space between toolbar items.
pub const TOOLBAR_GAP: usize = 3;

/// The toolbar of `context` for a screen `width` columns wide, as (key,
/// what): at most [`TOOLBAR`] actions that fit whole, as `Enter Boot`, the
/// most important first; then `? more`, which opens help with every key,
/// wherever help can be opened.
pub fn footer(context: Context, width: usize) -> Vec<(&'static str, &'static str)> {
    let all = bindings(context);
    let more = all.iter().any(|(key, _)| *key == "?");
    let mut used = if more {
        "? More".len() + TOOLBAR_GAP
    } else {
        0
    };
    let mut shown = Vec::new();
    for (key, help) in all.into_iter().filter(|(key, _)| *key != "?") {
        let length = key.len() + 1 + help.len();
        let gap = if shown.is_empty() { 0 } else { TOOLBAR_GAP };
        if shown.len() == TOOLBAR || used + gap + length > width {
            break;
        }
        used += gap + length;
        shown.push((key, help));
    }
    if more && "? More".len() <= width {
        shown.push(("?", "more"));
    }
    shown
}

/// The name of a key, as the help view shows it.
pub fn key_name(code: KeyCode, modifiers: KeyModifiers) -> String {
    let name = match code {
        KeyCode::Enter => "Enter".to_owned(),
        KeyCode::Esc => "Esc".to_owned(),
        KeyCode::Tab => "Tab".to_owned(),
        KeyCode::Up => "Up".to_owned(),
        KeyCode::Down => "Down".to_owned(),
        KeyCode::PageUp => "PgUp".to_owned(),
        KeyCode::PageDown => "PgDn".to_owned(),
        KeyCode::Home => "Home".to_owned(),
        KeyCode::End => "End".to_owned(),
        KeyCode::F(n) => format!("F{n}"),
        KeyCode::Char(' ') => "Space".to_owned(),
        KeyCode::Char(c) => c.to_string(),
        other => format!("{other:?}"),
    };
    if modifiers.contains(KeyModifiers::CONTROL) {
        format!("Ctrl-{}", name.to_ascii_uppercase())
    } else {
        name
    }
}

/// Every binding of `context` for the help view, the keys of one intent
/// together: (keys, what they do).
pub fn help(context: Context) -> Vec<(String, &'static str)> {
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

/// The classic bindings of `context`, as (`MOD+K`, what it does).
pub fn classic_bindings(context: Context) -> Vec<(String, &'static str)> {
    CLASSIC
        .iter()
        .filter_map(|(letter, meanings)| {
            let (_, intent) = meanings.iter().find(|(view, _)| *view == context)?;
            let key = format!("MOD+{}", letter.to_ascii_uppercase());
            Some((key, describe(context, *intent)))
        })
        .collect()
}

#[cfg(test)]
mod tests;
