use std::path::PathBuf;

use zbm_testkit::fakes::{FakeSystem, FileOperation};

use super::*;
use crate::Application as _;

/// The recovery of the tests: the shells it was asked to open.
#[derive(Default)]
struct Shells(std::sync::Mutex<Vec<String>>);

impl Recovery for Shells {
    fn unlock_late(&self) -> Result<(), zbm_boot::BootError> {
        Err(zbm_boot::BootError::NoCryptsetup)
    }

    fn emergency_shell(&self, message: &str) {
        self.0.lock().unwrap().push(message.to_owned());
    }
}

const BE: &str = "tank/ROOT/be";

/// [`BE`] as a dataset name.
fn be() -> DatasetName {
    BE.parse().unwrap()
}

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
    BootSettings::default().hooks("/libexec/hooks")
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
fn a_kernel_is_named_by_its_file_in_boot() {
    let fake = machine();
    let file = with_boot(&fake, |boot| boot.kernel_file(&be(), "6.1.10"));
    assert_eq!(file.as_deref(), Some("vmlinuz-6.1.10"));
    assert_eq!(
        with_boot(&fake, |boot| boot.kernel_file(&be(), "6.9")),
        None
    );

    let missing = FakeSystem::new();
    missing.runner.respond_program("mount", 32, "");
    assert_eq!(
        with_boot(&missing, |boot| boot.kernel_file(&be(), "6.1.10")),
        None
    );
}

#[test]
fn kernels_are_found_with_the_one_the_property_names() {
    let fake = machine();
    let kernels = with_boot(&fake, |boot| boot.kernels(&be()));
    let versions: Vec<&str> = kernels.iter().map(|k| k.version.get().as_str()).collect();
    assert_eq!(versions, ["6.6.2", "6.1.10"]);
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
    let kernels = with_boot(&named, |boot| boot.kernels(&be()));
    let marked: Vec<&str> = kernels
        .iter()
        .filter(|kernel| kernel.is_default)
        .map(|kernel| kernel.version.get().as_str())
        .collect();
    assert_eq!(marked, ["6.1.10"]);

    let missing = FakeSystem::new();
    missing.runner.respond_program("mount", 32, "");
    assert_eq!(with_boot(&missing, |boot| boot.kernels(&be())), []);
}

#[test]
fn the_kernel_the_menu_shows_first_is_the_one_that_boots() {
    let fake = machine();
    let (shown, booted) = with_boot(&fake, |boot| {
        let shown = boot
            .kernels(&be())
            .first()
            .map(|kernel| kernel.version.get().clone());
        let booted = zbm_bootenv::select_kernel(
            &zbm_platform::Context::new(*boot.system),
            boot.state,
            &be().into(),
        )
        .map(|kernel| kernel_version(&kernel.kernel));
        (shown, booted)
    });

    assert_eq!(shown.as_deref(), Some("6.6.2"));
    assert_eq!(shown, booted);
}

#[test]
fn a_typed_command_line_boots_once_without_root_and_with_noresume() {
    let fake = machine();
    State::new(&fake.files, "/zfsbootmenu")
        .set_noresume()
        .unwrap();
    let shells = Shells::default();
    let events = with_boot(&fake, |boot| {
        boot.boot(
            &BE.parse().unwrap(),
            Some("6.1.10"),
            Some("root=zfs:tank/other quiet resume=/dev/sda2 single"),
            false,
            &shells,
        )
    });

    // Nothing is written down for later boots.
    assert_eq!(fake.files.text("/zfsbootmenu/cmdline"), None);
    let loaded = lines(&fake);
    let load = loaded
        .iter()
        .find(|line| line.starts_with("kexec -a -l") && line.contains("vmlinuz-6.1.10"))
        .unwrap_or_else(|| panic!("{loaded:#?}"));
    assert!(load.contains("quiet single noresume"), "{load}");
    assert!(
        !load.contains("tank/other") && !load.contains("resume=/dev/sda2"),
        "{load}"
    );
    // The fake's kexec -e returns: the boot failed.
    assert!(matches!(
        events.last(),
        Some(Event::Finished(Outcome::Failed { what, .. })) if what == "unable to boot tank/ROOT/be"
    ));
}

