use std::path::Path;

use zbm_config::args::ZbmArgs;
use zbm_testkit::fakes::FakeSystem;

use super::*;
use crate::model::{self, Level, Timeout};

#[test]
fn the_snapshot_reads_the_machine_and_what_the_runtime_holds() {
    let fake = FakeSystem::new();
    fake.runner.respond_program("zpool", 0, "");
    fake.runner.respond_program("zfs", 0, "");
    fake.files.file("/layouts/layouts", "us\nde\n").file(
        "/zfsbootmenu/journal.jsonl",
        "{\"level\":\"WARN\",\"message\":\"slow\"}\n",
    );
    let (mut options, _) = ZbmOptions::parse(&ZbmArgs::from_cmdline("zbm.timeout=7"));
    options.control_term = "ttyS0".into();
    let settings = zbm_luks::unlocker::Settings::new("/zfsbootmenu");
    let prompter = crate::read::tests::NoPrompts;
    let luks = Luks {
        context: fake.context(),
        settings: &settings,
        interaction: &prompter,
    };
    let runtime = Runtime::default()
        .with_options(options)
        .with_base(Path::new("/zfsbootmenu"))
        .with_layouts(Path::new("/layouts"));

    let app = App::new(Context::new(fake.system()), &runtime);
    let bare = app.snapshot().unwrap();
    let snapshot = app.with_luks(luks).snapshot().unwrap();

    assert!(bare.environments.is_empty() && bare.volumes.is_empty());
    assert_eq!(snapshot.timeout, Timeout::Seconds(7));
    assert_eq!(snapshot.keyboard.layouts, ["us", "de"]);
    assert!(!snapshot.keyboard.local);
    assert_eq!(snapshot.log.count(Level::Warning), 1);
    assert!(snapshot.volumes.is_empty() && snapshot.now > 0);
    assert_eq!(Runtime::default().layouts, Path::new(LAYOUTS));
    assert_eq!(Runtime::default().base, Path::new("/zfsbootmenu"));
}

/// A recovery that opens no shell.
pub(super) struct NoShell;

impl Recovery for NoShell {
    fn unlock_late(&self) -> Result<(), zbm_boot::BootError> {
        Err(zbm_boot::BootError::NoCryptsetup)
    }

    fn emergency_shell(&self, _message: &str) {}
}

#[test]
fn changes_apply_quietly_or_on_the_console() {
    let fake = FakeSystem::new();
    fake.files.dir("/zfsbootmenu");
    fake.runner
        .respond("zpool get -H -o value readonly tank", 0, "off\n");
    fake.runner
        .respond("zpool list -H -o feature@encryption tank", 0, "disabled\n");
    let runtime = Runtime::default();
    let app = App::new(Context::new(fake.system()), &runtime);
    let take = Action::CreateSnapshot("tank/ROOT/be@new".parse().unwrap());
    let is_done = |events: &[Event]| {
        matches!(
            events.last(),
            Some(Event::Finished(model::Outcome::Done(_)))
        )
    };

    assert!(is_done(&app.apply(&take, Through::Quietly(fake.system()))));
    let on_console = Through::Console(fake.system(), &NoShell);
    assert!(is_done(&app.apply(&take, on_console)));
    let taken = fake
        .runner
        .lines()
        .into_iter()
        .filter(|line| line == "zfs snapshot tank/ROOT/be@new")
        .count();
    assert_eq!(taken, 2);

    // An action of the console applied quietly is one it cannot do.
    let reboot = app.apply(&Action::Reboot, Through::Quietly(fake.system()));
    assert!(!is_done(&reboot));
}

/// Whether `events` end as the locked menu refuses.
fn is_refused(events: &[Event]) -> bool {
    events == auth::refused()
}

