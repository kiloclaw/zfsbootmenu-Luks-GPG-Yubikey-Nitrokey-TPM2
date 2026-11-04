use super::*;
use crate::testing::stub_lines;
use zbm_platform::exec::{Input, Stream};
use zbm_platform::{Installed, KernelLog};
use zbm_testkit::fakes::{FakeFiles, FakeSystem, FileOperation};
use zbm_testkit::legacy::Bash;
use zbm_testkit::{TempDir, parity};

fn log(fake: &FakeSystem) -> Installed {
    fake.files.dir("/zfsbootmenu");
    KernelLog::new(&fake.files, "7", Path::new("/zfsbootmenu"), 1).install()
}

/// No built-in step.
static NO_STEPS: std::sync::LazyLock<BootSteps> = std::sync::LazyLock::new(BootSteps::default);

/// The environment of a stage without the boot's variables.
fn bare(stage: StageEnv<'_>) -> HookEnv<'_> {
    static NONE: Variables = Variables(Vec::new());
    HookEnv {
        variables: &NONE,
        stage,
    }
}

#[test]
fn runs_each_hook_with_the_stage_environment() {
    let fake = FakeSystem::new();
    let _log = log(&fake);
    let script = "#!/bin/sh\n";
    fake.files
        .file_with_mode("/libexec/hooks/setup.d/20-b", script, 0o755)
        .file_with_mode("/libexec/hooks/setup.d/10-a", script, 0o755)
        .file_with_mode("/libexec/hooks/setup.d/30-off", script, 0o644);
    let variables = Variables::from(vec![("BOOTFS".to_owned(), "tank/ROOT/void".to_owned())]);
    let env = HookEnv {
        variables: &variables,
        stage: StageEnv::Setup,
    };
    let ran = run_stage(&fake.context(), Path::new(HOOKS), &env, &NO_STEPS, false);
    assert!(ran > 0);
    assert_eq!(
        fake.runner.lines(),
        ["/libexec/hooks/setup.d/10-a", "/libexec/hooks/setup.d/20-b"]
    );
    for call in fake.runner.calls() {
        assert_eq!(call.get_stdin(), &Input::Inherit);
        assert_eq!(call.get_stdout(), Stream::Inherit);
        assert_eq!(call.get_stderr(), Stream::Inherit);
        assert_eq!(
            call.get_env(),
            [("BOOTFS".into(), Some("tank/ROOT/void".into()))]
        );
    }
    assert_eq!(
        stub_lines(&fake.kmsg()),
        "INFO: processing /libexec/hooks/setup.d/10-a\n\
         INFO: processing /libexec/hooks/setup.d/20-b\n"
    );
    // Without a directory, or without executable hooks, none runs.
    let locked = bare(StageEnv::LoadKey {
        fs: "tank/ROOT/void",
        root: "tank",
    });
    assert_eq!(
        run_stage(&fake.context(), Path::new(HOOKS), &locked, &NO_STEPS, false),
        0
    );
    assert!(fake.kmsg().contains("no hook directory for load-key.d"));
    fake.files.dir("/libexec/hooks/early-setup.d");
    assert_eq!(
        run_stage(
            &fake.context(),
            Path::new(HOOKS),
            &bare(StageEnv::EarlySetup),
            &NO_STEPS,
            false
        ),
        0
    );
}

/// ADR 0017, point 7: each stage's variables, after the boot's, as the
/// legacy runtime gave them.
#[test]
fn each_stage_has_its_variables_after_the_boot_ones() {
    let variables = Variables::from(vec![
        ("ZBM_BUILDSTYLE".to_owned(), "dracut".to_owned()),
        ("BOOTFS".to_owned(), "tank/ROOT/void".to_owned()),
    ]);
    let kernel = Kernel::new(
        "tank/ROOT/void",
        "/boot/vmlinuz-6.12",
        "/boot/initramfs-6.12.img",
    );
    let env = |stage| HookEnv {
        variables: &variables,
        stage,
    };
    let boot = [("ZBM_BUILDSTYLE", "dracut"), ("BOOTFS", "tank/ROOT/void")];
    let selected = [
        ("ZBM_SELECTED_BE", "tank/ROOT/void"),
        ("ZBM_SELECTED_KERNEL", "/boot/vmlinuz-6.12"),
        ("ZBM_SELECTED_INITRAMFS", "/boot/initramfs-6.12.img"),
    ];
    let with = |extra: &[(&'static str, &'static str)]| {
        boot.iter().chain(extra).copied().collect::<Vec<_>>()
    };
    assert_eq!(
        env(StageEnv::EarlySetup).vars(),
        with(&[("ZFSBOOTMENU_INITIALIZATION", "yes")])
    );
    assert_eq!(env(StageEnv::Setup).vars(), with(&[]));
    assert_eq!(
        env(StageEnv::LoadKey {
            fs: "tank/ROOT/void",
            root: "tank"
        })
        .vars(),
        with(&[
            ("ZBM_LOCKED_FS", "tank/ROOT/void"),
            ("ZBM_ENCRYPTION_ROOT", "tank")
        ])
    );
    let mut boot_sel = selected.to_vec();
    boot_sel.push(("ZBM_SELECTED_MOUNTPOINT", "/mnt"));
    assert_eq!(
        env(StageEnv::BootSel {
            selected: &kernel,
            mountpoint: "/mnt"
        })
        .vars(),
        with(&boot_sel)
    );
    assert_eq!(
        env(StageEnv::Teardown { selected: &kernel }).vars(),
        with(&selected)
    );
    for (stage, expected) in [
        (StageEnv::EarlySetup, HookStage::EarlySetup),
        (StageEnv::Setup, HookStage::Setup),
        (
            StageEnv::Teardown { selected: &kernel },
            HookStage::Teardown,
        ),
    ] {
        assert_eq!(stage.stage(), expected);
    }
}

#[test]
fn early_hooks_run_once() {
    let fake = FakeSystem::new();
    let _log = log(&fake);
    fake.files
        .file_with_mode("/libexec/hooks/early-setup.d/10-a", "", 0o755)
        .file_with_mode("/libexec/hooks/early-setup.d/20-b", "", 0o700);
    let run = || {
        run_stage(
            &fake.context(),
            Path::new(HOOKS),
            &bare(StageEnv::EarlySetup),
            &NO_STEPS,
            true,
        )
    };
    assert!(run() > 0);
    let operations: Vec<FileOperation> = fake
        .files
        .operations()
        .into_iter()
        .filter(|operation| {
            matches!(
                operation,
                FileOperation::SetMode(..) | FileOperation::Rename(..)
            )
        })
        .collect();
    let hook = |name: &str| PathBuf::from(format!("/libexec/hooks/early-setup.d/{name}"));
    assert_eq!(
        operations,
        [
            FileOperation::SetMode(hook("10-a"), 0),
            FileOperation::Rename(hook("10-a"), hook("10-a.completed")),
            FileOperation::SetMode(hook("20-b"), 0),
            FileOperation::Rename(hook("20-b"), hook("20-b.completed")),
        ]
    );
    assert!(fake.kmsg().contains(
        "<6>ZFSBootMenu: Disabling hook after execution: /libexec/hooks/early-setup.d/20-b"
    ));
    // The second time, nothing is left to run.
    assert_eq!(run(), 0);
    assert_eq!(fake.runner.lines().len(), 2);
}

/// A hook is told by its first bytes, read through the files port: a text
/// without `#!` runs on the shell, here Bash, as no shell was chosen, and
/// an ELF hook as it is (fixes 21 and 22).
#[test]
fn a_hook_runs_as_its_first_bytes_say() {
    let fake = FakeSystem::new();
    let _log = log(&fake);
    fake.files
        .file_with_mode("/libexec/hooks/setup.d/10-plain", "echo hi\n", 0o755)
        .file_with_mode("/libexec/hooks/setup.d/20-elf", "\x7fELF\x02", 0o755);
    assert!(
        run_stage(
            &fake.context(),
            Path::new(HOOKS),
            &bare(StageEnv::Setup),
            &NO_STEPS,
            false
        ) > 0
    );
    assert_eq!(
        fake.runner.lines(),
        [
            "/bin/bash /libexec/hooks/setup.d/10-plain",
            "/libexec/hooks/setup.d/20-elf"
        ]
    );
}

/// A step that records that it ran, or fails.
#[derive(Debug)]
struct Recorded {
    name: &'static str,
    stage: HookStage,
    fails: bool,
    ran: std::sync::Mutex<usize>,
}

impl BootStep for Recorded {
    fn name(&self) -> &'static str {
        self.name
    }

    fn stage(&self) -> HookStage {
        self.stage
    }

    fn run(&self, context: &Context<'_>, env: &HookEnv<'_>) -> Result<(), BootEnvError> {
        *self.ran.lock().unwrap() += 1;
        // What the step sees runs as a hook would: its stage's variables.
        let line = format!("step {} {}", self.name, env.vars().len());
        let _ = context.runner().run(&zbm_platform::Command::new(line));
        if self.fails {
            return Err(BootEnvError::Step {
                step: self.name.to_owned(),
                why: "it failed".to_owned(),
            });
        }
        Ok(())
    }
}

