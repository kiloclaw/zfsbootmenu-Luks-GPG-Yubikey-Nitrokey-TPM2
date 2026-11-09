//! The command palette: what it finds, its query and its selection.

mod entries;

use nucleo_matcher::pattern::CaseMatching;

use self::entries::entries;
use super::dialogs;
use super::text::edit;
use crate::menu::filter::Ranked;
use crate::menu::keymap::{Input, Intent};
use crate::menu::model::{Model, Palette, PaletteItem, Recent, Then};
use crate::menu::world::Action;
use crate::text::{self, Language};

/// Opens the command palette, with what it finds now: with its query
/// empty, the entries chosen before first.
pub(super) fn open_palette(model: &mut Model) {
    let items = palette_items(model);
    let matches = rank(&items, "", &model.recent, false);
    model.palette = Some(Palette {
        query: String::new(),
        items,
        matches,
        selected: 0,
        recent: false,
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
        palette.matches = rank(&items, &palette.query, &model.recent, palette.recent);
        palette.items = items;
        palette.selected = palette
            .selected
            .min(palette.matches.len().saturating_sub(1));
    }
}

/// The palette's `items` that `query` matches, by their labels or their
/// English ones, whatever their case: Caps Lock changes nothing found.
/// With `only_recent`, the entries chosen before alone, newest first among
/// equals; with the query empty, those first, then the others.
fn rank(items: &[PaletteItem], query: &str, recent: &Recent, only_recent: bool) -> Vec<Ranked> {
    let chosen = if only_recent || query.is_empty() {
        recent.found_in(items)
    } else {
        Vec::new()
    };
    if !only_recent && chosen.is_empty() {
        let every: Vec<usize> = (0..items.len()).collect();
        return rank_among(items, &every, query);
    }
    let mut order = chosen;
    if !only_recent {
        let others = (0..items.len()).filter(|index| !order.contains(index));
        order.extend(others.collect::<Vec<_>>());
    }
    rank_among(items, &order, query)
}

/// The entries of `items` at `order` that `query` matches, as indexes of
/// `items`; equal scores keep `order`.
fn rank_among(items: &[PaletteItem], order: &[usize], query: &str) -> Vec<Ranked> {
    let names = order.iter().filter_map(|index| {
        let item = items.get(*index)?;
        Some((item.label.as_str(), item.english.as_deref()))
    });
    let ranked = crate::menu::filter::rank_named(names, query, CaseMatching::Ignore);
    ranked
        .into_iter()
        .filter_map(|ranked| {
            let index = *order.get(ranked.index)?;
            Some(Ranked { index, ..ranked })
        })
        .collect()
}

/// The command palette.
pub(super) fn palette(model: &mut Model, input: Input) -> Vec<Action> {
    let Some(mut palette) = model.palette.take() else {
        return Vec::new();
    };
    if edit(&mut palette.query, input).is_none() {
        find_again(&mut palette, &model.recent);
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
            let then = chosen.map(|item| item.then.clone());
            if let Some(then) = &then {
                model.recent.chose(then);
            }
            // A change chosen here shows its plan first. A boot from here is
            // checked as from the lists: it may not boot.
            super::dry_run::start(model);
            return match then {
                Some(Then::Act(boot @ Action::Boot { .. })) => super::screens::guarded(model, boot),
                Some(then) => dialogs::follow(model, then),
                None => Vec::new(),
            };
        }
        Input::Intent(Intent::Up) => palette.selected = palette.selected.saturating_sub(1),
        Input::Intent(Intent::Down) => palette.selected = (palette.selected + 1).min(last),
        Input::Intent(Intent::Complete) => complete(&mut palette, &model.recent),
        Input::Intent(Intent::DeleteWord) => {
            delete_word(&mut palette.query);
            find_again(&mut palette, &model.recent);
        }
        Input::Intent(Intent::History) => {
            palette.recent = !palette.recent;
            find_again(&mut palette, &model.recent);
        }
        _ => {}
    }
    model.palette = Some(palette);
    Vec::new()
}

/// What the palette finds for its query, from the top.
fn find_again(palette: &mut Palette, recent: &Recent) {
    palette.matches = rank(&palette.items, &palette.query, recent, palette.recent);
    palette.selected = 0;
}

/// `Tab`: the query becomes the selected entry's label, and the entry
/// stays selected.
fn complete(palette: &mut Palette, recent: &Recent) {
    let Some(index) = palette.matches.get(palette.selected).map(|r| r.index) else {
        return;
    };
    let Some(item) = palette.items.get(index) else {
        return;
    };
    palette.query.clone_from(&item.label);
    find_again(palette, recent);
    let at = palette.matches.iter().position(|r| r.index == index);
    palette.selected = at.unwrap_or(0);
}

/// `Ctrl-W`: the query without its last word, and the spaces after it;
/// those before it stay, as a shell's line keeps them.
fn delete_word(query: &mut String) {
    let kept = query
        .trim_end()
        .trim_end_matches(|c: char| !c.is_whitespace())
        .len();
    query.truncate(kept);
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