#[test]
fn the_locked_menu_refuses_what_it_keeps_until_the_pin_is_typed_on_the_console() {
    let fake = FakeSystem::new();
    fake.files.dir("/zfsbootmenu");
    fake.files.file("/zfsbootmenu/bootfs", "tank/ROOT/be\n");
    fake.runner.respond_program("zpool", 0, "");
    fake.runner.respond_program("zfs", 0, "");
    let runtime = Runtime::default();
    let authority = Authority::new(auth::Lock::On("hash".to_owned()));
    let app = App::new(Context::new(fake.system()), &runtime).with_authority(&authority);
    let rollback = Action::Rollback("tank/ROOT/be@old".parse().unwrap());
    let other = Action::Boot {
        environment: "tank/ROOT/other".parse().unwrap(),
        kernel: None,
        command_line: None,
        countdown: false,
    };

    assert!(app.snapshot().unwrap().menu_locked);
    // Quietly, it is refused at once; on the console without keyholders,
    // nothing checks a PIN.
    assert!(is_refused(
        &app.apply(&rollback, Through::Quietly(fake.system()))
    ));
    assert!(is_refused(
        &app.apply(&other, Through::Console(fake.system(), &NoShell))
    ));
    // What it does not keep goes on.
    let plan = Action::Plan(Box::new(rollback.clone()));
    assert!(!is_refused(
        &app.apply(&plan, Through::Quietly(fake.system()))
    ));

    // A wrong PIN, then a blank line: refused; the right one: done, and
    // the menu is open while it is used.
    for (typed, right) in [
        (&b"wrong pin"[..], false),
        (b"", false),
        (b"correct horse", true),
    ] {
        let (client, keyholder) = zbm_sys::socket::Connection::pair().unwrap();
        fake.console.secret(typed);
        let runner = &fake.runner;
        let events = std::thread::scope(|scope| {
            zbm_platform::event::spawn_scoped_traced(scope, move || {
                let _ = crate::keyholder::serve(
                    &keyholder,
                    runner,
                    |_, _| Vec::new(),
                    |pin| pin.expose() == b"correct horse",
                );
            });
            // Unused, the client's end goes, and the keyholder ends.
            let keyholders = crate::keyholder::tests::Given::new(client);
            let events = app
                .with_keyholders(&keyholders)
                .apply(&rollback, Through::Console(fake.system(), &NoShell));
            drop(keyholders);
            events
        });
        assert_eq!(is_refused(&events), !right, "{typed:?}: {events:?}");
    }
    assert!(!app.snapshot().unwrap().menu_locked);
    assert!(fake.console.output_text().contains(auth::LOCKED_PROMPT));

    // No keyholder starts: the PIN cannot be checked, and nothing runs.
    authority.forget();
    let none = crate::keyholder::tests::Given::none();
    fake.console.secret(b"correct horse");
    let events = app
        .with_keyholders(&none)
        .apply(&rollback, Through::Console(fake.system(), &NoShell));
    assert!(is_refused(&events), "{events:?}");
}

#[test]
fn the_lock_keeps_all_but_the_default_boot_and_what_changes_nothing() {
    use zbm_types::ImportPolicy::{Force, Hostid, Strict};

    let default: zbm_types::DatasetName = "tank/ROOT/be".parse().unwrap();
    let boot = |environment: &str, kernel: Option<&str>, countdown| Action::Boot {
        environment: environment.parse().unwrap(),
        kernel: kernel.map(|kernel| zbm_types::Untrusted::new(kernel.to_owned())),
        command_line: None,
        countdown,
    };
    let bootfs = Some(&default);
    assert!(!boot("tank/ROOT/be", None, false).is_kept_by_lock(bootfs));
    assert!(!boot("tank/ROOT/other", None, true).is_kept_by_lock(bootfs));
    assert!(boot("tank/ROOT/other", None, false).is_kept_by_lock(bootfs));
    assert!(boot("tank/ROOT/be", Some("6.1"), false).is_kept_by_lock(bootfs));
    assert!(boot("tank/ROOT/be", None, false).is_kept_by_lock(None));
    let pool: zbm_types::PoolName = "tank".parse().unwrap();
    let import = |access, policy, rewind| {
        let options = model::ImportOptions {
            access,
            policy,
            rewind,
            id: None,
        };
        Action::Import(pool.clone(), options).is_kept_by_lock(bootfs)
    };
    let (read_only, read_write) = (model::Access::ReadOnly, model::Access::ReadWrite);
    assert!(!import(read_only, Strict, false));
    assert!(import(read_only, Hostid, false));
    assert!(import(read_only, Force, false));
    assert!(import(read_write, Strict, false));
    assert!(import(read_only, Strict, true));
    for open in [Action::Shell, Action::Reboot, Action::UnlockAll] {
        assert!(!open.is_kept_by_lock(bootfs), "{open:?}");
    }
    let snapshot: zbm_types::SnapshotName = "tank/ROOT/be@old".parse().unwrap();
    let chroot = Action::Chroot(snapshot.clone().into());
    let boot_clone = Action::BootSnapshot(snapshot, "tank/ROOT/new".parse().unwrap());
    for kept in [chroot, boot_clone] {
        assert!(
            kept.is_kept_by_lock(bootfs) && kept.needs_console(),
            "{kept:?}"
        );
    }
}

