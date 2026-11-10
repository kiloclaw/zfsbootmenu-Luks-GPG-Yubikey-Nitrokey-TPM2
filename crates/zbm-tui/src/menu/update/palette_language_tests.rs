//! Rule 4 of decision 18 (ADR 0007): the palette finds every entry by its
//! name in the menu's language and by its English one, whichever is typed;
//! on the demo machine, with both maps, in the tests' look-alike language.

use std::mem::Discriminant;

use nucleo_matcher::pattern::CaseMatching;

use pretty_assertions::assert_eq;
use zbm_display::keys::{Key, KeyPress};

use super::key_press;
use super::offers_tests::{Place, machine_all, menu, places};
use crate::menu::filter::{Ranked, rank_named};
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

/// The palette at `at`, opened in `language`, and what it matches, as each
/// of `queries` is typed into it in turn, the field emptied before each;
/// none where it does not open.
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
            // All but its last character set at once, which the last key
            // finds with: what it finds does not depend on the keys before.
            let mut chars = query.chars();
            let last = chars.next_back();
            model.palette.as_mut()?.query = chars.as_str().to_owned();
            if let Some(c) = last {
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

/// Each entry's English name and kind: in another language, the last of
/// the names it is also known by, after its command's in the menu's
/// language and in English.
fn by_english_name(palette: &Palette) -> Vec<(Option<String>, Discriminant<Then>)> {
    named(palette, |item| item.also.lines().nth(2).map(str::to_owned))
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
fn english_entries_are_also_known_by_their_command_alone() {
    let machine = machine_all();
    for at in every_place(&machine) {
        let (english, _) = palette(&machine, &at, &[], Language::English);

        assert!(!english.items.is_empty());
        assert!(english.items.iter().all(|item| !item.also.contains('\n')));
    }
}

#[test]
fn every_command_is_found_by_its_name_in_the_menu_s_language_and_in_english() {
    let machine = machine_all();
    let mut commands = Vec::new();
    let mut checked = 0;
    for at in every_place(&machine) {
        let where_ = format!("{:?} {:?} {:?}", at.0, at.1, at.2);
        let (other, _) = palette(&machine, &at, &[], Language::Lookalike);
        let names = || (other.items.iter()).map(|i| (i.label.as_str(), Some(i.also.as_str())));
        // Its command and target in the menu's language, then in English,
        // as the command line ranks a query.
        for item in &other.items {
            for query in item.also.lines().take(2) {
                let found = rank_named(names(), query, CaseMatching::Ignore);
                let is_it = |ranked: &Ranked| other.items[ranked.index] == *item;
                assert!(found.iter().any(is_it), "{where_} {query}");
                checked += 1;
            }
        }
        let (english, _) = palette(&machine, &at, &[], Language::English);
        commands.extend(english.items.into_iter().map(|item| item.also));
    }
    assert!(checked > 600, "{checked}");
    // A command line edited, and a pool imported read/write.
    let mut model = menu(&machine, Map::Workspace, Place::Main);
    model
        .edited
        .push((void(), zbm_types::Untrusted::new("quiet".to_owned())));
    if let Some(pool) = model.snapshot.pools.iter_mut().find(|pool| pool.imported) {
        pool.writable = true;
    }
    let items = super::palette_items(&model);
    commands.extend(items.into_iter().map(|item| item.also));
    // Each name of the catalogue is an entry's somewhere.
    for (name, _) in crate::menu::keymap::COMMAND_NAMES {
        let name = text::get(*name);
        let is_its = |also: &String| also == &name || also.starts_with(&format!("{name} "));
        assert!(commands.iter().any(is_its), "{name}");
    }
}

#[test]
fn a_name_match_highlights_nothing_of_the_label() {
    let machine = machine_all();
    let at = (Map::Workspace, Place::Main, Vec::new());
    let (english, _) = palette(&machine, &at, &[], Language::English);
    let index = (english.items.iter())
        .position(|item| item.then == Then::Act(Action::SetDefaultEnvironment(void())))
        .unwrap();
    let name = english.items[index].also.clone();
    let label = english.items[index].label.clone();
    let positions = |query: &str| {
        let (_, found) = palette(&machine, &at, &[query], Language::English);
        found[0]
            .iter()
            .find(|ranked| ranked.index == index)
            .map(|ranked| ranked.positions.clone())
    };

    assert_eq!(name, "default zroot/ROOT/void");
    assert_eq!(positions(&name), Some(Vec::new()));
    assert!(positions(&label).is_some_and(|found| !found.is_empty()));
}

/// The demo's default environment.
fn void() -> zbm_types::DatasetName {
    "zroot/ROOT/void".parse().unwrap()
}

#[test]
fn headings_are_not_matched() {
    let machine = machine_all();
    let at = (Map::Workspace, Place::Main, Vec::new());
    // A heading is no entry: the empty query finds each entry once, and
    // nothing more, whatever headings are drawn between them.
    let (empty, _) = palette(&machine, &at, &[], Language::English);
    let mut every = indexes(&empty.matches);
    every.sort_unstable();
    assert_eq!(every, (0..empty.items.len()).collect::<Vec<_>>());
    let headings = (1..empty.matches.len()).filter(|at| empty.heading_at(*at).is_some());
    assert!(headings.count() >= 2, "{empty:?}");
    for id in [
        crate::text::id!("section-recent"),
        crate::text::id!("section-actions"),
        crate::text::id!("section-items"),
        crate::text::id!("section-help"),
    ] {
        let heading = text::get(id);
        let (palette, found) = palette(&machine, &at, &[&heading], Language::English);
        assert!(palette.items.iter().all(|item| item.label != heading));
        // Only the entries whose own names match it.
        let names = (palette.items.iter()).map(|i| (i.label.as_str(), Some(i.also.as_str())));
        let own = rank_named(names, &heading, CaseMatching::Ignore);
        let mut found = indexes(&found[0]);
        let mut own = indexes(&own);
        found.sort_unstable();
        own.sort_unstable();
        assert_eq!(found, own, "{heading}");
        assert!(found.iter().all(|index| *index < palette.items.len()));
    }
}
