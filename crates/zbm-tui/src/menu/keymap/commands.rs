//! What only a character or a chord reaches, which rule 1 of decision 18
//! (ADR 0007) puts in the command palette: the palette lists each such
//! action of the view under it with its keys, and choosing one does what
//! the key does. Not listed: what another key reaches there (`Enter`, an
//! arrow, `Tab`, `Esc`, `Delete`, a page key, `Home`, `End` or a function
//! key), what the palette names by entries of its own, and what an `Enter`
//! menu of the view reaches. A text field types the characters, and its
//! chords (`Ctrl-K`, `Ctrl-R`) have no palette there.

use zbm_display::keys::Key;

use super::classic::{CLASSIC, alias_name};
use super::{Context, Intent, Map, is_text, key_name};

/// What the palette names by entries of its own: the boot of an
/// environment, its kernels and snapshots, the views, the recovery shell,
/// power, and the palette itself.
const PALETTE_HAS: &[Intent] = &[
    Intent::Boot,
    Intent::Kernels,
    Intent::Snapshots,
    Intent::Pools,
    Intent::Log,
    Intent::Recovery,
    Intent::Power,
    Intent::Shell,
    Intent::Palette,
];

/// What an `Enter` menu of a view reaches, as its key does: a snapshot's
/// actions (`update/snapshots.rs`) and a volume's unlock, which `Enter` on
/// it does. Not a snapshot's diff, whose key diffs the marked snapshots
/// too, nor a pool's rewind, whose key rewinds a pool not imported, which
/// its details do not offer.
pub(crate) const ENTER_REACHES: &[(Context, &[Intent])] = &[
    (
        Context::Snapshots,
        &[
            Intent::Rollback,
            Intent::Clone,
            Intent::CloneAndPromote,
            Intent::Duplicate,
            Intent::Chroot,
        ],
    ),
    (Context::Pools, &[Intent::Unlock]),
];

/// The actions of `context` with the key map `map` that only a character
/// or a chord reaches, in the order of the help view, each with its keys
/// as the help view names them (`d, MOD+D`).
pub(crate) fn commands(map: Map, context: Context) -> Vec<(Intent, String)> {
    if is_text(context) {
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
/// entry of the palette, or of an `Enter` menu. A digit only shortens the
/// way to a tab, which `Tab` and `Shift-Tab` reach; `Ctrl-L` redraws,
/// which does nothing yet (ADR 0016, "found and left"); and the palette
/// edits the command line of an environment (`palette-edit`), as `e` does
/// on the Boot tab.
fn is_reached(context: Context, intent: Intent) -> bool {
    match intent {
        Intent::Tab(_) | Intent::Redraw => true,
        Intent::Edit => context == Context::Main,
        _ => {
            PALETTE_HAS.contains(&intent)
                || ENTER_REACHES
                    .iter()
                    .any(|(view, intents)| *view == context && intents.contains(&intent))
        }
    }
}
