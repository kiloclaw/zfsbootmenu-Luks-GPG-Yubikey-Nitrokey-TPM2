//! Tests of the built-in steps, on a fake machine.

use std::path::PathBuf;

use zbm_bootenv::hooks::{StageEnv, Variables};
use zbm_bootenv::kernels::Kernel;
use zbm_testkit::fakes::{FakeSystem, FileOperation};

use super::*;

const BASE: &str = "/zfsbootmenu";

/// The names of `steps`.
fn names(steps: &BootSteps) -> Vec<&'static str> {
    steps.iter().map(BootStep::name).collect()
}

/// Runs the step named `name` on `fake` with the variables `vars`.
fn run(fake: &FakeSystem, name: &str, vars: &[(&str, &str)]) -> Result<(), BootEnvError> {
    let variables = Variables::from(
        vars.iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect::<Vec<_>>(),
    );
    let kernel = Kernel::new("tank/ROOT/void", "/boot/vmlinuz", "/boot/initramfs.img");
    let step = TABLE
        .iter()
        .find(|step| step.name() == name)
        .expect("a step of that name");
    let stage = match step.stage() {
        HookStage::Teardown => StageEnv::Teardown { selected: &kernel },
        _ => StageEnv::EarlySetup,
    };
    let env = HookEnv {
        variables: &variables,
        stage,
    };
    step.run(&fake.context(), &env)
}

#[test]
fn the_table_is_the_six_hooks_in_the_order_of_their_names() {
    let fake = FakeSystem::new();
    fake.files.dir(BASE);
    let state = State::new(&fake.files, BASE);
    assert_eq!(
        names(&built_in(&state)),
        [
            "05-screen-readers.sh",
            "10-generic-efi-init.sh",
            "20-dracut-console-init.sh",
            "30-console-autosize.sh",
            "80-luks-close.sh",
            "90-xhci-unbind.sh",
        ]
    );
    let stages: Vec<HookStage> = TABLE.iter().map(|step| step.stage()).collect();
    assert_eq!(
        stages
            .iter()
            .filter(|stage| **stage == HookStage::EarlySetup)
            .count(),
        4
    );
    assert_eq!(kept(), ["teardown.d/80-luks-close.sh"]);
    // The images leave out the scripts of exactly these.
    let table: Vec<(HookStage, &str)> = TABLE
        .iter()
        .map(|step| (step.stage(), step.name()))
        .collect();
    assert_eq!(table, zbm_types::BUILT_IN_HOOKS);
}

/// ADR 0017, points 4 and 5: `zbm.skip_hooks` skips a step by its name,
/// with or without `.sh`, but not the LUKS close on a guarded machine; an
/// early step that ran does not run again; the records survive.
#[test]
fn skipped_and_early_steps_that_ran_are_left_out() {
    let fake = FakeSystem::new();
    fake.files.dir(BASE);
    let state = State::new(&fake.files, BASE);
    let kept = skip(
        &state,
        "90-xhci-unbind,30-console-autosize.sh,99-mine.sh,,80-luks-close.sh",
        true,
    );
    assert_eq!(kept, ["80-luks-close.sh"]);
    let steps = built_in(&state);
    assert_eq!(
        names(&steps),
        [
            "05-screen-readers.sh",
            "10-generic-efi-init.sh",
            "20-dracut-console-init.sh",
            "80-luks-close.sh",
        ]
    );
    record_ran(&state, &steps);
    assert_eq!(names(&built_in(&state)), ["80-luks-close.sh"]);

    // Unguarded, the LUKS close is skipped too.
    let open = FakeSystem::new();
    open.files.dir(BASE);
    let state = State::new(&open.files, BASE);
    assert!(skip(&state, "80-luks-close", false).is_empty());
    assert!(!names(&built_in(&state)).contains(&"80-luks-close.sh"));
    // Records that cannot be written are logged, and the boot goes on.
    let nowhere = State::new(&open.files, "/missing");
    let _ = skip(&nowhere, "90-xhci-unbind", false);
    record_ran(&nowhere, &built_in(&nowhere));
    assert_eq!(names(&built_in(&nowhere)).len(), 6);
}

#[test]
fn the_screen_readers_start_as_the_build_options_ask() {
    let fake = FakeSystem::new();
    assert_eq!(run(&fake, "05-screen-readers.sh", &[]), Ok(()));
    assert!(fake.runner.lines().is_empty());

    let vars = [
        ("ZBM_BRLTTY", "1"),
        ("ZBM_SPEAKUP", "soft"),
        ("ZBM_ESPEAKUP", "1"),
    ];
    assert_eq!(run(&fake, "05-screen-readers.sh", &vars), Ok(()));
    assert_eq!(
        fake.runner.lines(),
        ["brltty -q", "modprobe -q speakup_soft", "espeakup"]
    );
}

#[test]
fn the_efi_framebuffer_is_told_on_the_control_terminal() {
    let fake = FakeSystem::new();
    fake.files.device("/dev/tty1").device("/dev/console");
    assert_eq!(
        run(
            &fake,
            "10-generic-efi-init.sh",
            &[("control_term", "/dev/tty1")]
        ),
        Ok(())
    );
    assert_eq!(run(&fake, "10-generic-efi-init.sh", &[]), Ok(()));
    let writes: Vec<FileOperation> = fake
        .files
        .operations()
        .into_iter()
        .filter(|operation| matches!(operation, FileOperation::Write(..)))
        .collect();
    let text = b"\x1b[0;30m ... \x1b[0m\n".to_vec();
    assert_eq!(
        writes,
        [
            FileOperation::Write(PathBuf::from("/dev/tty1"), text.clone()),
            FileOperation::Write(PathBuf::from("/dev/console"), text),
        ]
    );
    let missing = FakeSystem::new();
    let error = run(
        &missing,
        "10-generic-efi-init.sh",
        &[("control_term", "/no/tty")],
    );
    assert!(matches!(error, Err(BootEnvError::Step { .. })), "{error:?}");
}