/// An action of `kind`, on the environment `tank/ROOT/be` or its snapshot
/// `old`.
fn of_kind(kind: model::Offer) -> Action {
    use model::Offer;
    let environment: DatasetName = "tank/ROOT/be".parse().unwrap();
    let snapshot: zbm_types::SnapshotName = "tank/ROOT/be@old".parse().unwrap();
    let target: DatasetName = "tank/ROOT/new".parse().unwrap();
    match kind {
        Offer::Boot => Action::Boot {
            environment,
            kernel: None,
            command_line: None,
            countdown: false,
        },
        Offer::BootSnapshot => Action::BootSnapshot(snapshot, target),
        Offer::SetDefaultEnvironment => Action::SetDefaultEnvironment(environment),
        Offer::DefaultKernel => Action::InheritDefaultKernel(environment),
        Offer::CreateSnapshot => Action::CreateSnapshot("tank/ROOT/be@new".parse().unwrap()),
        Offer::Rollback => Action::Rollback(snapshot),
        Offer::Clone => Action::Clone(snapshot, target),
        Offer::Duplicate => Action::Duplicate(snapshot, target),
        Offer::Diff => Action::Diff(snapshot, None),
        Offer::RestoreFiles => Action::RestoreFiles {
            snapshot,
            paths: vec![zbm_types::Untrusted::new("/etc/fstab".to_owned())],
        },
        Offer::Chroot => Action::Chroot(environment.into()),
        Offer::Import => Action::Import(environment.pool(), model::ImportOptions::default()),
        Offer::Export => Action::Export(environment.pool()),
        Offer::OpenVolume => Action::Open {
            device: "/dev/sda2".to_owned(),
            name: None,
        },
        Offer::Reseal => Action::Reseal("luks-a".to_owned()),
        Offer::ResetBootTries => Action::ResetBootTries(environment),
        Offer::Shell => Action::Shell,
        Offer::FirmwareSetup => Action::FirmwareSetup,
        Offer::Power => Action::Reboot,
    }
}

#[test]
fn every_kind_the_app_offers_is_performed_and_the_others_are_unavailable() {
    // The chroot with `zfs-chroot`; opening a volume with a LUKS session,
    // in an image with `cryptsetup`.
    for (has_chroot, has_luks) in [(false, false), (true, true)] {
        let fake = FakeSystem::new();
        fake.files.dir("/zfsbootmenu");
        if has_chroot {
            fake.files.file(ZFS_CHROOT, "#!/bin/bash\n");
        }
        fake.environment.set("PATH", "/sbin");
        fake.files.file_with_mode("/sbin/cryptsetup", "", 0o755);
        fake.runner.respond_program("zpool", 0, "");
        fake.runner.respond_program("zfs", 0, "");
        let runtime = Runtime::default();
        let settings = zbm_luks::unlocker::Settings::new("/zfsbootmenu");
        let prompter = crate::read::tests::NoPrompts;
        let luks = Luks {
            context: fake.context(),
            settings: &settings,
            interaction: &prompter,
        };
        let mut app = App::new(Context::new(fake.system()), &runtime);
        if has_luks {
            app = app.with_luks(luks);
        }
        let offers = app.snapshot().unwrap().offers;

        assert!(!offers.is_empty());
        assert_eq!(offers.contains(&model::Offer::Chroot), has_chroot);
        assert_eq!(offers.contains(&model::Offer::OpenVolume), has_luks);
        for kind in model::Offer::ALL {
            let action = of_kind(kind);
            assert_eq!(action.offer(), Some(kind));
            let events = app.apply(&action, Through::Console(fake.system(), &NoShell));
            let is_unavailable = events == unavailable(&action);
            assert_eq!(
                is_unavailable,
                !offers.contains(&kind),
                "{kind:?}: {events:?}"
            );
        }
        // What only reads has no kind, and is performed whatever is offered.
        let device = zbm_types::Untrusted::new("/dev/sda2".to_owned());
        for action in [Action::ScanPools, Action::LoadDeviceLog(device)] {
            assert!(action.is_read() && action.offer().is_none());
            for through in [
                Through::Quietly(fake.system()),
                Through::Console(fake.system(), &NoShell),
            ] {
                let events = app.apply(&action, through);
                assert_ne!(events, unavailable(&action), "{action:?}");
            }
        }
    }
}

