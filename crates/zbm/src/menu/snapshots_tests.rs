//! `zfsbootmenu --snapshots [ENVIRONMENT]`, which `zsnapshots` runs: the
//! menu on its Snapshots tab, served by the session or alone.

use std::sync::Mutex;

use zbm_app::model::{CommandLine, Encryption, Environment, Origin, Snapshot};
use zbm_app::remote::{ToMenu, ToSession, attach};
use zbm_sys::socket::Connection;
use zbm_testkit::fakes::FakeSystem;
use zbm_tui::menu::Model;
use zbm_tui::menu::model::Screen;
use zbm_tui::menu::world::Action;
use zbm_types::Untrusted;

use super::tests::remotely;
use super::*;

/// The boot environment `name`, the default one when `is_default`.
fn environment(name: &str, is_default: bool) -> Environment {
    Environment {
        name: name.parse().unwrap(),
        is_default,
        encryption: Encryption::None,
        kernels: Vec::new(),
        command_line: CommandLine {
            value: Untrusted::new(String::new()),
            origin: Origin::Default,
            expanded: Untrusted::new(String::new()),
        },
        snapshot_count: 3,
        newest_snapshot: None,
        checks: Vec::new(),
        tries: None,
    }
}

/// `tank/ROOT/void`, the default environment, and `tank/ROOT/arch`.
fn machine() -> Snapshot {
    Snapshot {
        environments: vec![
            environment("tank/ROOT/void", true),
            environment("tank/ROOT/arch", false),
        ],
        bootfs: Some("tank/ROOT/void".parse().unwrap()),
        ..Snapshot::default()
    }
}

/// A session that serves [`machine`], keeping what the menu sends until it
/// detaches in `sent`.
fn serving_environments(peer: &Connection, sent: &Mutex<Vec<ToSession>>) {
    let _terminal = attach(peer).unwrap();
    let wait = Some(std::time::Duration::from_secs(5));
    let read = ToSession::receive(peer, wait).unwrap().unwrap();
    assert_eq!(read.message, ToSession::Read);
    ToMenu::Snapshot(Some(Box::new(machine())))
        .send(peer)
        .unwrap();
    loop {
        let message = ToSession::receive(peer, wait).unwrap().unwrap().message;
        let detached = message == ToSession::Detach;
        sent.lock().unwrap().push(message);
        if detached {
            return;
        }
    }
}

/// The menu the session serves, opened on `opening` and shown by `show`:
/// the exit status of `zfsbootmenu`, what it wrote on standard error, and
/// what the menu sent the session.
fn served(
    name: &str,
    opening: &Opening,
    show: impl Fn(&Model) + Sync,
) -> ((Option<i32>, String), Vec<ToSession>) {
    let sent = Mutex::new(Vec::new());
    let session = |peer: Connection| serving_environments(&peer, &sent);
    let shown = |machine: RemoteMachine<'_>| {
        let theme = Theme::new(Capabilities::ASCII);
        let session = opened(Session::on(machine, theme, Map::Workspace), opening)?;
        show(session.model());
        Ok(String::new())
    };
    let status = remotely(name, Some(&session), shown);
    (status, sent.into_inner().unwrap())
}

/// Whether the menu asked the session for the snapshots of `environment`.
fn is_loading(sent: &[ToSession], environment: &DatasetName) -> bool {
    let load = Action::LoadSnapshots(environment.clone());
    sent.iter()
        .any(|message| matches!(message, ToSession::Send { action } if *action == load))
}

#[test]
fn snapshots_opens_on_the_snapshots_tab_of_the_environment() {
    let arch: DatasetName = "tank/ROOT/arch".parse().unwrap();
    let opening = Opening::Snapshots(Some(arch.clone()));
    let on_arch = |model: &Model| {
        assert_eq!(model.screen, Screen::Snapshots);
        assert_eq!(model.focus.as_ref(), Some(&arch));
        assert_eq!(model.notification, None);
    };

    let (status, sent) = served("snapshots-arch", &opening, on_arch);

    assert_eq!(status, (Some(0), String::new()));
    assert!(is_loading(&sent, &arch), "{sent:?}");
}

#[test]
fn without_an_environment_the_selected_one_s_snapshots_show() {
    let void: DatasetName = "tank/ROOT/void".parse().unwrap();
    let on_void = |model: &Model| {
        assert_eq!(model.screen, Screen::Snapshots);
        assert_eq!(model.focus.as_ref(), Some(&void));
    };

    let (status, sent) = served("snapshots-selected", &Opening::Snapshots(None), on_void);

    assert_eq!(status, (Some(0), String::new()));
    assert!(is_loading(&sent, &void), "{sent:?}");
}

/// What the menu in this process says when it is asked to open on the
/// snapshots of `name`, on a machine with no environment.
fn alone(name: &str) -> Result<i32, String> {
    let fake = FakeSystem::new();
    fake.files.file("/dev/zfs", "");
    let state = zbm_state::State::new(&fake.files, "/zfsbootmenu");
    state.create().unwrap();
    state.mark_initialized().unwrap();
    fake.runner.respond_program("zpool", 0, "");
    fake.runner.respond_program("zfs", 0, "");
    let opening = Opening::Snapshots(Some(name.parse().unwrap()));
    let mut drawn = false;
    let mut show = |machine: RealMachine<'_, '_>| {
        let theme = Theme::new(Capabilities::ASCII);
        let _session = opened(Session::on(machine, theme, Map::Workspace), &opening)?;
        drawn = true;
        Ok(String::new())
    };
    let (options, _) = ZbmOptions::parse(&zbm_config::args::ZbmArgs::from_cmdline(""));
    let conf = Conf::parse(&options.conf("/zfsbootmenu"));
    let status = menu(fake.system(), &conf, (&mut show, Countdown::Skip), None);
    assert!(!drawn);
    status
}

#[test]
fn a_filesystem_that_is_no_environment_is_refused_before_drawing() {
    let refused = "tank/data is not a boot environment: zfs list -t snapshot -r tank/data \
                   lists its snapshots";
    // The menu in this process.
    assert_eq!(alone("tank/data"), Err(refused.to_owned()));

    // The menu the session serves.
    let data = Opening::Snapshots(Some("tank/data".parse().unwrap()));
    let unreached = |_: &Model| panic!("no menu shows");
    let ((status, stderr), _) = served("snapshots-data", &data, unreached);
    assert_eq!(status, Some(zbm_startup::program::menu::FAILED));
    assert_eq!(stderr, format!("zfsbootmenu: {refused}\n"));
}

#[test]
fn the_classic_menu_refuses_snapshots() {
    let fake = FakeSystem::new();
    let (options, _) = ZbmOptions::parse(&zbm_config::args::ZbmArgs::from_cmdline(""));
    let conf = Conf::parse(&options.conf("/zfsbootmenu"));
    let snapshots = Opening::Snapshots(None);
    let wanted = |opening| menu_opening(&fake.system(), &conf, Capabilities::ASCII, opening);
    assert_eq!(wanted(&snapshots), Ok((false, Map::Workspace)));

    fake.files
        .file("/zfsbootmenu/zbm.cmdline", "zbm.menu=classic\n");

    assert_eq!(
        wanted(&snapshots),
        Err("the classic menu (zbm.menu=classic) has no Snapshots tab")
    );
    assert_eq!(wanted(&Opening::First), Ok((false, Map::Classic)));
}
