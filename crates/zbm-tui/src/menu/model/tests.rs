use pretty_assertions::assert_eq;

use super::*;
use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, FakeMachine};

/// The menu on the demo machine, the snapshots of the default
/// environment read.
fn model() -> Model {
    let mut machine = FakeMachine::new(&DemoOptions::default());
    machine.apply(Action::LoadSnapshots("zroot/ROOT/void".parse().unwrap()));
    Model::new(machine.snapshot().clone(), Theme::new(Capabilities::FULL))
}

#[test]
fn defaults_come_first_then_names() {
    let model = model();

    let order: Vec<&str> = model
        .environments
        .matches
        .iter()
        .map(|r| model.snapshot.environments[r.index].name.as_str())
        .collect();
    assert_eq!(
        order,
        [
            "zroot/ROOT/void",
            "tank/ROOT/arch",
            "tank/ROOT/debian",
            "zroot/ROOT/ubuntu"
        ]
    );
}

#[test]
fn snapshots_sort_by_name_creation_or_size() {
    let mut model = model();
    model.focus = Some("zroot/ROOT/void".parse().unwrap());

    model.set_sort(Sort::Creation);
    let newest = model.focus_snapshots()[0].name.name().to_owned();
    model.set_sort(Sort::Size);
    let largest = model.focus_snapshots()[0].name.name().to_owned();

    assert_eq!(newest, "pre-upgrade");
    assert_eq!(largest, "pre-upgrade");
    assert_eq!(Sort::Size.next(), Sort::Name);
    assert_eq!(crate::text::get(Sort::Creation.id()), "creation");
}

#[test]
fn a_new_sort_order_keeps_the_snapshot_selected() {
    let mut model = model();
    model.focus = Some("zroot/ROOT/void".parse().unwrap());
    model.refresh();
    model.snapshots.selected = 3;
    let chosen = model.selected_snapshot().unwrap().name.clone();

    for _ in 0..3 {
        model.set_sort(model.sort.next());
        assert_eq!(
            model.selected_snapshot().map(|s| &s.name),
            Some(&chosen),
            "sorted by {:?}",
            model.sort
        );
    }
}

#[test]
fn the_pools_view_lists_pools_then_volumes() {
    let mut model = model();

    model.pools.selected = 1;
    let pool = model.selected_pool().map(|pool| pool.name.to_string());
    model.pools.selected = 2;
    let volume = model
        .selected_volume()
        .map(|volume| volume.name.get().clone());

    assert_eq!(model.pool_rows().len(), 5);
    assert_eq!(pool.as_deref(), Some("tank"));
    assert_eq!(volume.as_deref(), Some("luks-zroot-a"));
}

#[test]
fn an_edited_command_line_wins_over_the_own_one() {
    let mut model = model();
    let environment = model.snapshot.environments[0].clone();

    model.edited.push((
        environment.name.clone(),
        Untrusted::new("single".to_owned()),
    ));

    assert_eq!(model.command_line(&environment).get(), "single");
}

#[test]
fn the_context_follows_the_focus() {
    let mut model = model();
    assert_eq!(model.context(), Context::Startup);

    model.screen = Screen::Main;
    model.environments.filtering = true;
    assert_eq!(model.context(), Context::Text);
    model.environments.filtering = false;
    model.dialog = Some(Dialog::Error {
        what: String::new(),
        why: Vec::new(),
        next: Vec::new(),
    });
    assert_eq!(model.context(), Context::Choose);
}

#[test]
fn snapshots_are_ranked_again_only_when_their_list_changed() {
    let mut model = model();
    model.focus = Some("zroot/ROOT/void".parse().unwrap());
    model.snapshots.filter = "pre".to_owned();
    model.refresh();
    let ranked = model.ranked.clone();
    assert_eq!(model.snapshots.matches.len(), 1);

    // The same lists, sent again with the machine: nothing to rank.
    model.replace_snapshot(model.snapshot.clone());
    assert_eq!(model.ranked, ranked);
    // Another list, or another filter, is ranked.
    let mut changed = model.snapshot.clone();
    changed.loaded[0].snapshots.truncate(10);
    model.replace_snapshot(changed);
    assert_ne!(model.ranked, ranked);
    model.snapshots.filter.clear();
    model.refresh();
    assert_eq!(model.snapshots.matches.len(), 10);
}