/// A kernel of [`BE`] named by its files, as `zkexec` names one.
fn by_files() -> Kernel {
    Kernel::new(BE, "/boot/vmlinuz-6.1.10", "/boot/initramfs-6.1.10.img")
}

/// A caller of the boot: it boots on what it is given.
type Caller = fn(&Boot<'_, '_>) -> Vec<Event>;

/// The hooks of a boot chosen in the menu get the default boot environment
/// as the state directory has it now, as the legacy menu's `set_default_env`
/// changed `BOOTFS`, not as the menu started with it; so do those of a
/// kernel booted by its files (`zkexec`), the third caller of the boot
/// beside the menu's and the countdown's.
#[test]
fn the_hooks_get_the_default_as_it_is_now() {
    let callers: [(&str, Caller); 2] = [
        ("the menu", |boot| {
            boot.boot(&be(), Some("6.1.10"), None, false, &Shells::default())
        }),
        ("by its files", |boot| {
            boot.boot_files(&by_files(), &Shells::default())
        }),
    ];
    for (caller, run) in callers {
        let fake = machine();
        fake.files
            .file_with_mode("/libexec/hooks/boot-sel.d/10-sel", "#!/bin/sh\n", 0o755);
        State::new(&fake.files, "/zfsbootmenu")
            .set_bootfs("tank/ROOT/new")
            .unwrap();
        let system = fake.system();
        let state = State::new(&fake.files, "/zfsbootmenu");
        let mut settings = settings();
        settings.env = vec![("BOOTFS".to_owned(), "tank/ROOT/old".to_owned())].into();
        // With cryptsetup, the LUKS close has something to do.
        fake.environment.set("PATH", "/usr/bin");
        fake.files.file_with_mode("/usr/bin/cryptsetup", "", 0o755);
        let boot = Boot {
            system: &system,
            state: &state,
            settings: &settings,
        };
        let _ = run(&boot);

        let call = fake
            .runner
            .calls()
            .into_iter()
            .find(|call| call.program() == "/libexec/hooks/boot-sel.d/10-sel")
            .unwrap_or_else(|| panic!("{caller}: the hook ran"));
        let bootfs: Vec<_> = call
            .get_env()
            .iter()
            .filter(|(name, _)| name == "BOOTFS")
            .cloned()
            .collect();
        assert_eq!(
            bootfs,
            [("BOOTFS".into(), Some("tank/ROOT/new".into()))],
            "{caller}"
        );
        // The built-in steps of the teardown run: the boot carries the table.
        assert!(
            lines(&fake).iter().any(|line| line == "/bin/zluks close"),
            "{caller}"
        );
    }
}

/// A machine whose boot environment `tank/ROOT/be` is in the encryption
/// root `tank/ROOT`, locked, whose key `zfs load-key` refuses.
fn locked() -> FakeSystem {
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
    locked
}

#[test]
fn a_boot_without_its_key_or_kernel_says_why() {
    let locked = locked();
    let shells = Shells::default();
    let events = with_boot(&locked, |boot| {
        boot.boot(&BE.parse().unwrap(), None, None, true, &shells)
    });
    assert!(matches!(
        events.last(),
        Some(Event::Finished(Outcome::Failed { what, .. }))
            if what == "unable to load key for tank/ROOT/be"
    ));

    let fake = machine();
    let events = with_boot(&fake, |boot| {
        boot.boot(&BE.parse().unwrap(), Some("9.9"), None, false, &shells)
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
        boot.boot(&BE.parse().unwrap(), None, None, false, &shells)
    });
    assert!(matches!(
        events.last(),
        Some(Event::Finished(Outcome::Failed { .. }))
    ));
}

