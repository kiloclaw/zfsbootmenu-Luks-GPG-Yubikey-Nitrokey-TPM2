use std::path::PathBuf;

use zbm_testkit::fakes::FakeSystem;

use super::*;
use crate::model::Health;

/// The recovery of the tests: the shells it was asked to open.
#[derive(Default)]
struct Shells(Vec<String>);

impl Recovery for Shells {
    fn unlock_late(&mut self) -> Result<(), zbm_boot::BootError> {
        Err(zbm_boot::BootError::NoCryptsetup)
    }

    fn emergency_shell(&mut self, message: &str) {
        self.0.push(message.to_owned());
    }
}

const BE: &str = "tank/ROOT/be";

/// A machine with the unencrypted boot environment `tank/ROOT/be`, whose
/// `/boot` holds two kernels with their initramfs.
fn machine() -> FakeSystem {
    let fake = FakeSystem::new();
    fake.runner
        .respond("zpool list -H -o feature@encryption tank", 0, "disabled\n");
    let boot = PathBuf::from("/zfsbootmenu/environments")
        .join(BE)
        .join("mnt/boot");
    for version in ["6.1.10", "6.6.2"] {
        fake.files
            .file(boot.join(format!("vmlinuz-{version}")), "kernel")
            .file(boot.join(format!("initramfs-{version}.img")), "initramfs");
    }
    fake
}

fn settings() -> BootSettings {
    BootSettings {
        hooks: "/libexec/hooks".into(),
        ..BootSettings::default()
    }
}

/// Runs `test` with the boot use case on `fake`.
fn with_boot<T>(fake: &FakeSystem, test: impl FnOnce(&Boot<'_, '_>) -> T) -> T {
    let system = fake.system();
    let state = State::new(&fake.files, "/zfsbootmenu");
    let settings = settings();
    test(&Boot {
        system: &system,
        state: &state,
        settings: &settings,
    })
}

fn lines(fake: &FakeSystem) -> Vec<String> {
    fake.runner.lines()
}

#[test]
fn versions_are_what_follows_the_kernel_s_name() {
    assert_eq!(kernel_version("/boot/vmlinuz-6.12.9_1"), "6.12.9_1");
    assert_eq!(kernel_version("/boot/vmlinuz-linux-lts"), "linux-lts");
    assert_eq!(kernel_version("/boot/vmlinuz"), "vmlinuz");
}

#[test]
fn kernels_are_found_with_the_one_the_property_names() {
    let fake = machine();
    let kernels = with_boot(&fake, |boot| boot.kernels(BE));
    let versions: Vec<&str> = kernels.iter().map(|k| k.version.as_str()).collect();
    assert_eq!(versions, ["6.1.10", "6.6.2"]);
    assert!(kernels.iter().all(|kernel| !kernel.is_default));

    let named = machine();
    named.runner.respond(
        &format!("zfs get -H -o value org.zfsbootmenu:kernel {BE}"),
        0,
        "6.1\n",
    );
    named.runner.on(|command| {
        let line = command.display();
        (line.starts_with("bash") || line.contains("=~")).then(|| {
            let matched = line.contains("6.1.10");
            Ok(zbm_platform::exec::Output::new(
                zbm_platform::exec::Status::exited(u8::from(!matched)),
                "",
            ))
        })
    });
    let kernels = with_boot(&named, |boot| boot.kernels(BE));
    assert!(kernels.first().is_some_and(|kernel| kernel.is_default));

    let missing = FakeSystem::new();
    missing.runner.respond_program("mount", 32, "");
    assert_eq!(with_boot(&missing, |boot| boot.kernels(BE)), []);
}

#[test]
fn a_boot_writes_the_command_line_and_executes_the_chosen_kernel() {
    let fake = machine();
    let mut shells = Shells::default();
    let events = with_boot(&fake, |boot| {
        boot.boot(
            &BE.parse().unwrap(),
            Some("6.1.10"),
            Some("quiet single"),
            false,
            &mut shells,
        )
    });

    assert_eq!(
        fake.files.text("/zfsbootmenu/cmdline").as_deref(),
        Some("quiet\nsingle\n")
    );
    let loaded = lines(&fake);
    assert!(
        loaded
            .iter()
            .any(|line| line.starts_with("kexec -a -l") && line.contains("vmlinuz-6.1.10")),
        "{loaded:#?}"
    );
    // The fake's kexec -e returns: the boot failed.
    assert!(matches!(
        events.last(),
        Some(Event::Finished(Outcome::Failed { what, .. })) if what == "unable to boot tank/ROOT/be"
    ));
}

