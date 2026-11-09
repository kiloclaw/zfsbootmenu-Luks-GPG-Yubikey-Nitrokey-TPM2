//! The classic bindings of the `fzf` menu: `MOD+key`, where `MOD` is `Alt`
//! or `Ctrl`, with their old meaning in their old view (decision 8 of
//! `docs/dev/menu-design/keymap.md`). Where the menu binds the letter with
//! `Ctrl` itself, as `Ctrl-P`, the alias works with `Alt` only: `Alt+P`.

use zbm_display::keys::{Key, KeyPress};

use super::{Context, Input, Intent, Map, Modifiers, describe, input, press};
use crate::text::{Id, text};
use Context::{Kernels, Main, Pools, Snapshots};

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
    ('o', &[(Snapshots, Intent::Sort)]),
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
pub(super) fn classic(context: Context, press: KeyPress) -> Option<Intent> {
    let Key::Char(c) = press.key else {
        return None;
    };
    let modifier = Modifiers::of(&press).has_any(Modifiers::ALT | Modifiers::CTRL);
    if !modifier || matches!(context, Context::Text | Context::Command) {
        return None;
    }
    CLASSIC
        .iter()
        .find(|(letter, _)| *letter == c)
        .and_then(|(_, meanings)| {
            meanings
                .iter()
                .find(|(view, _)| *view == context)
                .map(|(_, intent)| *intent)
        })
}

/// The classic bindings of `context` whose intents are `offered`, as
/// (`MOD+K`, what it does), or `Alt+K` where the letter with `Ctrl` means
/// something else there.
pub fn classic_bindings(context: Context, offered: &dyn Fn(Intent) -> bool) -> Vec<(String, Id)> {
    aliases(context, offered)
        .into_iter()
        .map(|(_, key, what)| (key, what))
        .collect()
}

/// The classic bindings of `context` whose intents are `offered`, as
/// (letter, `MOD+K` or `Alt+K`, what it does): the help view's list and
/// the manual's.
pub(super) fn aliases(
    context: Context,
    offered: &dyn Fn(Intent) -> bool,
) -> Vec<(char, String, Id)> {
    CLASSIC
        .iter()
        .filter_map(|(letter, meanings)| {
            let (_, intent) = meanings
                .iter()
                .find(|(view, intent)| *view == context && offered(*intent))?;
            let key = alias_name(*letter, context);
            Some((*letter, key, describe(context, *intent)))
        })
        .collect()
}

/// The name of the classic binding of `letter` in `context`, as the help
/// view shows it: `MOD+K`, or `Alt+K` where the letter with `Ctrl` means
/// something else there.
pub(super) fn alias_name(letter: char, context: Context) -> String {
    let upper = letter.to_uppercase().to_string();
    if taken_with_ctrl(letter, context).is_some() {
        text!("key-alt", letter = upper)
    } else {
        text!("key-classic", letter = upper)
    }
}

/// What the workspace's table binds `letter` with `Ctrl` to in `context`,
/// if that is not the classic binding's meaning: the table comes first,
/// and the classic binding then works with `Alt` only (`Ctrl-P` selects
/// the entry above).
pub(super) fn taken_with_ctrl(letter: char, context: Context) -> Option<Intent> {
    let ctrl = press(Key::Char(letter), Modifiers::CTRL);
    match input(Map::Workspace, context, ctrl) {
        Some(Input::Intent(intent)) if classic(context, ctrl) != Some(intent) => Some(intent),
        _ => None,
    }
}

/// The key a terminal's control character for `letter` with `Ctrl` reads
/// as, when that is another key: a serial line and SSH send `Ctrl-J` as a
/// newline, which is `Enter`. The display console reads the keyboard
/// itself, and its `Ctrl-J` is `Ctrl-J`.
pub(super) fn sent_as(letter: char) -> Option<KeyPress> {
    let byte = u8::try_from(letter).ok().filter(u8::is_ascii_lowercase)? - b'a' + 1;
    let mut keys = Vec::new();
    zbm_display::term_keys::decode(&[byte], true, &mut keys);
    let ctrl = press(Key::Char(letter), Modifiers::CTRL);
    keys.into_iter().next().filter(|key| *key != ctrl)
}