fn recorded(name: &'static str, stage: HookStage, fails: bool) -> &'static Recorded {
    Box::leak(Box::new(Recorded {
        name,
        stage,
        fails,
        ran: std::sync::Mutex::new(0),
    }))
}

/// ADR 0017, point 1: steps run among the stage's hooks in the order of
/// their names, a hook of a step's name replaces it, a step of another
/// stage does not run, and a failing step lets the stage go on.
#[test]
fn steps_run_among_the_hooks_in_the_order_of_their_names() {
    let fake = FakeSystem::new();
    let _log = log(&fake);
    let script = "#!/bin/sh\n";
    fake.files
        .file_with_mode("/libexec/hooks/teardown.d/85-user.sh", script, 0o755)
        .file_with_mode("/libexec/hooks/teardown.d/95-mine.sh", script, 0o755);
    let close = recorded("80-luks-close.sh", HookStage::Teardown, true);
    let unbind = recorded("90-xhci-unbind.sh", HookStage::Teardown, false);
    let replaced = recorded("95-mine.sh", HookStage::Teardown, false);
    let early = recorded("10-generic-efi-init.sh", HookStage::EarlySetup, false);
    let steps = BootSteps::from(vec![
        early as &dyn BootStep,
        close as &dyn BootStep,
        unbind as &dyn BootStep,
        replaced as &dyn BootStep,
    ]);
    let kernel = Kernel::new("tank/ROOT/void", "/boot/vmlinuz", "/boot/initramfs.img");
    let env = bare(StageEnv::Teardown { selected: &kernel });

    let ran = run_stage(&fake.context(), Path::new(HOOKS), &env, &steps, false);

    assert_eq!(ran, 4);
    assert_eq!(
        fake.runner.lines(),
        [
            "step 80-luks-close.sh 3",
            "/libexec/hooks/teardown.d/85-user.sh",
            "step 90-xhci-unbind.sh 3",
            "/libexec/hooks/teardown.d/95-mine.sh",
        ]
    );
    assert_eq!(*replaced.ran.lock().unwrap(), 0);
    assert_eq!(*early.ran.lock().unwrap(), 0);
    let kmsg = fake.kmsg();
    assert!(
        kmsg.contains("the boot step 80-luks-close.sh failed: it failed"),
        "{kmsg}"
    );
    assert!(
        kmsg.contains("the hook 95-mine.sh replaces its built-in step"),
        "{kmsg}"
    );
    assert!(
        kmsg.contains("running the built-in step 90-xhci-unbind.sh"),
        "{kmsg}"
    );
    // Without a hook directory, the stage's steps still run.
    let none = run_stage(&fake.context(), Path::new("/nowhere"), &env, &steps, false);
    assert_eq!(none, 3);
}

#[test]
fn steps_compare_and_print_by_their_names() {
    let one = recorded("10-a.sh", HookStage::Setup, false);
    let two = recorded("20-b.sh", HookStage::Setup, false);
    let both = BootSteps::from(vec![one as &dyn BootStep, two as &dyn BootStep]);
    assert_eq!(format!("{both:?}"), r#"["10-a.sh", "20-b.sh"]"#);
    let first = both.clone().retain(|step| step.name() == "10-a.sh");
    assert_eq!(first, BootSteps::from(vec![one as &dyn BootStep]));
    assert_ne!(first, both);
    assert_ne!(
        BootSteps::from(vec![two as &dyn BootStep]),
        BootSteps::from(vec![one as &dyn BootStep])
    );
    assert!(!one.is_kept_when_guarded());
}

/// A hook root with executable hooks in several stages.
#[test]
fn lists_what_the_legacy_glob_and_test_run() {
    let files = FakeFiles::new();
    let root = Path::new(HOOKS);
    files.file_with_mode("/libexec/hooks/early-setup.d/20-b.sh", "", 0o755);
    files.file_with_mode("/libexec/hooks/early-setup.d/10-a.sh", "", 0o700);
    files.file_with_mode("/libexec/hooks/early-setup.d/30-off.sh", "", 0o644);
    files.file_with_mode("/libexec/hooks/early-setup.d/40-done.sh.completed", "", 0);
    files.file_with_mode("/libexec/hooks/early-setup.d/.hidden", "", 0o755);
    files.file_with_mode("/libexec/hooks/early-setup.d/Z-upper", "", 0o001);
    files.dir("/libexec/hooks/early-setup.d/50-dir");
    files.symlink("/libexec/hooks/early-setup.d/60-link", "20-b.sh");
    files.symlink("/libexec/hooks/early-setup.d/70-dangling", "missing");
    let names: Vec<String> = executable_hooks(&files, root, HookStage::EarlySetup)
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        ["10-a.sh", "20-b.sh", "50-dir", "60-link", "Z-upper"]
    );
    assert!(executable_hooks(&files, root, HookStage::LoadKey).is_empty());
}

/// Whether the tests run as root, as the legacy code always does; `[ -x ]`
/// means something else for other users.
fn running_as_root() -> bool {
    std::fs::read_to_string("/proc/self/status").is_ok_and(|status| {
        status
            .lines()
            .find_map(|line| line.strip_prefix("Uid:"))
            .and_then(|ids| ids.split_whitespace().nth(1))
            == Some("0")
    })
}

#[test]
fn matches_the_legacy_glob_on_real_files() {
    use std::os::unix::fs::PermissionsExt;
    use zbm_platform::files::RealFiles;

    if !running_as_root() {
        #[allow(
            clippy::disallowed_macros,
            reason = "the test harness shows a skipped test's standard error"
        )]
        {
            eprintln!("skipped: the legacy code runs as root");
        }
        return;
    }
    let stage_dir = || {
        let dir = TempDir::new("zbm-hooks");
        let stage = dir.join("teardown.d");
        std::fs::create_dir(&stage).unwrap();
        for (name, mode) in [
            ("90-xhci", 0o755),
            ("80-luks", 0o700),
            ("85-off", 0o644),
            ("10 space", 0o100),
            (".hidden", 0o755),
            ("x.completed", 0o000),
            ("B", 0o010),
        ] {
            let path = dir.write(format!("teardown.d/{name}"), "");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        }
        std::fs::create_dir(stage.join("sub")).unwrap();
        std::os::unix::fs::symlink("missing", stage.join("dangling")).unwrap();
        dir
    };
    let mut parity = parity!("matches_the_legacy_glob_on_real_files");
    let legacy: Vec<String> = parity.legacy("teardown.d", || {
        let dir = stage_dir();
        // The loop of zfsbootmenu-run-hooks
        let output = Bash::new()
            .code(
                r#"list_hooks() {
                  for _hook in "$1"/*; do
                    [ -x "${_hook}" ] || continue; echo "${_hook##*/}"
                  done
                }"#,
            )
            .call(
                "list_hooks",
                &[&dir.join("teardown.d").to_string_lossy()],
                b"",
            );
        output.stdout_text().lines().map(str::to_owned).collect()
    });
    let dir = stage_dir();
    let rust: Vec<String> = executable_hooks(&RealFiles, dir.path(), HookStage::Teardown)
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(rust, legacy);
    assert_eq!(rust, ["10 space", "80-luks", "90-xhci", "B", "sub"]);
}
