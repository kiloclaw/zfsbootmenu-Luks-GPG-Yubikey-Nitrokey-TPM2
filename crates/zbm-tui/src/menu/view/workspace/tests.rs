//! Tests of what needs attention in the workspace (ADR 0016, point 8).

use pretty_assertions::assert_eq;
use zbm_display::keys::Key;
use zbm_types::Untrusted;

use super::*;
use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, Session};
use crate::menu::theme::Theme;
use crate::menu::world::{Device, DeviceClass, LogEntry, PoolHost, Snapshot};

/// A device named `name` in `state`, with the devices `children`.
fn device(name: &str, state: DeviceState, children: Vec<Device>) -> Device {
    Device {
        name: Untrusted::new(name.to_owned()),
        class: DeviceClass::Data,
        state: Some(state),
        errors: None,
        note: None,
        children,
    }
}

/// The menu of a machine with `pools` and `log`, and nothing else.
fn model(pools: Vec<Pool>, log: Vec<LogEntry>) -> Model {
    let snapshot = Snapshot {
        pools,
        log: Log { entries: log },
        ..Snapshot::default()
    };
    Model::new(snapshot, Theme::new(Capabilities::FULL))
}

/// What needs attention, as it reads, with the tab each leads to.
fn said(model: &Model) -> Vec<(String, Tab)> {
    attention(model)
        .into_iter()
        .map(|item| (item.what, item.tab))
        .collect()
}

#[test]
fn a_pool_lists_its_lost_devices_worst_first_and_its_checkpoint() {
    let mut tank = Pool::imported("tank".parse().unwrap(), Health::Degraded);
    tank.checkpoint = Some(true);
    tank.devices = vec![
        device(
            "mirror-0",
            DeviceState::Degraded,
            vec![
                device("/dev/disk/by-id/ata-A", DeviceState::Removed, Vec::new()),
                device("/dev/sdb", DeviceState::Offline, Vec::new()),
            ],
        ),
        device(
            "mirror-1",
            DeviceState::Degraded,
            vec![
                // ZFS names a device it lost by its identifier, and says
                // which path it had.
                Device {
                    note: Some(Untrusted::new("was /dev/sdc\u{1b}".to_owned())),
                    ..device("9876543210987654321", DeviceState::Unavailable, Vec::new())
                },
                device("/dev/sdd", DeviceState::Faulted, Vec::new()),
            ],
        ),
    ];
    let model = model(vec![tank], Vec::new());

    let pools = |what: &str| (what.to_owned(), Tab::Pools);
    assert_eq!(
        said(&model),
        [
            pools("tank DEGRADED"),
            pools("tank: sdd FAULTED"),
            pools("tank: sdc\\x1b UNAVAIL"),
            pools("tank: ata-A REMOVED"),
            pools("tank: checkpoint"),
        ]
    );
    let roles: Vec<Role> = attention(&model).iter().map(|item| item.role).collect();
    assert_eq!(
        roles,
        [
            Role::Warning,
            Role::Danger,
            Role::Warning,
            Role::Warning,
            Role::Muted
        ]
    );
}

#[test]
fn pools_not_imported_say_who_used_them_last() {
    let not_imported = |name: &str, host: Option<PoolHost>| Pool {
        imported: false,
        checkpoint: None,
        host,
        ..Pool::imported(name.parse().unwrap(), Health::Online)
    };
    let named = PoolHost {
        hostid: None,
        hostname: Some(Untrusted::new("other\u{1b}[2J".to_owned())),
    };
    let known = PoolHost {
        hostid: Some("deadbeef".parse().unwrap()),
        hostname: Some(Untrusted::new("other".to_owned())),
    };
    let model = model(
        vec![
            not_imported("backup", None),
            not_imported("archive", Some(known)),
            not_imported("data", Some(named)),
            not_imported("lost", Some(PoolHost::default())),
        ],
        Vec::new(),
    );

    let what: Vec<String> = said(&model).into_iter().map(|(what, _)| what).collect();
    assert_eq!(
        what,
        [
            "backup: not imported",
            "archive: host deadbeef",
            "data: host other\\x1b[2J",
            "lost: another host",
        ]
    );
}