#[test]
fn a_reset_exports_the_writable_pools_of_the_names_it_is_given() {
    let fake = FakeSystem::new();
    fake.files.dir("/proc");
    fake.runner
        .respond("zpool get -H -o value readonly tank", 0, "off\n");
    fake.runner
        .respond("zpool get -H -o value readonly rpool", 0, "on\n");
    let pools: Vec<PoolName> = ["tank", "rpool"]
        .iter()
        .map(|name| name.parse().unwrap())
        .collect();
    let events = reset(&fake.system(), &pools, "o");

    assert!(lines(&fake).contains(&"zpool export tank".to_owned()));
    assert!(!lines(&fake).contains(&"zpool export rpool".to_owned()));
    assert_eq!(fake.files.text("/proc/sysrq-trigger").as_deref(), Some("o"));
    assert!(matches!(
        events.last(),
        Some(Event::Finished(Outcome::Failed { .. }))
    ));

    // No `/proc/sysrq-trigger`, though the fakes have `/proc/self`.
    let no_proc = FakeSystem::new();
    no_proc
        .files
        .fail("/proc/sysrq-trigger", std::io::ErrorKind::NotFound);
    let events = reset(&no_proc.system(), &[], "b");
    assert!(matches!(
        events.last(),
        Some(Event::Finished(Outcome::Failed { why, .. }))
            if why[0].starts_with("/proc/sysrq-trigger")
    ));
}

/// A UEFI machine whose firmware can open its setup at the next boot, with
/// `OsIndications` holding `value` if it is given.
fn uefi(value: Option<&[u8]>) -> FakeSystem {
    let fake = FakeSystem::new();
    fake.files.dir("/proc").dir("/sys/firmware/efi/efivars");
    fake.files.file(
        OS_INDICATIONS_SUPPORTED,
        [7, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0],
    );
    if let Some(value) = value {
        fake.files.file(OS_INDICATIONS, value);
    }
    fake
}

/// The writes of `OsIndications` that `fake` saw.
fn os_indications_writes(fake: &FakeSystem) -> Vec<Vec<u8>> {
    fake.files
        .operations()
        .into_iter()
        .filter_map(|operation| match operation {
            FileOperation::Write(path, contents) if path == Path::new(OS_INDICATIONS) => {
                Some(contents)
            }
            _ => None,
        })
        .collect()
}

#[test]
fn a_missing_os_indications_is_created_with_the_boot_to_firmware_bit() {
    let fake = uefi(None);
    firmware_setup(&fake.system(), &[]);
    // The attributes (non-volatile, boot service and runtime access) and
    // the 64-bit value, little-endian, in one write, as systemd writes them.
    let created = vec![7, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0];
    assert_eq!(os_indications_writes(&fake), [created]);
    assert_eq!(fake.files.text("/proc/sysrq-trigger").as_deref(), Some("b"));
}

#[test]
fn an_existing_os_indications_keeps_its_attributes_and_other_bits() {
    let fake = uefi(Some(&[0x27, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0x80]));
    firmware_setup(&fake.system(), &[]);
    assert_eq!(
        os_indications_writes(&fake),
        [vec![0x27, 0, 0, 0, 5, 0, 0, 0, 0, 0, 0, 0x80]]
    );
    assert_eq!(fake.files.text("/proc/sysrq-trigger").as_deref(), Some("b"));
}

#[test]
fn an_os_indications_with_the_bit_set_already_is_not_written_again() {
    // As systemd spares the firmware's store.
    let fake = uefi(Some(&[7, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0]));
    firmware_setup(&fake.system(), &[]);
    assert!(os_indications_writes(&fake).is_empty());
    assert_eq!(fake.files.text("/proc/sysrq-trigger").as_deref(), Some("b"));
}

#[test]
fn an_os_indications_that_cannot_be_read_is_not_written() {
    let fake = uefi(None);
    fake.files
        .fail(OS_INDICATIONS, std::io::ErrorKind::PermissionDenied);
    let events = firmware_setup(&fake.system(), &[]);
    assert!(matches!(
        events.last(),
        Some(Event::Finished(Outcome::Failed { why, .. }))
            if why[0].starts_with("OsIndications cannot be read: ")
    ));
    assert!(os_indications_writes(&fake).is_empty());
    assert!(fake.files.text("/proc/sysrq-trigger").is_none());
}

