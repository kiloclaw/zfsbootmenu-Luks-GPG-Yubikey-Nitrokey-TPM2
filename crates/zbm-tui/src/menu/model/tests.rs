use pretty_assertions::assert_eq;

use super::*;
use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, FakeMachine};

/// The classic menu (`zbm.menu=classic`) on the demo machine, the
/// snapshots of the default environment read.
fn model() -> Model {
    let options = DemoOptions {
        classic: true,
        ..DemoOptions::default()
    };
    let mut machine = FakeMachine::new(&options);
    machine.apply(Action::LoadSnapshots("zroot/ROOT/void".parse().unwrap()));
    let mut model = Model::new(machine.snapshot().clone(), Theme::new(Capabilities::FULL));
    model.map = Map::Classic;
    model.refresh();
    model
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

#[test]
fn the_workspace_groups_the_environments_by_pool_under_unnumbered_headers() {
    let mut model = model();
    model.map = Map::Workspace;
    model.refresh();
    let order: Vec<&str> = model
        .environments
        .matches
        .iter()
        .map(|r| model.snapshot.environments[r.index].name.as_str())
        .collect();
    let headers: Vec<Option<String>> = (0..5)
        .map(|position| model.group_at(position).map(|pool| pool.to_string()))
        .collect();

    // The default's pool first, and the default first in it.
    assert_eq!(
        order,
        [
            "zroot/ROOT/void",
            "zroot/ROOT/ubuntu",
            "tank/ROOT/arch",
            "tank/ROOT/debian"
        ]
    );
    let zroot = Some("zroot".to_owned());
    let tank = Some("tank".to_owned());
    assert_eq!(headers, [zroot, None, tank, None, None]);
    // A filter ranks by match, and the classic map has no groups.
    model.environments.filter = "o".to_owned();
    model.refresh();
    assert_eq!(model.group_at(0), None);
    model.environments.filter.clear();
    model.map = Map::Classic;
    model.refresh();
    assert_eq!(model.group_at(0), None);
}

#[test]
fn the_path_names_the_pool_the_environment_and_the_kernel_open() {
    let mut model = model();
    model.screen = Screen::Main;
    assert_eq!(model.path(), ["zroot", "zroot/ROOT/void"]);

    model.focus = Some("tank/ROOT/arch".parse().unwrap());
    model.screen = Screen::Kernels;
    model.refresh();
    model.kernels.selected = 1;
    assert_eq!(model.path(), ["tank", "tank/ROOT/arch", "6.10.10-arch1-1"]);

    model.screen = Screen::Log;
    assert!(model.path().is_empty());
}