#[test]
fn the_log_s_errors_and_warnings_are_counted_apart() {
    let entry = |level| LogEntry {
        level,
        text: Untrusted::new("something".to_owned()),
    };
    let model = model(
        Vec::new(),
        vec![
            entry(Level::Warning),
            entry(Level::Error),
            entry(Level::Info),
            entry(Level::Warning),
        ],
    );

    let log = |what: &str| (what.to_owned(), Tab::Log);
    assert_eq!(
        said(&model),
        [log("1 error in the log"), log("2 warnings in the log")]
    );
    assert_eq!(attention_count(&model), 2);
}

/// The workspace's Pools tab with `log` loaded, at 80×24.
fn pools_tab(log: crate::menu::world::DeviceLog, map: Map) -> String {
    let mut model = model(Vec::new(), Vec::new());
    model.map = map;
    model.screen = crate::menu::model::Screen::Pools;
    model.device_log = Some(log);
    crate::menu::view::tests::screen(&model, 80, 24)
}

#[test]
fn a_devices_messages_show_in_the_workspace_alone_and_say_why_there_are_none() {
    let untrusted = |text: &str| Untrusted::new(text.to_owned());
    let log = crate::menu::world::DeviceLog {
        device: untrusted("/dev/sdb2"),
        names: vec![untrusted("sdb2"), untrusted("sdb")],
        lines: vec![untrusted("[ 1.0] sdb: \u{1b}]0;owned\u{7}")],
    };

    let shown = pools_tab(log.clone(), Map::Workspace);
    assert!(shown.contains("sdb: \\x1b]0;owned\\x07"), "{shown}");
    assert!(!shown.contains('\u{1b}'));
    assert!(shown.contains("name only a port"), "{shown}");
    // The classic menu shows nothing new.
    let classic = pools_tab(log.clone(), Map::Classic);
    assert!(!classic.contains("owned"), "{classic}");

    let quiet = crate::menu::world::DeviceLog {
        lines: Vec::new(),
        ..log.clone()
    };
    assert!(pools_tab(quiet, Map::Workspace).contains("No kernel message names sdb2, sdb."));
    let unknown = crate::menu::world::DeviceLog {
        names: Vec::new(),
        lines: Vec::new(),
        ..log
    };
    let shown = pools_tab(unknown, Map::Workspace);
    assert!(shown.contains("lists no block device /dev/sdb2"), "{shown}");
}

/// The demo in the workspace with `options`, started, after `keys`.
fn session_after(options: &DemoOptions, keys: &[Key]) -> Session {
    let options = DemoOptions {
        classic: false,
        ..options.clone()
    };
    let mut session = Session::new(&options, Theme::new(Capabilities::FULL));
    session.start();
    for key in keys {
        session.key(*key);
    }
    session
}

/// What the workspace draws at `width` by `height`.
fn drawn(session: &Session, (width, height): (u16, u16)) -> String {
    crate::menu::view::tests::screen(session.model(), width, height)
}

#[test]
fn the_path_shows_above_the_columns_at_every_size_with_the_countdown_and_hints() {
    let counting = session_after(&DemoOptions::default(), &[]);
    let kernels = session_after(&DemoOptions::default(), &[Key::Right, Key::Down]);

    for size in [(120, 40), (80, 25), (80, 24), (52, 20)] {
        let main = drawn(&counting, size);
        assert!(main.contains(" zroot › zroot/ROOT/void\n"), "{main}");
        assert!(main.contains("BOOT ENVIRONMENTS"), "{main}");
        assert!(main.contains("Booting zroot/ROOT/void in 10 s"), "{main}");
        assert!(main.contains("Enter Boot"), "{main}");
        assert!(main.contains("? More"), "{main}");
        let open = drawn(&kernels, size);
        assert!(
            open.contains("zroot › zroot/ROOT/void › 6.12.8_1"),
            "{open}"
        );
        assert!(open.contains("KERNELS"), "{open}");
    }
    // From 80 columns both columns show, at 52 the deepest alone.
    assert!(drawn(&kernels, (80, 24)).contains("BOOT ENVIRONMENTS"));
    assert!(!drawn(&kernels, (52, 20)).contains("BOOT ENVIRONMENTS"));
    assert!(drawn(&counting, (80, 24)).contains(": commands"));
}

