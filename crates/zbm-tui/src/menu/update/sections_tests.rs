//! The command line's sections (ADR 0007, point 5): what it finds comes
//! grouped as the entries chosen before, actions, items and help, best
//! first in each, under headings no key selects; the selection stays on its
//! entry as the machine sends what it finds again; and an item goes to its
//! place. On the demo machine, under the workspace.

use nucleo_matcher::pattern::CaseMatching;
use pretty_assertions::assert_eq;
use zbm_display::keys::{Key, KeyPress};

use super::key_press;
use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, Session};
use crate::menu::filter::rank_named;
use crate::menu::model::{Model, Palette, Place, Screen, Section, Then};
use crate::menu::theme::Theme;
use crate::menu::update::{Message, update};
use crate::menu::world::Event;
use crate::text::text;

/// The demo under the workspace, started, its countdown stopped.
fn menu() -> Model {
    let mut session = Session::new(&DemoOptions::default(), Theme::new(Capabilities::FULL));
    session.start();
    session.key(Key::Escape);
    session.into_model()
}

/// Presses `key` in `model`.
fn press(model: &mut Model, key: Key) {
    let _ = key_press(model, KeyPress::new(key));
}

/// Opens the command line of `model` and types `query`.
fn open(model: &mut Model, query: &str) {
    press(model, Key::Char(':'));
    for c in query.chars() {
        press(model, Key::Char(c));
    }
}

/// The open palette.
fn palette(model: &Model) -> &Palette {
    model.palette.as_ref().expect("the command line is open")
}

/// The section of each match, in order.
fn sections(palette: &Palette) -> Vec<Section> {
    (0..palette.matches.len())
        .filter_map(|at| palette.section_at(at))
        .collect()
}

/// The label of the selected entry.
fn selected(model: &Model) -> String {
    let palette = palette(model);
    let index = palette.matches[palette.selected].index;
    palette.items[index].label.clone()
}

#[test]
fn matches_come_grouped_recent_actions_items_help_best_first_in_each() {
    let mut model = menu();
    open(&mut model, "log");
    press(&mut model, Key::Enter);
    press(&mut model, Key::Char('1'));

    // With the query empty, the entry chosen before first, under Recent.
    open(&mut model, "");
    let found = palette(&model);
    assert_eq!(found.chosen, 1);
    assert_eq!(found.items[found.matches[0].index].label, text!("tool-log"));
    let every = sections(found);
    assert!(every.is_sorted(), "{every:?}");
    for section in [
        Section::Recent,
        Section::Actions,
        Section::Items,
        Section::Help,
    ] {
        assert!(every.contains(&section), "{section:?}");
    }
    assert_eq!(found.matches.len(), found.items.len());

    // With a query, none: each section's entries in the order the query
    // ranks them, best first.
    for query in ["o", "void", "pools"] {
        press(&mut model, Key::Escape);
        open(&mut model, query);
        let found = palette(&model);
        let names = found
            .items
            .iter()
            .map(|i| (i.label.as_str(), Some(i.also.as_str())));
        let ranked = rank_named(names, query, CaseMatching::Ignore);
        let every = sections(found);
        assert_eq!(found.chosen, 0);
        assert!(
            every.is_sorted() && !every.contains(&Section::Recent),
            "{query}"
        );
        for section in [Section::Actions, Section::Items, Section::Help] {
            let of = |index: &usize| found.items[*index].section == section;
            let best: Vec<usize> = ranked.iter().map(|r| r.index).filter(of).collect();
            let shown: Vec<usize> = found.matches.iter().map(|r| r.index).filter(of).collect();
            assert_eq!(shown, best, "{query} {section:?}");
        }
    }
}

#[test]
fn up_and_down_pass_over_the_headings() {
    let mut model = menu();
    open(&mut model, "pools");
    let found = palette(&model);
    let headings: Vec<Option<Section>> = (0..3).map(|at| found.heading_at(at)).collect();
    let expected = [Section::Actions, Section::Items, Section::Help];
    assert_eq!(headings, expected.map(Some));
    let entries = [
        text!("palette-rescan-pools"),
        text!("tool-pools"),
        text!("palette-help", view = text!("view-pools")),
    ];

    for entry in &entries {
        assert_eq!(&selected(&model), entry);
        press(&mut model, Key::Down);
    }
    assert_eq!(selected(&model), entries[2]);
    for entry in entries.iter().rev() {
        assert_eq!(&selected(&model), entry);
        press(&mut model, Key::Up);
    }
    assert_eq!(palette(&model).selected, 0);
    // The line view says the headings, and numbers the entries alone.
    let page = crate::menu::lines::page(&model);
    let at = |line: &str| page.iter().position(|said| said == line);
    let actions = at(&format!("  {}", text!("section-actions"))).expect("a heading");
    assert_eq!(page[actions + 1], format!("  1) {}", entries[0]));
    let help = at(&format!("  {}", text!("section-help"))).expect("a heading");
    assert_eq!(page[help + 1], format!("  3) {}", entries[2]));
}

#[test]
fn the_selection_stays_on_its_entry_when_the_machine_sends_a_snapshot() {
    let mut model = menu();
    open(&mut model, "");
    let boot_of = |environment: &str| text!("palette-boot", environment = environment);
    let target = boot_of("zroot/ROOT/ubuntu");
    while selected(&model) != target {
        press(&mut model, Key::Down);
    }
    let before = palette(&model).selected;

    // The first environment is gone: the entry moved up, and stays
    // selected.
    let mut snapshot = model.snapshot.clone();
    snapshot.environments.remove(0);
    let message = Message::Event(Box::new(Event::Changed(Box::new(snapshot))));
    let (mut model, _) = update(model, message);

    assert_eq!(selected(&model), target);
    assert!(palette(&model).selected < before);
    // An entry gone leaves the selection where it was, in range.
    let mut snapshot = model.snapshot.clone();
    snapshot
        .environments
        .retain(|e| e.name.to_string() != "zroot/ROOT/ubuntu");
    let at = palette(&model).selected;
    let message = Message::Event(Box::new(Event::Changed(Box::new(snapshot))));
    (model, _) = update(model, message);
    let palette = palette(&model);
    assert_eq!(palette.selected, at.min(palette.matches.len() - 1));
    assert_ne!(selected(&model), target);
}

#[test]
fn going_to_a_pool_selects_it_on_the_pools_tab() {
    let mut model = menu();
    open(&mut model, &text!("palette-go-pool", pool = "tank"));
    press(&mut model, Key::Enter);

    assert_eq!(model.screen, Screen::Pools);
    assert_eq!(
        model.selected_pool().map(|p| p.name.to_string()),
        Some("tank".to_owned())
    );
    // A volume, on the same tab; an environment, on the Boot tab, though
    // its list was filtered.
    let volume = model.snapshot.volumes[1].name.clone();
    let then = Then::Go(Place::Volume(volume.clone()));
    let _ = super::follow(&mut model, then);
    assert_eq!(
        model.selected_volume().map(|v| v.name.clone()),
        Some(volume)
    );
    model.environments.filter = "arch".to_owned();
    let environment: zbm_types::DatasetName = "zroot/ROOT/ubuntu".parse().unwrap();
    let _ = super::follow(
        &mut model,
        Then::Go(Place::Environment(environment.clone())),
    );
    assert_eq!(model.screen, Screen::Main);
    assert_eq!(model.environments.filter, "");
    let selected = model.selected_environment().map(|e| e.name.clone());
    assert_eq!(selected, Some(environment));
}