#[test]
fn a_boot_without_its_key_or_kernel_says_why() {
    let locked = FakeSystem::new();
    let runner = &locked.runner;
    runner.respond("zpool list -H -o feature@encryption tank", 0, "active\n");
    runner.respond(
        "zfs get -H -o value encryptionroot tank/ROOT/be",
        0,
        "tank/ROOT\n",
    );
    runner.respond(
        "zfs get -H -o value keystatus tank/ROOT",
        0,
        "unavailable\n",
    );
    runner.respond("zfs get -H -o value keylocation tank/ROOT", 0, "prompt\n");
    runner.respond("zfs load-key -L prompt tank/ROOT", 1, "");
    let mut shells = Shells::default();
    let events = with_boot(&locked, |boot| {
        boot.boot(&BE.parse().unwrap(), None, None, true, &mut shells)
    });
    assert!(matches!(
        events.last(),
        Some(Event::Finished(Outcome::Failed { what, .. }))
            if what == "unable to load key for tank/ROOT/be"
    ));

    let fake = machine();
    let events = with_boot(&fake, |boot| {
        boot.boot(&BE.parse().unwrap(), Some("9.9"), None, false, &mut shells)
    });
    assert!(matches!(
        events.last(),
        Some(Event::Finished(Outcome::Failed { why, .. })) if why == &["it has no kernel 9.9"]
    ));
    let empty = FakeSystem::new();
    empty
        .runner
        .respond("zpool list -H -o feature@encryption tank", 0, "disabled\n");
    let events = with_boot(&empty, |boot| {
        boot.boot(&BE.parse().unwrap(), None, None, false, &mut shells)
    });
    assert!(matches!(
        events.last(),
        Some(Event::Finished(Outcome::Failed { .. }))
    ));
}

/// The pool `tank`, imported read/write.
fn pools() -> Vec<model::Pool> {
    vec![model::Pool {
        name: "tank".parse().unwrap(),
        health: Health::Online,
        writable: true,
        imported: true,
        foreign_hostid: None,
        checkpoint: false,
    }]
}

#[test]
fn resetting_exports_the_writable_pools_then_triggers_sysrq() {
    let fake = FakeSystem::new();
    fake.files.dir("/proc");
    fake.runner
        .respond("zpool get -H -o value readonly tank", 0, "off\n");
    let events = reset(&fake.system(), &pools(), "o");

    assert!(lines(&fake).contains(&"zpool export tank".to_owned()));
    assert_eq!(fake.files.text("/proc/sysrq-trigger").as_deref(), Some("o"));
    assert!(matches!(
        events.last(),
        Some(Event::Finished(Outcome::Failed { .. }))
    ));

    let no_proc = FakeSystem::new();
    let events = reset(&no_proc.system(), &[], "b");
    assert!(matches!(
        events.last(),
        Some(Event::Finished(Outcome::Failed { why, .. }))
            if why[0].starts_with("/proc/sysrq-trigger")
    ));
}

#[test]
fn the_firmware_setup_is_asked_for_through_os_indications() {
    let fake = FakeSystem::new();
    fake.files.dir("/proc").dir("/sys/firmware/efi/efivars");
    fake.files
        .file(OS_INDICATIONS_SUPPORTED, [7, 0, 0, 0, 1, 0, 0, 0])
        .file(OS_INDICATIONS, [7, 0, 0, 0, 4, 0, 0, 0]);
    firmware_setup(&fake.system(), &[]);
    assert_eq!(
        fake.files.contents(OS_INDICATIONS),
        Some(vec![7, 0, 0, 0, 5, 0, 0, 0])
    );
    assert_eq!(fake.files.text("/proc/sysrq-trigger").as_deref(), Some("b"));

    let unsupported = FakeSystem::new();
    unsupported
        .files
        .file(OS_INDICATIONS_SUPPORTED, [7, 0, 0, 0, 0, 0, 0, 0]);
    let events = firmware_setup(&unsupported.system(), &[]);
    assert!(matches!(
        events.last(),
        Some(Event::Finished(Outcome::Failed { why, .. }))
            if why[0].contains("cannot open its setup")
    ));
    for files in [
        vec![],
        vec![(OS_INDICATIONS_SUPPORTED, vec![7, 0, 0, 0, 1])],
        vec![
            (OS_INDICATIONS_SUPPORTED, vec![7, 0, 0, 0, 1]),
            (OS_INDICATIONS, vec![7]),
        ],
    ] {
        let fake = FakeSystem::new();
        for (path, contents) in files {
            fake.files.file(path, contents);
        }
        assert!(request_firmware_setup(&fake.system()).is_err());
    }
}

