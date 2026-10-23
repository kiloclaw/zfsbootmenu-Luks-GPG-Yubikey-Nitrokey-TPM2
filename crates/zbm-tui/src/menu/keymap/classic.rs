//! The classic bindings of the `fzf` menu: `MOD+key`, where `MOD` is `Alt`
//! or `Ctrl`, with their old meaning in their old view (decision 8 of
//! `docs/dev/menu-design/keymap.md`).

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{Context, Intent, describe};
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
pub(super) fn classic(context: Context, event: KeyEvent) -> Option<Intent> {
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