#[test]
fn an_os_indications_that_holds_no_64_bit_value_is_refused() {
    // systemd refuses a value of any other size (`get_os_indications`).
    for value in [&[7_u8][..], &[7, 0, 0, 0, 4, 0, 0, 0], &[7; 13]] {
        let fake = uefi(Some(value));
        assert_eq!(
            request_firmware_setup(&fake.system()),
            Err("the OsIndications variable holds no 64-bit value".to_owned())
        );
        assert_eq!(fake.files.contents(OS_INDICATIONS).as_deref(), Some(value));
    }
}

#[test]
fn the_firmware_setup_needs_the_firmware_to_say_it_can_open() {
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
    // Neither on a BIOS machine, nor where the firmware does not say so,
    // nor where it says so in no 64-bit value, as systemd reads it, is
    // `OsIndications` created.
    let efi = Path::new("/sys/firmware/efi");
    let bios = FakeSystem::new();
    bios.files.dir("/proc");
    assert!(!bios.system().files.is_dir(efi));
    let short = [7, 0, 0, 0, 1, 0, 0, 0];
    let long = [7, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0];
    for supported in [None, Some(&short[..]), Some(&long[..])] {
        let fake = FakeSystem::new();
        fake.files.dir("/proc").dir("/sys/firmware/efi/efivars");
        if let Some(supported) = supported {
            fake.files.file(OS_INDICATIONS_SUPPORTED, supported);
        }
        assert!(fake.system().files.is_dir(efi));
        assert!(request_firmware_setup(&fake.system()).is_err());
        assert!(fake.files.contents(OS_INDICATIONS).is_none());
    }
    assert!(request_firmware_setup(&bios.system()).is_err());
    assert!(bios.files.contents(OS_INDICATIONS).is_none());
}

/// An environment of the snapshot, with no kernels read yet.
fn environment(name: &str, encryption: model::Encryption) -> model::Environment {
    model::Environment {
        name: name.parse().unwrap(),
        is_default: false,
        encryption,
        kernels: Vec::new(),
        command_line: model::CommandLine {
            value: zbm_types::Untrusted::default(),
            origin: model::Origin::Default,
            expanded: zbm_types::Untrusted::default(),
        },
        snapshot_count: 0,
        newest_snapshot: None,
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
    let runtime = crate::Runtime::default().with_boot(settings());
    let app = crate::App::new(fake.context(), &runtime);
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
    let runtime = crate::Runtime::default().with_boot(settings());
    let app = crate::App::new(fake.context(), &runtime);
    let shells = Shells::default();
    let on_console =
        |action: &model::Action| app.apply(action, crate::Through::Console(fake.system(), &shells));

    assert!(on_console(&model::Action::Shell).is_empty());
    assert_eq!(*shells.0.lock().unwrap(), [""]);

    for (action, trigger) in [(model::Action::Reboot, "b"), (model::Action::PowerOff, "o")] {
        on_console(&action);
        assert_eq!(
            fake.files.text("/proc/sysrq-trigger").as_deref(),
            Some(trigger)
        );
    }
    let firmware = on_console(&model::Action::FirmwareSetup);
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
    let booted = on_console(&boot);
    assert!(matches!(
        booted.last(),
        Some(Event::Finished(Outcome::Failed { what, .. })) if what == "unable to boot tank/ROOT/be"
    ));
    assert!(
        lines(&fake)
            .iter()
            .any(|line| line.starts_with("kexec -a -l"))
    );

    let other = on_console(&model::Action::ResetBootTries(BE.parse().unwrap()));
    assert!(matches!(
        other.last(),
        Some(Event::Finished(Outcome::Failed { .. }))
    ));
}

mod files;
mod mounts;
