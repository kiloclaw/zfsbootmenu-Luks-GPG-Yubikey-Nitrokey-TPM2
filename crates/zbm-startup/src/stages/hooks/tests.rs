use super::*;
use zbm_bootenv::hooks::HOOKS;
use zbm_testkit::legacy::stub_lines;

/// The early stage as the `EarlyHooks` step runs it, without the
/// configuration's variables.
fn early_stage(
    system: &System<'_>,
    state: &State<'_>,
    hook_root: &str,
    skip: Option<&str>,
) -> Result<(), StartupError> {
    let root = Path::new(HOOKS);
    let prepared = prepare_early_stage(system, state, root, hook_root, skip);
    run_early_stage(system, state, root, &Variables::default());
    prepared
}

use zbm_platform::{Installed, KernelLog};
use zbm_testkit::fakes::FakeSystem;
use zbm_testkit::legacy::{Bash, LOG_STUBS, log_lines, path};
use zbm_testkit::{TempDir, parity};

fn log(fake: &FakeSystem) -> Installed {
    fake.files.dir("/zfsbootmenu");
    KernelLog::new(&fake.files, "7", Path::new("/zfsbootmenu"), 1).install()
}

fn skip_root() -> TempDir {
    let dir = TempDir::new("zbm-skip");
    for name in [
        "early-setup.d/10-a",
        "early-setup.d/20-b.sh",
        "setup.d/10-a",
        "setup.d/.10-a",
        "teardown.d/30-c",
        "other/10-a",
    ] {
        let file = dir.write(name, "");
        let mode = std::os::unix::fs::PermissionsExt::from_mode(0o755);
        std::fs::set_permissions(&file, mode).unwrap();
    }
    dir
}

/// The modes of the hooks under `dir`, by path relative to it.
fn skip_modes(dir: &TempDir) -> Vec<(String, u32)> {
    let mut modes = Vec::new();
    for stage in ["early-setup.d", "setup.d", "teardown.d", "other"] {
        for entry in std::fs::read_dir(dir.join(stage)).unwrap() {
            let entry = entry.unwrap();
            let mode = entry.metadata().unwrap().permissions();
            let mode = std::os::unix::fs::PermissionsExt::mode(&mode) & 0o7777;
            let name = entry.file_name().to_string_lossy().into_owned();
            modes.push((format!("{stage}/{name}"), mode));
        }
    }
    modes.sort();
    modes
}

/// `text` with the directory written as `$DIR`.
fn without_dir(text: &str, dir: &TempDir) -> String {
    text.replace(&*dir.path().to_string_lossy(), "$DIR")
}

#[test]
fn skipped_hooks_match_the_legacy_loop() {
    let mut parity = parity!("skipped_hooks_match_the_legacy_loop");
    for skip in [
        "",
        "10-a",
        "10-a,,20-b.sh",
        ",missing,30-c",
        "early-setup.d",
    ] {
        let (legacy_modes, legacy_log): (Vec<(String, u32)>, String) = parity.legacy(skip, || {
            let early = std::fs::read_to_string(path("zfsbootmenu/init.d/40-early-hooks")).unwrap();
            let start = early.find("# Remove the executable bit").unwrap();
            let end = early.find("# Run early setup hooks").unwrap();
            let dir = skip_root();
            let root = dir.path().to_string_lossy().into_owned();
            let snippet = early[start..end].replace("/libexec/hooks", &root);
            let legacy = Bash::new()
                .code(LOG_STUBS)
                .code(&format!(
                    "get_zbm_arg() {{ echo \"$SKIP\"; }}\nrun() {{\n{snippet}\n}}"
                ))
                .env("SKIP", skip)
                .call("run", &[], b"");
            (
                skip_modes(&dir),
                without_dir(&log_lines(&legacy.stderr_text()), &dir),
            )
        });

        let dir = skip_root();
        let files = zbm_platform::files::RealFiles;
        let kmsg = dir.join("kmsg");
        let _log = KernelLog::new(&files, "7", dir.path(), 1)
            .with_device(&kmsg)
            .install();
        skip_hooks(&files, dir.path(), skip);
        assert_eq!(skip_modes(&dir), legacy_modes, "{skip}");
        let rust_log = stub_lines(&std::fs::read_to_string(&kmsg).unwrap_or_default());
        assert_eq!(without_dir(&rust_log, &dir), legacy_log, "{skip}");
    }

    // Unlike the legacy loop, a name without `.sh` skips the `.sh` hook, as
    // it skips a built-in step.
    let dir = skip_root();
    skip_hooks(&zbm_platform::files::RealFiles, dir.path(), "20-b");
    let modes = skip_modes(&dir);
    assert!(
        modes.contains(&("early-setup.d/20-b.sh".to_owned(), 0)),
        "{modes:?}"
    );
}