#[test]
fn the_inspector_follows_the_deepest_column() {
    let mut session = session_after(&DemoOptions::default(), &[Key::Escape, Key::Right]);

    let kernel = drawn(&session, (80, 24));
    assert!(kernel.contains("┌ 6.12.9_1 ─"), "{kernel}");
    assert!(kernel.contains("boots ┐"), "{kernel}");
    assert!(kernel.contains("Environment  zroot/ROOT/void"), "{kernel}");
    assert!(kernel.contains("e edit"), "{kernel}");
    session.key(Key::Left);
    let environment = drawn(&session, (80, 24));
    assert!(environment.contains("┌ zroot/ROOT/void ─"), "{environment}");
    assert!(environment.contains("Boot Tries"), "{environment}");
}

#[test]
fn the_inspector_wraps_long_values_under_their_label() {
    let options = DemoOptions {
        long_command_line: true,
        ..DemoOptions::default()
    };
    let keys = [Key::Escape, Key::Down, Key::Down, Key::Right];
    let session = session_after(&options, &keys);

    let shown = drawn(&session, (80, 24));
    let lines: Vec<&str> = shown.lines().collect();
    let first = lines
        .iter()
        .position(|line| line.contains("Command Line  rw quiet splash"))
        .unwrap();
    let hanging = "│               ";
    assert!(lines[first + 1].starts_with(hanging), "{shown}");
    assert!(lines[first + 1].chars().nth(16).is_some_and(|c| c != ' '));
    assert!(
        shown.contains("resume=/dev/mapper/luks-swap · set here"),
        "{shown}"
    );
    // A word longer than a line breaks within it, and the classic menu
    // still cuts.
    let narrow = drawn(&session, (120, 40));
    assert!(narrow.contains("rd.udev.log_lev"), "{narrow}");
    assert!(narrow.contains("el=3"), "{narrow}");
    let mut model = session.into_model();
    model.map = Map::Classic;
    model.screen = crate::menu::model::Screen::Main;
    let classic = crate::menu::view::tests::screen(&model, 80, 25);
    assert!(classic.contains("rw quiet splash loglevel=3"), "{classic}");
    assert!(classic.contains('…'), "{classic}");
    assert!(!classic.contains("luks-swap"), "{classic}");
}

#[test]
fn the_line_view_numbers_the_environments_alone_and_says_the_path() {
    let session = session_after(&DemoOptions::default(), &[Key::Escape]);
    let model = session.model();

    let page = crate::menu::lines::page(model);

    assert_eq!(
        page.first().map(String::as_str),
        Some("Boot Environments: zroot > zroot/ROOT/void")
    );
    let numbered: Vec<&str> = page
        .iter()
        .filter(|line| line.trim_start().starts_with(|c: char| c.is_ascii_digit()))
        .map(|line| line.trim_start())
        .collect();
    assert_eq!(numbered.len(), 4, "{page:#?}");
    assert!(numbered[0].starts_with("1) zroot/ROOT/void"), "{page:#?}");
    assert!(numbered[2].starts_with("3) tank/ROOT/arch"), "{page:#?}");
    let mut classic = session.into_model();
    classic.map = Map::Classic;
    assert_eq!(
        crate::menu::view::words::title(&classic),
        "Boot Environments"
    );
}