/// A machine whose pool `tank` is writable and unencrypted, and whose new
/// environment `tank/ROOT/new` has a kernel once it is mounted.
fn cloning_machine() -> FakeSystem {
    let fake = FakeSystem::new();
    fake.files.dir("/zfsbootmenu");
    fake.runner
        .respond("zpool get -H -o value readonly tank", 0, "off\n");
    fake.runner
        .respond("zpool list -H -o feature@encryption tank", 0, "disabled\n");
    let boot = "/zfsbootmenu/environments/tank/ROOT/new/mnt/boot";
    fake.files
        .file(format!("{boot}/vmlinuz-6.6.2"), "kernel")
        .file(format!("{boot}/initramfs-6.6.2.img"), "initramfs");
    fake
}

#[test]
fn a_snapshot_boots_as_a_new_clone_once_the_clone_is_made() {
    let fake = cloning_machine();
    let runtime = Runtime::default();
    let app = App::new(Context::new(fake.system()), &runtime);
    let action = Action::BootSnapshot(
        "tank/ROOT/be@old".parse().unwrap(),
        "tank/ROOT/new".parse().unwrap(),
    );

    let events = app.apply(&action, Through::Console(fake.system(), &NoShell));

    let lines = fake.runner.lines();
    let cloned = lines
        .iter()
        .position(|line| {
            line == "zfs clone -o mountpoint=/ -o canmount=noauto tank/ROOT/be@old tank/ROOT/new"
        })
        .unwrap_or_else(|| panic!("{lines:#?}"));
    let loaded = lines
        .iter()
        .position(|line| line.starts_with("kexec -a -l") && line.contains("tank/ROOT/new"))
        .unwrap_or_else(|| panic!("{lines:#?}"));
    assert!(cloned < loaded, "{lines:#?}");
    assert!(!lines.iter().any(|line| line.starts_with("zfs promote")));
    // The fake's kexec returns: the boot failed, and the clone stays.
    let Some(Event::Finished(model::Outcome::Failed { what, next, .. })) = events.last() else {
        panic!("{events:?}");
    };
    assert_eq!(what, "unable to boot tank/ROOT/new");
    assert_eq!(
        next.last().map(String::as_str),
        Some("tank/ROOT/new cloned from tank/ROOT/be@old: it stays, to boot or destroy")
    );
    // Quietly, nothing is cloned and nothing boots.
    let quiet = app.apply(&action, Through::Quietly(fake.system()));
    assert_eq!(quiet, unavailable(&action));
}

#[test]
fn a_snapshot_whose_clone_fails_does_not_boot() {
    let fake = cloning_machine();
    fake.runner.first(|command| {
        let failed = zbm_platform::Output::new(zbm_platform::Status::exited(1), "");
        command
            .display()
            .starts_with("zfs clone")
            .then(|| Ok(failed))
    });
    let runtime = Runtime::default();
    let app = App::new(Context::new(fake.system()), &runtime);
    let action = Action::BootSnapshot(
        "tank/ROOT/be@old".parse().unwrap(),
        "tank/ROOT/new".parse().unwrap(),
    );

    let events = app.apply(&action, Through::Console(fake.system(), &NoShell));

    assert!(
        matches!(
            events.last(),
            Some(Event::Finished(model::Outcome::Failed { what, .. }))
                if what == "unable to create tank/ROOT/new from tank/ROOT/be@old"
        ),
        "{events:?}"
    );
    let lines = fake.runner.lines();
    assert!(lines.iter().any(|line| line.starts_with("zfs clone")));
    assert!(
        !lines
            .iter()
            .any(|line| line.starts_with("kexec") || line.starts_with("mount")),
        "{lines:#?}"
    );
}