#[test]
fn malformed_hook_roots_match_the_legacy_checks() {
    let mut parity = parity!("malformed_hook_roots_match_the_legacy_checks");
    for hook_root in ["", "tank", "tank//", "//hooks", "a//b//", "tank/hooks//zbm"] {
        let (success, logged): (bool, String) = parity.legacy(hook_root, || {
            let core =
                std::fs::read_to_string(path("zfsbootmenu/lib/zfsbootmenu-core.sh")).unwrap();
            let start = core.find("\nimport_zbm_hooks() {\n").unwrap() + 1;
            let end = start + core[start..].find("\n}\n").unwrap() + 3;
            let dir = TempDir::new("zbm-hookroot");
            let legacy = Bash::new()
                .code(LOG_STUBS)
                .code(&core[start..end])
                .code("mount() { return 1; }")
                .env("BASE", &dir.path().to_string_lossy())
                .call("import_zbm_hooks", &[hook_root], b"");
            (
                legacy.status.is_success(),
                log_lines(&legacy.stderr_text())
                    .replace(&*dir.path().to_string_lossy(), "/zfsbootmenu"),
            )
        });

        let fake = FakeSystem::new();

        let _log = log(&fake);
        fake.runner.respond_program("mount", 1, "");
        let state = State::new(&fake.files, "/zfsbootmenu");
        let imported = import_hooks(&fake.system(), &state, Path::new(HOOKS), hook_root);
        assert_eq!(imported.is_ok(), success, "{hook_root}");
        assert_eq!(stub_lines(&fake.kmsg()), logged, "{hook_root}");
    }
}

#[test]
fn imports_the_stage_directories_of_a_hook_root() {
    let fake = FakeSystem::new();
    let _log = log(&fake);
    let mnt = "/zfsbootmenu/.external_hooks";
    fake.files
        .file(format!("{mnt}/zbm/early-setup.d/10-net"), "")
        .file(format!("{mnt}/zbm/early-setup.d/.hidden"), "")
        .dir(format!("{mnt}/zbm/early-setup.d/sub"))
        .file(format!("{mnt}/zbm/teardown.d/90-off"), "")
        .file(format!("{mnt}/zbm/README"), "")
        // The image's hook that the failed copy overwrites in part.
        .file_with_mode("/libexec/hooks/teardown.d/90-off", "#!/bin/sh\n", 0o755);
    fake.runner.on(|command| {
        command
            .display()
            .ends_with("90-off /libexec/hooks/teardown.d")
            .then(|| {
                Ok(zbm_platform::exec::Output::new(
                    zbm_platform::exec::Status::exited(1),
                    "",
                ))
            })
    });
    let state = State::new(&fake.files, "/zfsbootmenu");
    assert!(import_hooks(&fake.system(), &state, Path::new(HOOKS), "tank/hooks//zbm",).is_ok());
    assert_eq!(
        fake.runner.lines(),
        [
            format!("mount -r tank/hooks {mnt}"),
            format!("cp {mnt}/zbm/early-setup.d/10-net /libexec/hooks/early-setup.d"),
            format!("cp {mnt}/zbm/teardown.d/90-off /libexec/hooks/teardown.d"),
            format!("umount {mnt}"),
        ]
    );
    assert!(fake.files.is_dir(Path::new("/libexec/hooks/teardown.d")));
    assert!(fake.kmsg().contains(&format!(
        "<4>ZFSBootMenu: failed to copy user hook {mnt}/zbm/teardown.d/90-off"
    )));
    // What the failed copy left does not run.
    let left = fake
        .files
        .metadata(Path::new("/libexec/hooks/teardown.d/90-off"))
        .unwrap();
    assert!(!left.is_executable());

    let fake = FakeSystem::new();
    let _log = log(&fake);
    fake.runner.respond_program("mount", 32, "");
    let state = State::new(&fake.files, "/zfsbootmenu");
    assert!(import_hooks(&fake.system(), &state, Path::new(HOOKS), "tank/hooks//zbm",).is_err());
    assert!(
        fake.kmsg()
            .contains("failed to mount hook filesystem tank/hooks")
    );
    assert_eq!(fake.runner.lines().len(), 1);
}

