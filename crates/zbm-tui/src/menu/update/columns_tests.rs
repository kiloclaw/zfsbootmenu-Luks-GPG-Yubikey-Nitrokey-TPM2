//! Tests of the columns of the workspace's Boot tab (ADR 0007, decision 2):
//! the environments grouped by pool, the arrows into and out of the
//! kernels, and the selection each column keeps.

use pretty_assertions::assert_eq;
use zbm_display::keys::Key;

use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, Session};
use crate::menu::keymap::Map;
use crate::menu::model::{Model, Screen, Segment};
use crate::menu::theme::Theme;
use crate::menu::world::{Environment, Snapshot};

/// The demo in the workspace, started, its countdown stopped.
fn workspace() -> Session {
    let options = DemoOptions {
        classic: false,
        ..DemoOptions::default()
    };
    let mut session = Session::new(&options, Theme::new(Capabilities::FULL));
    session.start();
    session.key(Key::Escape);
    session
}

/// The name of the environment the Boot tab selects.
fn selected(model: &Model) -> String {
    model
        .selected_environment()
        .map(|environment| environment.name.to_string())
        .unwrap_or_default()
}

/// The version of the kernel the kernels column selects.
fn kernel(model: &Model) -> String {
    model
        .selected_kernel()
        .map(|kernel| kernel.version.get().clone())
        .unwrap_or_default()
}

#[test]
fn up_and_down_go_from_environment_to_environment_over_the_pool_headers() {
    let mut session = workspace();

    let mut seen = vec![selected(session.model())];
    for _ in 0..3 {
        session.key(Key::Down);
        seen.push(selected(session.model()));
    }
    session.key(Key::Up);

    assert_eq!(
        seen,
        [
            "zroot/ROOT/void",
            "zroot/ROOT/ubuntu",
            "tank/ROOT/arch",
            "tank/ROOT/debian"
        ]
    );
    assert_eq!(selected(session.model()), "tank/ROOT/arch");
}

/// An environment like the demo's `zroot/ROOT/void`, called `name`.
fn environment(model: &Model, name: &str, is_default: bool) -> Environment {
    Environment {
        name: name.parse().unwrap(),
        is_default,
        ..model.snapshot.environments[0].clone()
    }
}

#[test]
fn up_from_the_default_reaches_the_environment_above_it() {
    // As integrity-locked boots it: two pools, each with a default, and
    // `zbm.prefer` selecting the second.
    let session = workspace();
    let model = session.model();
    let snapshot = Snapshot {
        environments: vec![
            environment(model, "tplain/ROOT/old", false),
            environment(model, "tplain/ROOT/be", true),
            environment(model, "tauto/ROOT/be", true),
        ],
        ..model.snapshot.clone()
    };
    let mut model = session.into_model();
    model.replace_snapshot(snapshot);
    model.select_environment(&"tplain/ROOT/be".parse().unwrap());
    let order: Vec<String> = (0..3)
        .map(|position| {
            let index = model.environments.matches[position].index;
            model.snapshot.environments[index].name.to_string()
        })
        .collect();

    let (model, _) = super::update(model, key(Key::Up));

    assert_eq!(
        order,
        ["tauto/ROOT/be", "tplain/ROOT/be", "tplain/ROOT/old"]
    );
    assert_eq!(selected(&model), "tauto/ROOT/be");
}

fn key(key: Key) -> super::Message {
    super::Message::Key(zbm_display::keys::KeyPress::new(key))
}

#[test]
fn right_goes_into_the_kernels_and_left_out_and_the_column_keeps_its_selection() {
    let mut session = workspace();

    session.key(Key::Right);
    assert_eq!(session.model().screen, Screen::Kernels);
    assert_eq!(kernel(session.model()), "6.12.9_1");
    session.key(Key::Down);
    session.key(Key::Left);
    assert_eq!(session.model().screen, Screen::Main);
    session.key(Key::Right);
    assert_eq!(kernel(session.model()), "6.12.8_1");
    // `h` and `l` too.
    session.key(Key::Char('h'));
    session.key(Key::Char('l'));
    assert_eq!(kernel(session.model()), "6.12.8_1");

    // Another environment's kernels open on the one that boots.
    session.key(Key::Left);
    session.key(Key::Down);
    session.key(Key::Right);
    assert_eq!(kernel(session.model()), "6.8.0-47-generic");
    // Right in the kernels goes nowhere.
    session.key(Key::Right);
    assert_eq!(session.model().screen, Screen::Kernels);
    assert!(session.model().notification.is_none());
}

