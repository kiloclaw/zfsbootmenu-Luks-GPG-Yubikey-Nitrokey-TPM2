use std::path::Path;

use pretty_assertions::assert_eq;
use zbm_bootenv::hooks::Variables;
use zbm_platform::KernelLog;
use zbm_testkit::fakes::FakeSystem;

use super::*;
use crate::stages::{DeviceWait, EarlyHooks, ImportPools, LuksUnlock, Modules};
use crate::step::{Rerun, Step, StepContext, run_steps};
use crate::testing::Recording;

/// A machine with a pool to import and an early hook.
fn machine() -> FakeSystem {
    let fake = FakeSystem::new();
    fake.files
        .dir("/zfsbootmenu")
        .dir("/hooks/early-setup.d")
        .file_with_mode("/hooks/early-setup.d/10-hook", "#!/bin/sh\n", 0o755);
    fake.runner.respond("zpool list -H -o name", 0, "tank\n");
    fake.runner
        .respond("zpool list -H -o name,health", 0, "tank\tONLINE\n");
    fake
}

#[test]
fn the_stages_ask_the_front_end() {
    let fake = machine();
    let system = fake.system();
    let state = State::new(&fake.files, "/zfsbootmenu");
    let _log = KernelLog::new(&fake.files, "7", Path::new("/zfsbootmenu"), 1).install();
    let none = Variables::default();
    let steps: Vec<Box<dyn Step>> = vec![
        Box::new(Modules),
        Box::new(DeviceWait {
            wait_for: "/dev/missing",
            retry_delay: zbm_types::RetryDelay::new(3).unwrap(),
        }),
        Box::new(EarlyHooks {
            root: Path::new("/hooks"),
            variables: &none,
            hook_root: "",
            skip: None,
        }),
        Box::new(LuksUnlock),
        Box::new(ImportPools {
            settings: ImportSettings::default(),
        }),
    ];

    let mut front = Recording::default();
    let mut context = StepContext::new(&system, &state, &mut front);
    run_steps(&steps, &mut context, Rerun::All);

    assert_eq!(
        front.asked,
        ["recovery", "devices 3", "console", "unlock", "import"]
    );
    assert_eq!(front.events.len(), 2 * steps.len());
    let hooks = fake.runner.lines();
    assert!(
        hooks
            .iter()
            .any(|line| line == "/hooks/early-setup.d/10-hook"),
        "{hooks:?}"
    );
}

#[test]
fn early_hooks_take_the_console_only_when_there_are_some() {
    let fake = FakeSystem::new();
    fake.files.dir("/zfsbootmenu").dir("/hooks/early-setup.d");
    let system = fake.system();
    let state = State::new(&fake.files, "/zfsbootmenu");
    let none = Variables::default();
    let steps: Vec<Box<dyn Step>> = vec![Box::new(EarlyHooks {
        root: Path::new("/hooks"),
        variables: &none,
        hook_root: "",
        skip: None,
    })];

    let mut front = Recording::default();
    let mut context = StepContext::new(&system, &state, &mut front);
    run_steps(&steps, &mut context, Rerun::All);

    assert!(front.asked.is_empty(), "{:?}", front.asked);
}

/// ADR 0017, point 7: the early-setup hooks get the configuration's
/// variables and `ZFSBOOTMENU_INITIALIZATION`, as the legacy runtime's did.
#[test]
fn early_hooks_get_the_configuration_and_the_initialization() {
    let fake = FakeSystem::new();
    fake.files.dir("/zfsbootmenu").file_with_mode(
        "/hooks/early-setup.d/10-hook",
        "#!/bin/sh\n",
        0o755,
    );
    let system = fake.system();
    let state = State::new(&fake.files, "/zfsbootmenu");
    let variables = Variables::from(vec![("ZBM_BUILDSTYLE".to_owned(), "dracut".to_owned())]);
    let steps: Vec<Box<dyn Step>> = vec![Box::new(EarlyHooks {
        root: Path::new("/hooks"),
        variables: &variables,
        hook_root: "",
        skip: None,
    })];

    let mut front = Recording::default();
    let mut context = StepContext::new(&system, &state, &mut front);
    run_steps(&steps, &mut context, Rerun::All);

    let calls = fake.runner.calls();
    let hook = calls
        .iter()
        .find(|call| call.program() == "/hooks/early-setup.d/10-hook")
        .expect("the hook ran");
    let pair = |name: &str, value: &str| (name.into(), Some(value.into()));
    assert_eq!(
        hook.get_env(),
        [
            pair("ZBM_BUILDSTYLE", "dracut"),
            pair("ZFSBOOTMENU_INITIALIZATION", "yes")
        ]
    );
}

#[test]
fn the_console_front_asks_on_the_console() {
    let fake = FakeSystem::new();
    fake.files.dir("/zfsbootmenu");
    fake.console.keys(b"\x1b");
    let state = State::new(&fake.files, "/zfsbootmenu");
    let system = fake.system();
    let mut front = ConsoleFront::new(system, &state);

    assert_eq!(front.devices_missing(&system, 3), Answer::Escape);
    let output = fake.console.output_text();
    assert!(output.contains("One or more required devices are missing"));
    let mut ran = false;
    front.with_console(&system, &mut |_| ran = true);
    assert!(ran);
    // Without cryptsetup there is nothing to unlock, now or late.
    front.unlock_luks(&system);
    assert!(front.recovery().unlock_late().is_err());
    let lines = fake.runner.lines();
    assert!(
        !lines.iter().any(|line| line.contains("zluks")),
        "{lines:?}"
    );
}

#[test]
fn without_a_console_nothing_is_asked() {
    let fake = FakeSystem::new();
    fake.files.dir("/zfsbootmenu");
    let system = fake.system();
    let _log = KernelLog::new(&fake.files, "7", Path::new("/zfsbootmenu"), 1).install();
    let mut front = NoRecovery;

    assert_eq!(front.devices_missing(&system, 5), Answer::Escape);
    front.unlock_luks(&system);
    assert!(front.recovery().unlock_late().is_err());
    front.recovery().emergency_shell("nothing to see");
    assert!(fake.runner.lines().is_empty());
    assert!(
        fake.kmsg()
            .contains("no recovery shell for: nothing to see")
    );
}