#[test]
fn dracut_s_console_set_up_runs_on_dracut_images_only() {
    let fake = FakeSystem::new();
    fake.files
        .file_with_mode(CONSOLE_INIT, "", 0o755)
        .device("/dev/tty1");
    let term = ("control_term", "/dev/tty1");
    assert_eq!(run(&fake, "20-dracut-console-init.sh", &[term]), Ok(()));
    assert_eq!(
        run(
            &fake,
            "20-dracut-console-init.sh",
            &[("ZBM_BUILDSTYLE", "mkinitcpio"), term]
        ),
        Ok(())
    );
    assert!(fake.runner.lines().is_empty());
    assert_eq!(
        run(
            &fake,
            "20-dracut-console-init.sh",
            &[("ZBM_BUILDSTYLE", "Dracut"), term]
        ),
        Ok(())
    );
    assert_eq!(fake.runner.lines(), ["/lib/udev/console_init tty1"]);
    // Not a terminal: nothing to set up.
    let file = FakeSystem::new();
    file.files
        .file_with_mode(CONSOLE_INIT, "", 0o755)
        .file("/dev/tty1", "");
    let vars = [("ZBM_BUILDSTYLE", "dracut"), term];
    assert_eq!(run(&file, "20-dracut-console-init.sh", &vars), Ok(()));
    assert!(file.runner.lines().is_empty());
}

/// A machine with `cryptsetup` on its `PATH`.
fn with_cryptsetup() -> FakeSystem {
    let fake = FakeSystem::new();
    fake.environment.set("PATH", "/usr/bin");
    fake.files.file_with_mode("/usr/bin/cryptsetup", "", 0o755);
    fake
}

#[test]
fn the_font_and_the_luks_close_run_their_applets() {
    let fake = with_cryptsetup();
    assert_eq!(run(&fake, "30-console-autosize.sh", &[]), Ok(()));
    assert_eq!(run(&fake, "80-luks-close.sh", &[]), Ok(()));
    assert_eq!(
        fake.runner.lines(),
        ["/libexec/zbm console-autosize", "/bin/zluks close"]
    );

    let open = with_cryptsetup();
    open.runner.respond("/bin/zluks close", 1, "");
    let error = run(&open, "80-luks-close.sh", &[]).unwrap_err();
    assert_eq!(
        error.to_string(),
        "the boot step 80-luks-close.sh failed: zluks close left mappings open"
    );
    let gone = FakeSystem::new();
    gone.runner.on(|command| {
        (command.program() == "/libexec/zbm")
            .then(|| Err(std::io::Error::from(std::io::ErrorKind::NotFound)))
    });
    assert!(run(&gone, "30-console-autosize.sh", &[]).is_err());
    // Without cryptsetup, there is nothing to close.
    assert_eq!(run(&gone, "80-luks-close.sh", &[]), Ok(()));
    assert!(
        gone.runner
            .lines()
            .iter()
            .all(|line| !line.contains("zluks"))
    );
}

#[test]
fn every_usb_controller_is_unbound() {
    let fake = FakeSystem::new();
    let driver = "/sys/bus/pci/drivers/xhci_hcd";
    fake.files
        .symlink(
            format!("{driver}/0000:00:14.0"),
            "../../../devices/pci0000:00/0000:00:14.0",
        )
        .symlink(
            format!("{driver}/0000:03:00.3"),
            "../../../devices/pci0000:00/0000:03:00.3",
        )
        .file(format!("{driver}/new_id"), "")
        .file(format!("{driver}/0000:00:15.0"), "")
        .device(format!("{driver}/unbind"));
    assert_eq!(run(&fake, "90-xhci-unbind.sh", &[]), Ok(()));
    let unbound: Vec<Vec<u8>> = fake
        .files
        .operations()
        .into_iter()
        .filter_map(|operation| match operation {
            FileOperation::Write(path, bytes) if path.ends_with("unbind") => Some(bytes),
            _ => None,
        })
        .collect();
    assert_eq!(
        unbound,
        [b"0000:00:14.0".to_vec(), b"0000:03:00.3".to_vec()]
    );
    // A controller that stays bound does not keep the next one bound.
    let stuck = FakeSystem::new();
    stuck
        .files
        .symlink(format!("{driver}/0000:00:14.0"), "../0000:00:14.0")
        .symlink(format!("{driver}/0000:03:00.3"), "../0000:03:00.3")
        .fail(
            format!("{driver}/unbind"),
            std::io::ErrorKind::PermissionDenied,
        );
    let error = run(&stuck, "90-xhci-unbind.sh", &[]).unwrap_err();
    assert!(error.to_string().contains("0000:00:14.0"), "{error}");
    assert!(error.to_string().contains("0000:03:00.3"), "{error}");
    // Without the driver, there is nothing to unbind.
    assert_eq!(run(&FakeSystem::new(), "90-xhci-unbind.sh", &[]), Ok(()));
    assert!(is_pci_address("0000:00:14.0") && !is_pci_address("new_id"));
}