#[test]
fn the_hooks_copied_are_measured_into_pcr_9_before_they_run() {
    let fake = FakeSystem::new();
    let _log = log(&fake);
    let mnt = "/zfsbootmenu/.external_hooks";
    fake.files
        .file(format!("{mnt}/zbm/early-setup.d/10-net"), "net")
        .file(format!("{mnt}/zbm/teardown.d/90-off"), "off")
        .file(format!("{mnt}/zbm/teardown.d/95-gone"), "gone")
        // What `cp` made of the first two; the third's copy is gone.
        .file("/libexec/hooks/early-setup.d/10-net", "net")
        .file("/libexec/hooks/teardown.d/90-off", "off")
        .device("/dev/tpmrm0");
    fake.runner
        .respond("tpm2_getcap pcrs", 0, "selected-pcrs:\n  - sha256: [ 0 ]\n");
    let state = State::new(&fake.files, "/zfsbootmenu");

    assert!(import_hooks(&fake.system(), &state, Path::new(HOOKS), "tank/hooks//zbm").is_ok());

    let extended: Vec<String> = fake
        .runner
        .lines()
        .into_iter()
        .filter(|line| line.starts_with("tpm2_pcrextend 9:sha256="))
        .collect();
    assert_eq!(extended.len(), 2, "{:?}", fake.runner.lines());
    let first = Event::hook("early-setup.d/10-net", b"net");
    let digest = measured::hex(first.digests.get(measured::Bank::Sha256).unwrap());
    assert_eq!(extended[0], format!("tpm2_pcrextend 9:sha256={digest}"));
    // Measured after the copies, before any hook ran: the log has them.
    let log = fake.files.text("/zfsbootmenu/tpm-measure.json").unwrap();
    let events = measured::parse_log(&log).unwrap();
    let names: Vec<&str> = events.iter().map(|event| event.measured.value()).collect();
    assert_eq!(names, ["early-setup.d/10-net", "teardown.d/90-off"]);
    assert!(
        fake.kmsg()
            .contains("measured 2 hooks of the hook root into PCR 9")
    );
    assert!(
        fake.kmsg()
            .contains("unable to read the hook teardown.d/95-gone to measure it"),
        "{}",
        fake.kmsg()
    );
}

#[test]
fn the_early_stage_imports_skips_and_runs_once() {
    let fake = FakeSystem::new();
    let _log = log(&fake);
    fake.files
        .file_with_mode("/libexec/hooks/early-setup.d/10-a", "#!/bin/sh\n", 0o755)
        .file_with_mode("/libexec/hooks/early-setup.d/20-b", "#!/bin/sh\n", 0o755);
    let state = State::new(&fake.files, "/zfsbootmenu");
    assert!(early_stage(&fake.system(), &state, "", Some("20-b")).is_ok());
    // The built-in steps run among the hooks, the font's applet after them.
    assert_eq!(
        fake.runner.lines(),
        [
            "/libexec/hooks/early-setup.d/10-a",
            "/libexec/zbm console-autosize"
        ]
    );
    assert_eq!(
        fake.console.screens(),
        [zbm_platform::console::Screen::Clear]
    );
    assert!(
        fake.files
            .exists(Path::new("/libexec/hooks/early-setup.d/10-a.completed"))
    );
    assert!(
        fake.kmsg()
            .contains("Disabling hook: /libexec/hooks/early-setup.d/20-b")
    );
}

#[test]
fn a_hook_root_that_cannot_be_read_fails_the_stage_after_the_hooks() {
    let fake = FakeSystem::new();
    let _log = log(&fake);
    fake.files
        .file_with_mode("/libexec/hooks/early-setup.d/10-a", "#!/bin/sh\n", 0o755);
    let state = State::new(&fake.files, "/zfsbootmenu");
    let error = early_stage(&fake.system(), &state, "no-path", None).unwrap_err();
    assert!(matches!(error, StartupError::HookRoot { .. }), "{error:?}");
    // The image's own hooks ran all the same.
    assert!(
        fake.runner
            .lines()
            .contains(&"/libexec/hooks/early-setup.d/10-a".to_owned())
    );
}

