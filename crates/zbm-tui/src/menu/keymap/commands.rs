//! The commands of the command palette. What only a character or a chord
//! reaches, which rule 1 of decision 18 (ADR 0007) puts in the palette:
//! the palette lists each such action of the view under it with its keys,
//! and choosing one does what the key does. Not listed: what another key
//! reaches there (`Enter`, an arrow, `Tab`, `Esc`, `Delete`, a page key,
//! `Home`, `End` or a function key), what the palette names by entries of
//! its own, and what an `Enter` menu of the view reaches. A text field
//! types the characters, and its chords (`Ctrl-K`, `Ctrl-R`) have no
//! palette there. And the commands' names (rule 4), which the palette
//! finds in the menu's language and in English.

use zbm_display::keys::Key;

use super::classic::{CLASSIC, alias_name};
use super::{Context, Intent, Map, Tab, is_text, key_name};
use crate::text::{Id, id};

/// What the palette names by entries of its own: the boot of an
/// environment, its kernels and snapshots, its default, a snapshot of it,
/// a chroot into it, its command line reverted, and the default kernel
/// inherited; a pool's imports, read/write or read-only; the views, the
/// recovery shell, power, and the palette itself.
const PALETTE_HAS: &[Intent] = &[
    Intent::Boot,
    Intent::Kernels,
    Intent::Snapshots,
    Intent::SetDefault,
    Intent::NewSnapshot,
    Intent::Chroot,
    Intent::RevertCommandLine,
    Intent::InheritDefault,
    Intent::Import,
    Intent::ReadWrite,
    Intent::Pools,
    Intent::Log,
    Intent::Recovery,
    Intent::Power,
    Intent::Shell,
    Intent::Palette,
];

/// The names of the palette's commands, each with what it does: what a
/// query finds an entry by, before its target, as `boot zroot/ROOT/void`
/// (rule 4 of decision 18). The help of the command line and
/// `zfsbootmenu(7)` list them.
pub(crate) const COMMAND_NAMES: &[(Id, Id)] = &[
    (id!("command-boot"), id!("command-boot.what")),
    (id!("command-edit"), id!("command-edit.what")),
    (id!("command-revert"), id!("command-revert.what")),
    (id!("command-commandline"), id!("command-commandline.what")),
    (
        id!("command-inherit-commandline"),
        id!("command-inherit-commandline.what"),
    ),
    (id!("command-default"), id!("command-default.what")),
    (id!("command-inherit"), id!("command-inherit.what")),
    (id!("command-snapshot"), id!("command-snapshot.what")),
    (id!("command-chroot"), id!("command-chroot.what")),
    (id!("command-tries"), id!("command-tries.what")),
    (id!("command-rollback"), id!("command-rollback.what")),
    (id!("command-clone"), id!("command-clone.what")),
    (id!("command-promote"), id!("command-promote.what")),
    (id!("command-duplicate"), id!("command-duplicate.what")),
    (id!("command-diff"), id!("command-diff.what")),
    (id!("command-destroy"), id!("command-destroy.what")),
    (id!("command-export"), id!("command-export.what")),
    (id!("command-import"), id!("command-import.what")),
    (
        id!("command-import-last-host"),
        id!("command-import-last-host.what"),
    ),
    (
        id!("command-import-force"),
        id!("command-import-force.what"),
    ),
    (id!("command-rewind"), id!("command-rewind.what")),
    (id!("command-read-write"), id!("command-read-write.what")),
    (id!("command-read-only"), id!("command-read-only.what")),
    (id!("command-unlock"), id!("command-unlock.what")),
    (id!("command-reseal"), id!("command-reseal.what")),
    (id!("command-rescan"), id!("command-rescan.what")),
    (id!("command-shell"), id!("command-shell.what")),
    (id!("command-power"), id!("command-power.what")),
    (id!("command-sort"), id!("command-sort.what")),
    (id!("command-fold"), id!("command-fold.what")),
    (id!("command-mark"), id!("command-mark.what")),
    (id!("command-filter"), id!("command-filter.what")),
    (id!("command-go"), id!("command-go.what")),
    (id!("command-kernels"), id!("command-kernels.what")),
    (id!("command-snapshots"), id!("command-snapshots.what")),
    (id!("command-pools"), id!("command-pools.what")),
    (id!("command-recovery"), id!("command-recovery.what")),
    (id!("command-log"), id!("command-log.what")),
    (id!("command-help"), id!("command-help.what")),
];