#[test]
fn a_long_default_kernel_is_cut_to_keep_its_label_in_the_workspace_alone() {
    // `zroot/ROOT/ubuntu` names its second kernel.
    let keys = [Key::Escape, Key::Down, Key::Right];
    let mut model = session_after(&DemoOptions::default(), &keys).into_model();
    let mut snapshot = model.snapshot.clone();
    let kernel = snapshot
        .environments
        .iter_mut()
        .flat_map(|environment| environment.kernels.iter_mut())
        .find(|kernel| kernel.is_default)
        .unwrap();
    kernel.version = Untrusted::new(format!("6.8.0-47-{}", "x".repeat(50)));
    model.replace_snapshot(snapshot);

    let workspace = crate::menu::view::tests::screen(&model, 52, 20);
    model.map = Map::Classic;
    let classic = crate::menu::view::tests::screen(&model, 52, 20);

    assert!(workspace.contains("xx…  default"), "{workspace}");
    // The classic menu draws as before: the version runs to the edge.
    assert!(!classic.contains('…'), "{classic}");
    assert!(!classic.contains("default"), "{classic}");
}

#[test]
fn beside_the_sidebar_the_inspector_goes_below_until_the_environments_fit() {
    let session = session_after(&DemoOptions::default(), &[Key::Escape]);

    for width in [110, 119, 120] {
        let shown = drawn(&session, (width, 40));
        // Neither name is cut, nor the labels after them.
        for name in ["zroot/ROOT/void   ", "tank/ROOT/arch   "] {
            assert!(shown.contains(name), "{shown}");
        }
        assert!(shown.contains("default │"), "{shown}");
        assert!(shown.contains("1 warning │"), "{shown}");
    }
    // Below 120 columns the inspector is under the columns.
    let narrow = drawn(&session, (110, 40));
    let line = |text: &str| narrow.lines().position(|line| line.contains(text));
    assert!(
        line("┌ zroot/ROOT/void ─") > line("BOOT ENVIRONMENTS"),
        "{narrow}"
    );
}

#[test]
fn the_path_takes_one_row_from_the_columns_not_the_inspector_s_facts() {
    let counting = session_after(&DemoOptions::default(), &[]);

    let shown = drawn(&counting, (52, 20));

    for label in [
        "Checks",
        "Kernel",
        "Snapshots",
        "Encryption",
        "Command Line",
    ] {
        assert!(shown.contains(&format!("{label}  ")), "{label}: {shown}");
    }
}

#[test]
fn a_wrapped_value_stays_under_its_start_after_a_long_label() {
    let options = DemoOptions {
        long_command_line: true,
        ..DemoOptions::default()
    };
    let keys = [Key::Escape, Key::Down, Key::Down, Key::Right];
    let session = session_after(&options, &keys);
    let old = crate::text::set_language(crate::text::Language::Pseudo);

    let shown = drawn(&session, (52, 20));

    crate::text::set_language(old);
    let lines: Vec<&str> = shown.lines().collect();
    let first = lines
        .iter()
        .position(|line| line.contains("  rw quiet splash"))
        .unwrap();
    let (line, next) = (lines[first], lines[first + 1]);
    let start = line.split("rw quiet").next().unwrap().chars().count();
    let indent = next.chars().take_while(|c| [' ', '│'].contains(c)).count();
    assert_eq!(indent, start, "{shown}");
}

#[test]
fn with_no_environment_the_hint_names_the_keys_of_the_map() {
    let mut model = model(Vec::new(), Vec::new());
    model.screen = crate::menu::model::Screen::Main;
    // At 52 columns the one column is wide enough for the whole hint.
    let workspace = crate::menu::view::tests::screen(&model, 52, 20);
    model.map = Map::Classic;
    let classic = crate::menu::view::tests::screen(&model, 120, 40);

    assert!(workspace.contains("Press 3"), "{workspace}");
    assert!(
        workspace.contains("for the pools, ! for recovery."),
        "{workspace}"
    );
    assert!(!workspace.contains("Press p"), "{workspace}");
    assert!(classic.contains("Press p for the pools"), "{classic}");
}