#[test]
fn a_guarded_machine_keeps_the_luks_close() {
    // A locked image: zbm.skip_hooks skips other steps, not the close.
    let fake = FakeSystem::new();
    let _log = log(&fake);
    fake.files.dir("/zfsbootmenu").file(
        auth::LOCK_FILE,
        "$argon2id$v=19$m=65536,t=3,p=1$c2FsdA$aGFzaA\n",
    );
    let state = State::new(&fake.files, "/zfsbootmenu");
    let skip = Some("80-luks-close.sh,90-xhci-unbind");
    assert!(prepare_early_stage(&fake.system(), &state, Path::new(HOOKS), "", skip).is_ok());
    let names: Vec<&str> = steps::built_in(&state)
        .iter()
        .map(zbm_bootenv::hooks::BootStep::name)
        .collect();
    assert!(names.contains(&"80-luks-close.sh"), "{names:?}");
    assert!(!names.contains(&"90-xhci-unbind.sh"), "{names:?}");
    assert!(
        fake.kmsg()
            .contains("80-luks-close.sh, which this locked or measured machine runs anyway"),
        "{}",
        fake.kmsg()
    );

    // Under lockdown, a hook root's 80-luks-close.sh is disabled, and
    // zbm.skip_hooks keeps the close too.
    let fake = FakeSystem::new();
    let _log = log(&fake);
    let mnt = "/zfsbootmenu/.external_hooks";
    fake.files
        .dir("/zfsbootmenu")
        .file(
            "/sys/kernel/security/lockdown",
            "none [integrity] confidentiality\n",
        )
        .file(
            format!("{mnt}/zbm/teardown.d/80-luks-close.sh"),
            "#!/bin/sh\n",
        )
        .file(format!("{mnt}/zbm/teardown.d/85-mine.sh"), "#!/bin/sh\n");
    let copy = |file: &str| format!("/libexec/hooks/teardown.d/{file}");
    let files = fake.files.clone();
    fake.runner.on(move |command| {
        let line = command.display();
        line.starts_with("cp ").then(|| {
            let name = line
                .split('/')
                .nth(5)
                .unwrap_or_default()
                .split(' ')
                .next()
                .unwrap_or_default()
                .to_owned();
            files.file_with_mode(
                format!("/libexec/hooks/teardown.d/{name}"),
                "#!/bin/sh\n",
                0o755,
            );
            Ok(zbm_platform::exec::Output::new(
                zbm_platform::exec::Status::exited(0),
                "",
            ))
        })
    });
    let state = State::new(&fake.files, "/zfsbootmenu");
    let skip = Some("80-luks-close");
    assert!(
        prepare_early_stage(
            &fake.system(),
            &state,
            Path::new(HOOKS),
            "tank/hooks//zbm",
            skip
        )
        .is_ok()
    );
    let mode = |file: &str| {
        fake.files
            .metadata(Path::new(&copy(file)))
            .map(|m| m.is_executable())
    };
    assert_eq!(mode("80-luks-close.sh").ok(), Some(false));
    assert_eq!(mode("85-mine.sh").ok(), Some(true));
    let names: Vec<&str> = steps::built_in(&state)
        .iter()
        .map(zbm_bootenv::hooks::BootStep::name)
        .collect();
    assert!(names.contains(&"80-luks-close.sh"), "{names:?}");
    assert!(
        fake.kmsg()
            .contains("would replace a step this machine keeps"),
        "{}",
        fake.kmsg()
    );
}

#[test]
fn a_locked_image_ignores_the_hook_root_and_says_what_the_lock_is_worth() {
    let fake = FakeSystem::new();
    let _log = log(&fake);
    fake.files.file(
        auth::LOCK_FILE,
        "$argon2id$v=19$m=65536,t=3,p=1$c2FsdA$aGFzaA\n",
    );
    let state = State::new(&fake.files, "/zfsbootmenu");

    let prepared = prepare_early_stage(
        &fake.system(),
        &state,
        Path::new(HOOKS),
        "tank/hooks//zbm",
        None,
    );

    assert!(prepared.is_ok());
    // Nothing of the hook root was mounted or copied.
    assert!(
        !fake
            .runner
            .lines()
            .iter()
            .any(|line| line.contains("tank/hooks")),
        "{:?}",
        fake.runner.lines()
    );
    let logged = fake.kmsg();
    assert!(
        logged.contains("zbm.hookroot=tank/hooks//zbm is ignored"),
        "{logged}"
    );
    assert!(
        logged.contains("neither lockdown nor Secure Boot is on"),
        "{logged}"
    );
}