/// The name of the command that does what `intent` does, as
/// [`COMMAND_NAMES`] has it; `None` for what moves, chooses, cancels or
/// types, and for the command line itself, which no command opens.
pub(crate) fn command_name(intent: Intent) -> Option<Id> {
    use Intent::{
        Back, Complete, DeleteWord, Down, First, History, Last, Layout, NextTab, PageDown, PageUp,
        Palette, PreviousTab, Primary, Redraw, Reveal, Segment, Tab, Up,
    };
    let name = match intent {
        Primary | Back | Up | Down | PageUp | PageDown | First | Last | Palette | Redraw
        | Layout | Reveal | Segment | Tab(_) | NextTab | PreviousTab | Complete | DeleteWord
        | History => return None,
        Intent::Boot => id!("command-boot"),
        Intent::Edit => id!("command-edit"),
        Intent::RevertCommandLine => id!("command-revert"),
        Intent::SetDefault => id!("command-default"),
        Intent::InheritDefault => id!("command-inherit"),
        Intent::NewSnapshot => id!("command-snapshot"),
        Intent::Chroot => id!("command-chroot"),
        Intent::Rollback => id!("command-rollback"),
        Intent::Clone => id!("command-clone"),
        Intent::CloneAndPromote => id!("command-promote"),
        Intent::Duplicate => id!("command-duplicate"),
        Intent::Diff => id!("command-diff"),
        Intent::Destroy => id!("command-destroy"),
        Intent::Import => id!("command-import"),
        Intent::Rewind => id!("command-rewind"),
        Intent::ReadWrite => id!("command-read-write"),
        Intent::Unlock => id!("command-unlock"),
        Intent::Shell => id!("command-shell"),
        Intent::Power => id!("command-power"),
        Intent::Sort => id!("command-sort"),
        Intent::Fold => id!("command-fold"),
        Intent::Mark => id!("command-mark"),
        Intent::Filter => id!("command-filter"),
        Intent::Kernels => id!("command-kernels"),
        Intent::Snapshots => id!("command-snapshots"),
        Intent::Pools => id!("command-pools"),
        Intent::Recovery => id!("command-recovery"),
        Intent::Log => id!("command-log"),
        Intent::Help => id!("command-help"),
    };
    Some(name)
}

/// Where rule 1 of decision 18 finds an action that a character reaches in
/// the workspace, as the maintainer amended it: the `Enter` menu of what
/// it applies to, but where `Enter` is the item's primary action, and for
/// what applies to a view rather than an item.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Reach {
    /// An entry of the `Enter` menu of the item it applies to, a
    /// snapshot's sheet or a pool's details, which hold every import of
    /// `i`; or what `Enter` does itself, a volume's unlock, an environment's
    /// or a kernel's boot. The command line lists no key's entry of it.
    Menu,
    /// An entry of that `Enter` menu in the workspace, and a key's entry of
    /// the command line too, as the classic map's menu does not reach it in
    /// full: a pool's rewind, which the classic details offer only where a
    /// checkpoint is known; a snapshot's diff, whose key diffs the marked
    /// ones too, which the workspace's sheet offers. With one snapshot
    /// marked, the diff is that one's, whose own sheet offers it.
    MenuAndCommand,
    /// An entry of the command line, where `Enter` is the item's primary
    /// action, an environment's or a kernel's boot, as rule 1 is amended.
    /// The inspector's next steps list the first four keys too: of an
    /// environment its kernels, snapshots, edit and default, not its new
    /// snapshot; of a kernel its edit and default, not the default
    /// inherited.
    CommandLine,
    /// A command only, as it applies to the view: the order, the folds,
    /// the marks, the filter, the views and tabs, the recovery tools and
    /// shell, power, help and the redraw. A digit only shortens the way to a
    /// tab, which `Tab` and `Shift-Tab` reach.
    Command,
}

/// The views, where a view's commands apply, and a sheet, which `Ctrl-L`
/// redraws too.
const VIEWS: &[Context] = &[
    Context::Startup,
    Context::Main,
    Context::Snapshots,
    Context::Kernels,
    Context::Pools,
    Context::Recovery,
    Context::Log,
    Context::Help,
    Context::Diff,
    Context::Packages,
    Context::Choose,
];