#[test]
fn the_kernels_open_on_the_default_after_another_view_focused_the_environment() {
    // Snapshots, the command line, then the kernels of `zroot/ROOT/ubuntu`,
    // whose default kernel is its second.
    for detour in [
        [Key::Char('2'), Key::Char('1')],
        [Key::Char('s'), Key::Escape],
        [Key::Char('e'), Key::Escape],
    ] {
        let mut session = workspace();
        session.key(Key::Down);
        for key in detour {
            session.key(key);
        }
        assert_eq!(session.model().screen, Screen::Main, "{detour:?}");

        session.key(Key::Right);

        assert_eq!(kernel(session.model()), "6.8.0-47-generic", "{detour:?}");
    }
}

#[test]
fn the_classic_map_opens_the_kernels_afresh_each_time() {
    let options = DemoOptions {
        classic: true,
        ..DemoOptions::default()
    };
    let mut session = Session::new(&options, Theme::new(Capabilities::FULL));
    session.start();
    session.key(Key::Escape);

    session.key(Key::Char('k'));
    session.key(Key::Down);
    session.key(Key::Escape);
    session.key(Key::Char('k'));

    assert_eq!(session.model().map, Map::Classic);
    assert_eq!(kernel(session.model()), "6.12.9_1");
    session.key(Key::Escape);
    session.key(Key::Right);
    assert_eq!(session.model().screen, Screen::Main);
}

#[test]
fn the_snapshots_tab_keeps_its_selection_for_the_same_environment() {
    let mut session = workspace();
    session.key(Key::Char('2'));
    session.key(Key::Down);
    session.key(Key::Down);
    let chosen = session.model().selected_snapshot().cloned();

    session.key(Key::Char('1'));
    session.key(Key::Char('2'));

    let model = session.model();
    assert_eq!(model.screen, Screen::Snapshots);
    assert!(chosen.is_some());
    assert_eq!(model.selected_snapshot().cloned(), chosen);
}

#[test]
fn a_refresh_keeps_the_kernel_by_name_in_the_workspace_alone() {
    for (map, expected) in [(Map::Workspace, "6.12.8_1"), (Map::Classic, "6.8.0_1")] {
        let mut model = workspace().into_model();
        model.map = map;
        model.refresh();
        let void = selected(&model).parse().unwrap();
        model.focus = Some(void);
        model.refresh();
        model.kernels.selected = 1;
        let mut snapshot = model.snapshot.clone();
        let environment = snapshot
            .environments
            .iter_mut()
            .find(|environment| environment.name.as_str() == "zroot/ROOT/void")
            .unwrap();
        environment.kernels.rotate_left(1);

        model.replace_snapshot(snapshot);

        assert_eq!(kernel(&model), expected, "{map:?}");
    }
}

#[test]
fn the_changes_still_switch_segments_with_left_and_right_in_the_workspace() {
    let mut session = workspace();
    session.key(Key::Char('2'));
    session.key(Key::Char('o'));
    session.key(Key::Home);
    session.key(Key::Char('f'));
    let segment = |session: &Session| session.model().diff.as_ref().map(|diff| diff.segment);
    assert_eq!(segment(&session), Some(Segment::Files));

    session.key(Key::Right);
    assert_eq!(segment(&session), Some(Segment::Packages));
    session.key(Key::Left);
    assert_eq!(segment(&session), Some(Segment::Files));
    assert_eq!(session.model().screen, Screen::Diff);
}

#[test]
fn the_bang_opens_the_recovery_tools_shell_first_in_each_map() {
    for classic in [false, true] {
        let options = DemoOptions {
            classic,
            ..DemoOptions::default()
        };
        let mut session = Session::new(&options, Theme::new(Capabilities::FULL));
        session.start();
        session.key(Key::Escape);

        session.key(Key::Char('!'));
        assert_eq!(session.model().screen, Screen::Recovery, "{classic}");
        session.key(Key::Enter);
        assert_eq!(
            session.actions.last(),
            Some(&crate::menu::world::Action::Shell),
            "{classic}"
        );
    }
}