#[test]
fn the_chroot_runs_zfs_chroot_on_the_console_through_the_gate() {
    let fake = FakeSystem::new();
    fake.files.dir("/zfsbootmenu");
    fake.files.file(ZFS_CHROOT, "#!/bin/bash\n");
    let runtime = Runtime::default();
    let app = App::new(Context::new(fake.system()), &runtime);
    let on_console = |target: &str| {
        let action = Action::Chroot(target.parse().unwrap());
        app.apply(&action, Through::Console(fake.system(), &NoShell))
    };

    let environment = on_console("tank/ROOT/be");
    let snapshot = on_console("tank/ROOT/be@old");

    assert_eq!(
        fake.runner.lines(),
        [
            "/bin/zfs-chroot tank/ROOT/be",
            "/bin/zfs-chroot tank/ROOT/be@old"
        ]
    );
    let call = &fake.runner.calls()[0];
    assert_eq!(
        call.get_default_signals(),
        [zbm_platform::Signal::Interrupt]
    );
    assert_eq!(call.get_stdout(), zbm_platform::Stream::Inherit);
    assert_eq!(
        environment,
        [Event::Finished(model::Outcome::Done(
            "the shell in tank/ROOT/be ended".to_owned()
        ))]
    );
    assert!(matches!(
        snapshot.last(),
        Some(Event::Finished(model::Outcome::Done(_)))
    ));

    // A chroot that cannot start says why.
    fake.runner
        .first(|_| Some(Err(std::io::Error::from(std::io::ErrorKind::NotFound))));
    let failed = on_console("tank/ROOT/be");
    assert!(matches!(
        failed.last(),
        Some(Event::Finished(model::Outcome::Failed { what, .. }))
            if what == "unable to open a shell in tank/ROOT/be"
    ));
    // Nor is one done that opened no shell: zfs-chroot exits 1 when it
    // cannot mount the filesystem, one whose key is not loaded.
    fake.runner.first(|_| {
        let status = zbm_platform::Status::exited(1);
        Some(Ok(zbm_platform::Output::new(status, "")))
    });
    let refused = on_console("tank/ROOT/be@old");
    assert!(
        matches!(
            refused.as_slice(),
            [Event::Finished(model::Outcome::Failed { what, why, next })]
                if what == "unable to open a shell in tank/ROOT/be@old"
                    && why[0].contains("status 1")
                    && next[0] == "if tank/ROOT/be@old is locked, load its key, then try again"
        ),
        "{refused:?}"
    );
}

#[test]
fn the_locked_menu_runs_the_chroot_only_after_the_pin_and_locks_again() {
    let fake = FakeSystem::new();
    fake.files.dir("/zfsbootmenu");
    fake.files.file(ZFS_CHROOT, "#!/bin/bash\n");
    let runtime = Runtime::default();
    let authority = Authority::new(auth::Lock::On("hash".to_owned()));
    let app = App::new(Context::new(fake.system()), &runtime).with_authority(&authority);
    let chroot = Action::Chroot("tank/ROOT/be".parse().unwrap());
    let ran_chroot = || {
        fake.runner
            .lines()
            .iter()
            .filter(|line| line.contains("zfs-chroot"))
            .count()
    };

    // Quietly, and on the console with no keyholder to check a PIN.
    assert!(is_refused(
        &app.apply(&chroot, Through::Quietly(fake.system()))
    ));
    fake.console.secret(b"correct horse");
    assert!(is_refused(
        &app.apply(&chroot, Through::Console(fake.system(), &NoShell))
    ));
    assert_eq!(ran_chroot(), 0, "{:?}", fake.runner.lines());

    // The right PIN: the chroot runs, and the menu is locked again after.
    let (client, keyholder) = zbm_sys::socket::Connection::pair().unwrap();
    fake.console.secret(b"correct horse");
    let runner = &fake.runner;
    let events = std::thread::scope(|scope| {
        zbm_platform::event::spawn_scoped_traced(scope, move || {
            let _ = crate::keyholder::serve(
                &keyholder,
                runner,
                |_, _| Vec::new(),
                |pin| pin.expose() == b"correct horse",
            );
        });
        let keyholders = crate::keyholder::tests::Given::new(client);
        let events = app
            .with_keyholders(&keyholders)
            .apply(&chroot, Through::Console(fake.system(), &NoShell));
        drop(keyholders);
        events
    });
    assert!(
        matches!(
            events.last(),
            Some(Event::Finished(model::Outcome::Done(_)))
        ),
        "{events:?}"
    );
    assert_eq!(ran_chroot(), 1, "{:?}", fake.runner.lines());
    assert!(app.snapshot().unwrap().menu_locked);
}
