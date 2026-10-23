use pretty_assertions::assert_eq;

use super::*;
use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, FakeMachine};

fn model() -> Model {
    let machine = FakeMachine::new(&DemoOptions::default());
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

    model.sort = Sort::Creation;
    let newest = model.focus_snapshots()[0].name.name().to_owned();
    model.sort = Sort::Size;
    let largest = model.focus_snapshots()[0].name.name().to_owned();

    assert_eq!(newest, "pre-upgrade");
    assert_eq!(largest, "pre-upgrade");
    assert_eq!(Sort::Size.next(), Sort::Name);
    assert_eq!(Sort::Creation.as_str(), "creation");
}

#[test]
fn the_pools_view_lists_pools_then_volumes() {
    let mut model = model();

    model.pools.selected = 1;
    let pool = model.selected_pool().map(ToString::to_string);
    model.pools.selected = 2;
    let volume = model.selected_volume().map(str::to_owned);

    assert_eq!(model.pool_rows().len(), 5);
    assert_eq!(pool.as_deref(), Some("tank"));
    assert_eq!(volume.as_deref(), Some("luks-zroot-a"));
}

#[test]
fn an_edited_command_line_wins_over_the_own_one() {
    let mut model = model();
    let environment = model.snapshot.environments[0].clone();

    model
        .edited
        .push((environment.name.clone(), "single".to_owned()));

    assert_eq!(model.command_line(&environment), "single");
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
