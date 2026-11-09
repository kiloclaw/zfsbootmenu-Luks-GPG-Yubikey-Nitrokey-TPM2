//! The command palette: what it finds, its query and its selection.

mod entries;

use self::entries::entries;
use super::dialogs;
use super::text::edit;
use crate::menu::filter::Ranked;
use crate::menu::keymap::{Input, Intent};
use crate::menu::model::{Model, Palette, PaletteItem, Then};
use crate::menu::world::Action;
use crate::text::{self, Language};

/// Opens the command palette, with what it finds now.
pub(super) fn open_palette(model: &mut Model) {
    let items = palette_items(model);
    let matches = rank(&items, "");
    model.palette = Some(Palette {
        query: String::new(),
        items,
        matches,
        selected: 0,
    });
}

/// Makes what the open palette finds again, as the machine sent a
/// snapshot, its pools or snapshots, keeping the query; the selection
/// stays in range.
pub(super) fn palette_again(model: &mut Model) {
    if model.palette.is_none() {
        return;
    }
    let items = palette_items(model);
    if let Some(palette) = &mut model.palette {
        palette.matches = rank(&items, &palette.query);
        palette.items = items;
        palette.selected = palette
            .selected
            .min(palette.matches.len().saturating_sub(1));
    }
}

/// The palette's `items` that `query` matches, by their labels or their
/// English ones.
fn rank(items: &[PaletteItem], query: &str) -> Vec<Ranked> {
    let names = items
        .iter()
        .map(|item| (item.label.as_str(), item.english.as_deref()));
    crate::menu::filter::rank_named(names, query)
}

/// The command palette.
pub(super) fn palette(model: &mut Model, input: Input) -> Vec<Action> {
    let Some(mut palette) = model.palette.take() else {
        return Vec::new();
    };
    if edit(&mut palette.query, input).is_none() {
        palette.matches = rank(&palette.items, &palette.query);
        palette.selected = 0;
        model.palette = Some(palette);
        return Vec::new();
    }
    let last = palette.matches.len().saturating_sub(1);
    match input {
        Input::Intent(Intent::Back) => return Vec::new(),
        Input::Intent(Intent::Primary) => {
            let chosen = palette
                .matches
                .get(palette.selected)
                .and_then(|r| palette.items.get(r.index));
            // A change chosen here shows its plan first. A boot from here is
            // checked as from the lists: it may not boot.
            super::dry_run::start(model);
            return match chosen.map(|item| item.then.clone()) {
                Some(Then::Act(boot @ Action::Boot { .. })) => super::screens::guarded(model, boot),
                Some(then) => dialogs::follow(model, then),
                None => Vec::new(),
            };
        }
        Input::Intent(Intent::Up) => palette.selected = palette.selected.saturating_sub(1),
        Input::Intent(Intent::Down) => palette.selected = (palette.selected + 1).min(last),
        _ => {}
    }
    model.palette = Some(palette);
    Vec::new()
}

/// Everything the command palette finds: environments, their kernels, the
/// snapshots read so far, the actions of every view the machine offers,
/// and those of the view under it that only a key reaches. In a language
/// other than English, each also has its English label, which the palette
/// matches too (rule 4 of decision 18); in English, they are built once.
pub fn palette_items(model: &Model) -> Vec<PaletteItem> {
    let mut items = entries(model);
    if text::language() == Language::English {
        return items;
    }
    // The same model gives the same entries in the same order, whatever
    // the language: each takes the English label of its twin. Were they to
    // differ, none would have one.
    let english = text::in_language(Language::English, || entries(model));
    let kind = std::mem::discriminant::<Then>;
    let is_paired = items.len() == english.len()
        && items
            .iter()
            .zip(&english)
            .all(|(item, twin)| kind(&item.then) == kind(&twin.then));
    debug_assert!(is_paired);
    if is_paired {
        for (item, twin) in items.iter_mut().zip(english) {
            item.english = Some(twin.label);
        }
    }
    items
}