/// Each action a character reaches in the workspace, by the views it is
/// bound in, with where rule 1 finds it ([`Reach`]). What only an arrow,
/// `Esc` or another key that is no character reaches is not listed, nor
/// what a text field's chords do, nor the command line, which `:` opens.
const ENTER_REACHES: &[(&[Context], Reach, &[Intent])] = &[
    (
        &[Context::Snapshots],
        Reach::Menu,
        &[
            Intent::Boot,
            Intent::Rollback,
            Intent::Clone,
            Intent::CloneAndPromote,
            Intent::Duplicate,
            Intent::Chroot,
        ],
    ),
    (
        &[Context::Snapshots],
        Reach::MenuAndCommand,
        &[Intent::Diff],
    ),
    (
        &[Context::Pools],
        Reach::Menu,
        &[Intent::Unlock, Intent::Import, Intent::ReadWrite],
    ),
    (&[Context::Pools], Reach::MenuAndCommand, &[Intent::Rewind]),
    (
        &[Context::Main, Context::Kernels],
        Reach::Menu,
        &[Intent::Boot],
    ),
    (
        &[Context::Main],
        Reach::CommandLine,
        &[
            Intent::Kernels,
            Intent::Snapshots,
            Intent::Edit,
            Intent::SetDefault,
            Intent::NewSnapshot,
            Intent::ReadWrite,
            Intent::Chroot,
            Intent::RevertCommandLine,
        ],
    ),
    // A new snapshot is the focused environment's.
    (
        &[Context::Snapshots],
        Reach::CommandLine,
        &[Intent::NewSnapshot],
    ),
    (
        &[Context::Kernels],
        Reach::CommandLine,
        &[Intent::Edit, Intent::SetDefault, Intent::InheritDefault],
    ),
    (
        VIEWS,
        Reach::Command,
        &[
            Intent::Sort,
            Intent::Fold,
            Intent::Mark,
            Intent::Filter,
            Intent::Pools,
            Intent::Log,
            Intent::Recovery,
            Intent::Shell,
            Intent::Power,
            Intent::Help,
            Intent::Redraw,
            Intent::Tab(Tab::Boot),
            Intent::Tab(Tab::Snapshots),
            Intent::Tab(Tab::Pools),
            Intent::Tab(Tab::Log),
        ],
    ),
];

/// Where rule 1 finds `intent` in `context` of the workspace, as
/// [`ENTER_REACHES`] says; `None` where it lists none.
pub(crate) fn reach(context: Context, intent: Intent) -> Option<Reach> {
    ENTER_REACHES
        .iter()
        .find(|(views, _, intents)| views.contains(&context) && intents.contains(&intent))
        .map(|(_, reach, _)| *reach)
}

/// The actions of `context` with the key map `map` that only a character
/// or a chord reaches, in the order of the help view, each with its keys
/// as the help view names them (`d, MOD+D`). The pager, outside the menu,
/// has no command line, so none.
pub(crate) fn commands(map: Map, context: Context) -> Vec<(Intent, String)> {
    if is_text(context) || context == Context::Pager {
        return Vec::new();
    }
    let mut keys: Vec<(Intent, Vec<String>)> = Vec::new();
    let mut elsewhere = Vec::new();
    let bound = map.table().into_iter();
    for binding in bound.filter(|binding| map.is_bound(binding, context)) {
        if matches!(binding.key, Key::Char(_)) {
            let name = key_name(binding.key, binding.modifiers);
            add(&mut keys, binding.intent, name);
        } else {
            elsewhere.push(binding.intent);
        }
    }
    // The classic aliases, as the key map reads them.
    if map == Map::Classic || context != Context::Startup {
        for (letter, meanings) in CLASSIC {
            if let Some((_, intent)) = meanings.iter().find(|(view, _)| *view == context) {
                add(&mut keys, *intent, alias_name(*letter, context));
            }
        }
    }
    keys.into_iter()
        .filter(|(intent, _)| !elsewhere.contains(intent) && !is_reached(context, *intent))
        .map(|(intent, names)| (intent, names.join(", ")))
        .collect()
}

/// Adds `name` to the keys of `intent`.
fn add(keys: &mut Vec<(Intent, Vec<String>)>, intent: Intent, name: String) {
    match keys.iter_mut().find(|(known, _)| *known == intent) {
        Some((_, names)) => names.push(name),
        None => keys.push((intent, vec![name])),
    }
}

/// Whether `intent` in `context` is reached otherwise than by a key: an
/// entry of the palette, or of an `Enter` menu alone ([`Reach::Menu`]). A
/// digit only shortens the way to a tab, which `Tab` and `Shift-Tab`
/// reach; `Ctrl-L` redraws, which does nothing yet (ADR 0016, "found and
/// left"); and the palette edits the command line of an environment
/// (`palette-edit`), as `e` does on the Boot tab.
fn is_reached(context: Context, intent: Intent) -> bool {
    match intent {
        Intent::Tab(_) | Intent::Redraw => true,
        Intent::Edit => context == Context::Main,
        _ => PALETTE_HAS.contains(&intent) || reach(context, intent) == Some(Reach::Menu),
    }
}