/// An environment of the snapshot, with no kernels read yet.
fn environment(name: &str, encryption: model::Encryption) -> model::Environment {
    model::Environment {
        name: name.parse().unwrap(),
        is_default: false,
        encryption,
        kernels: Vec::new(),
        command_line: model::CommandLine {
            value: String::new(),
            origin: model::Origin::Default,
        },
        snapshots: Vec::new(),
        checks: Vec::new(),
        tries: None,
    }
}

#[test]
fn the_app_reads_the_kernels_of_the_environments_it_can_mount() {
    let fake = machine();
    let mut snapshot = model::Snapshot {
        environments: vec![
            environment(BE, model::Encryption::None),
            environment("tank/ROOT/locked", model::Encryption::Locked),
        ],
        ..model::Snapshot::default()
    };
    let settings = settings();
    let base = std::path::Path::new("/zfsbootmenu");
    let unset = crate::App::new(zbm_platform::Context::new(fake.system()));
    unset.read_kernels(&mut snapshot, fake.system());
    assert!(snapshot.environments[0].kernels.is_empty());

    let app = unset.with_base(base).with_boot(&settings);
    app.read_kernels(&mut snapshot, fake.system());
    assert_eq!(snapshot.environments[0].kernels.len(), 2);
    assert!(snapshot.environments[1].kernels.is_empty());
    assert!(
        !lines(&fake)
            .iter()
            .any(|line| line.contains("tank/ROOT/locked"))
    );
}

#[test]
fn the_app_hands_the_console_to_the_shell_the_boot_and_the_resets() {
    let fake = machine();
    fake.files.dir("/proc");
    fake.runner.respond_program("zpool", 0, "");
    let settings = settings();
    let base = std::path::Path::new("/zfsbootmenu");
    let unset = crate::App::new(zbm_platform::Context::new(fake.system()));
    let app = unset.with_base(base).with_boot(&settings);
    let mut shells = Shells::default();

    assert!(
        app.act_on_console(&model::Action::Shell, fake.system(), &mut shells)
            .is_empty()
    );
    assert_eq!(shells.0, [""]);

    for (action, trigger) in [(model::Action::Reboot, "b"), (model::Action::PowerOff, "o")] {
        app.act_on_console(&action, fake.system(), &mut shells);
        assert_eq!(
            fake.files.text("/proc/sysrq-trigger").as_deref(),
            Some(trigger)
        );
    }
    let firmware = app.act_on_console(&model::Action::FirmwareSetup, fake.system(), &mut shells);
    assert!(matches!(
        firmware.last(),
        Some(Event::Finished(Outcome::Failed { .. }))
    ));

    let boot = model::Action::Boot {
        environment: BE.parse().unwrap(),
        kernel: None,
        command_line: None,
        countdown: false,
    };
    assert!(boot.needs_console() && model::Action::Shell.needs_console());
    assert!(!model::Action::ResetBootTries(BE.parse().unwrap()).needs_console());
    let booted = app.act_on_console(&boot, fake.system(), &mut shells);
    assert!(matches!(
        booted.last(),
        Some(Event::Finished(Outcome::Failed { what, .. })) if what == "unable to boot tank/ROOT/be"
    ));
    assert!(
        lines(&fake)
            .iter()
            .any(|line| line.starts_with("kexec -a -l"))
    );

    let unable = unset.act_on_console(&boot, fake.system(), &mut shells);
    assert!(matches!(
        unable.last(),
        Some(Event::Finished(Outcome::Failed { what, .. })) if what == "unable to do that here"
    ));
    let other = app.act_on_console(
        &model::Action::ResetBootTries(BE.parse().unwrap()),
        fake.system(),
        &mut shells,
    );
    assert!(matches!(
        other.last(),
        Some(Event::Finished(Outcome::Failed { .. }))
    ));
}
