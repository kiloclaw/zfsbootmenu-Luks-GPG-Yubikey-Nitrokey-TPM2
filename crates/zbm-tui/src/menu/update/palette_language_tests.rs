//! Rule 4 of decision 18 (ADR 0007): the palette finds every entry by its
//! name in the menu's language and by its English one, whichever is typed;
//! on the demo machine, with both maps, in the tests' look-alike language.

use std::mem::Discriminant;

use pretty_assertions::assert_eq;
use zbm_display::keys::{Key, KeyPress};

use super::key_press;
use super::offers_tests::{Place, machine_all, menu, places};
use crate::menu::filter::Ranked;
use crate::menu::keymap::Map;
use crate::menu::model::{Model, Palette, PaletteItem, Then};
use crate::menu::world::{Action, Event, Snapshot};
use crate::text::{self, Language};

/// The demo machine.
type Machine = (Snapshot, Event);

/// Where the palette opens: a map, a place, and the keys pressed there
/// first.
type At = (Map, Place, Vec<KeyPress>);

/// Every place of both maps where the palette opens, one of those whose
/// entries are the same; in the workspace's snapshots, also with the
/// selected one marked, which adds an entry.
fn every_place(machine: &Machine) -> Vec<At> {
    let mut every = Vec::new();
    for map in [Map::Classic, Map::Workspace] {
        for place in places(machine, map) {
            every.push((map, place, Vec::new()));
        }
    }
    let mark = KeyPress::new(Key::Char(' '));
    every.push((Map::Workspace, Place::Snapshots, vec![mark]));
    let mut seen = Vec::new();
    every.retain(|at| {
        let Some((palette, _)) = opened(machine, at, &[], Language::English) else {
            return false;
        };
        let entries = by_label(&palette);
        let is_new = !seen.contains(&entries);
        seen.push(entries);
        is_new
    });
    every
}

/// The palette at `at`, opened in `language`, and what it matches, best
/// first, as each of `queries` is typed into it in turn, the field emptied
/// before each; none where it does not open.
fn opened(
    machine: &Machine,
    at: &At,
    queries: &[&str],
    language: Language,
) -> Option<(Palette, Vec<Vec<Ranked>>)> {
    let (map, place, before) = at;
    let press = |model: &mut Model, key: KeyPress| {
        let _ = key_press(model, key);
    };
    text::in_language(language, || {
        let mut model = menu(machine, *map, *place);
        for key in before {
            press(&mut model, *key);
        }
        press(&mut model, KeyPress::new(Key::Char(':')));
        let palette = model.palette.clone()?;
        let mut found = Vec::new();
        for query in queries {
            press(&mut model, KeyPress::new(Key::Char('u')).with_ctrl());
            for c in query.chars() {
                press(&mut model, KeyPress::new(Key::Char(c)));
            }
            found.push(model.palette.as_ref()?.matches.clone());
        }
        Some((palette, found))
    })
}

/// The palette at `at`, opened in `language`, and what each of `queries`
/// matches in it.
fn palette(
    machine: &Machine,
    at: &At,
    queries: &[&str],
    language: Language,
) -> (Palette, Vec<Vec<Ranked>>) {
    opened(machine, at, queries, language).expect("the palette opens")
}

/// The labels of `palette`'s entries.
fn labels(palette: &Palette) -> Vec<&str> {
    palette
        .items
        .iter()
        .map(|item| item.label.as_str())
        .collect()
}

/// The entries `matches` names, best first, as indexes.
fn indexes(matches: &[Ranked]) -> Vec<usize> {
    matches.iter().map(|ranked| ranked.index).collect()
}

/// Each entry's label and kind.
fn by_label(palette: &Palette) -> Vec<(Option<String>, Discriminant<Then>)> {
    named(palette, |item| Some(item.label.clone()))
}

/// Each entry's English name and kind.
fn by_english_name(palette: &Palette) -> Vec<(Option<String>, Discriminant<Then>)> {
    named(palette, |item| item.english.clone())
}

/// Each entry's name, as `name` gives it, and kind.
fn named(
    palette: &Palette,
    name: impl Fn(&PaletteItem) -> Option<String>,
) -> Vec<(Option<String>, Discriminant<Then>)> {
    let kind = |item: &PaletteItem| std::mem::discriminant(&item.then);
    palette.items.iter().map(|i| (name(i), kind(i))).collect()
}

#[test]
fn the_palette_finds_every_entry_by_its_english_name_in_another_language() {
    let machine = machine_all();
    let mut checked = 0;
    for at in every_place(&machine) {
        let where_ = format!("{:?} {:?} {:?}", at.0, at.1, at.2);
        let (english, _) = palette(&machine, &at, &[], Language::English);
        let queries = labels(&english);
        let (_, in_english) = palette(&machine, &at, &queries, Language::English);
        let (other, by_english) = palette(&machine, &at, &queries, Language::Lookalike);

        // Each entry has the English label of the same entry in English:
        // the same kind, in the same place.
        assert_eq!(by_english_name(&other), by_label(&english), "{where_}");
        for (index, query) in queries.iter().enumerate() {
            let at = format!("{where_} {query}");
            assert!(!labels(&other).contains(query), "{at}");
            assert!(indexes(&by_english[index]).contains(&index), "{at}");
            assert_eq!(
                indexes(&by_english[index]),
                indexes(&in_english[index]),
                "{at}"
            );
            checked += 1;
        }
    }
    assert!(checked > 300, "{checked}");
}

#[test]
fn the_palette_finds_every_entry_by_its_own_name() {
    let machine = machine_all();
    for at in every_place(&machine) {
        let (other, _) = palette(&machine, &at, &[], Language::Lookalike);
        let queries = labels(&other);
        let (_, by_own_name) = palette(&machine, &at, &queries, Language::Lookalike);
        for (index, query) in queries.iter().enumerate() {
            let ranked = by_own_name[index].iter().find(|r| r.index == index);

            let at = format!("{:?} {:?} {query}", at.0, at.1);
            assert!(ranked.is_some_and(|r| !r.positions.is_empty()), "{at}");
        }
    }
}

#[test]
fn an_english_match_highlights_nothing_of_the_shown_label() {
    let machine = machine_all();
    let at = (Map::Workspace, Place::Main, Vec::new());
    let (own, _) = palette(&machine, &at, &[], Language::Lookalike);
    let index = own
        .items
        .iter()
        .position(|item| item.then == Then::Act(Action::PowerOff))
        .unwrap();
    let own_name = own.items[index].label.as_str();
    let positions = |queries: &[&str], language| {
        let (_, found) = palette(&machine, &at, queries, language);
        found[0]
            .iter()
            .find(|ranked| ranked.index == index)
            .map(|ranked| ranked.positions.clone())
    };
    let all = vec![0, 1, 2, 3, 4, 6, 7, 8];

    assert_eq!(own_name, "Роԝег Оғғ");
    assert_eq!(
        positions(&["Power Off"], Language::English),
        Some(all.clone())
    );
    assert_eq!(
        positions(&["Power Off"], Language::Lookalike),
        Some(Vec::new())
    );
    assert_eq!(positions(&[own_name], Language::Lookalike), Some(all));
}

#[test]
fn english_entries_carry_no_second_name() {
    let machine = machine_all();
    for at in every_place(&machine) {
        let (english, _) = palette(&machine, &at, &[], Language::English);

        assert!(!english.items.is_empty());
        assert!(english.items.iter().all(|item| item.english.is_none()));
    }
}
