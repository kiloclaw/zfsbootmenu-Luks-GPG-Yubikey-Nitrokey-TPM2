//! `zbm run-hooks [-once] STAGE`: a hook stage, its hooks and its built-in
//! steps, for the legacy shell code of Rust images (ADR 0017, point 2).
//! Their `/libexec/zfsbootmenu-run-hooks` runs it, so that the legacy
//! `zkexec` of the recovery shell closes the LUKS mappings as a boot from
//! the menu does.
//!
//! As the legacy script, it reads `/etc/zfsbootmenu.conf`, takes the
//! stage's variables from its environment (`ZBM_SELECTED_BE` and the
//! others, which the legacy shell's boot sets), and succeeds when at least
//! one hook or step ran, which the legacy shell's key loading asks.

use std::path::Path;

use zbm_bootenv::hooks::{HOOKS, HookEnv, StageEnv, Variables, run_stage};
use zbm_bootenv::kernels::Kernel;
use zbm_config::conf::{self, Conf};
use zbm_platform::{Context, RealSystem};
use zbm_state::State;
use zbm_types::HookStage;

use crate::Io;

const USAGE: &str = crate::table::RUN_HOOKS.usage;

/// `zbm run-hooks`: runs the stage below `/libexec/hooks` on the real
/// system. The exit status is 0 when a hook or step ran, 1 when none did
/// or the configuration cannot be read, 2 for wrong usage.
pub fn run(args: &[String], io: &mut Io) -> i32 {
    let (once, stage) = match args {
        [flag, stage] if flag == "-once" => (true, stage),
        [stage] if !stage.starts_with('-') => (false, stage),
        _ => {
            let _ = io.stderr.write_all(USAGE.as_bytes());
            return crate::USAGE;
        }
    };
    let Ok(stage) = stage.parse::<HookStage>() else {
        let _ = writeln!(io.stderr, "run-hooks: {stage} is not a hook stage");
        return crate::USAGE;
    };
    let real = RealSystem::new();
    match run_hooks(&real.context(), Path::new("/"), stage, once) {
        Some(0) => 1,
        Some(_) => 0,
        None => {
            let _ = writeln!(io.stderr, "ERROR: unable to source {}", conf::PATH);
            1
        }
    }
}

/// Runs the hooks of `stage` and its built-in steps, those start-up did
/// not leave out, in the image below `root`: how many ran, or `None` when
/// the image's configuration cannot be read.
fn run_hooks(context: &Context<'_>, root: &Path, stage: HookStage, once: bool) -> Option<usize> {
    let path = root.join(conf::PATH.trim_start_matches('/'));
    let conf = Conf::parse(&context.files().read_to_string(&path).ok()?);
    let base = zbm_startup::preinit::base(&conf);
    let _log = context.kernel_log(conf.get("loglevel").unwrap_or_default(), &base);
    let records = State::new(context.files(), &base);
    let variables = Variables::from(conf.vars().to_vec());
    let from_env = |name: &str| context.environment().var(name).unwrap_or_default();
    let (fs, encryption_root, mountpoint) = (
        from_env("ZBM_LOCKED_FS"),
        from_env("ZBM_ENCRYPTION_ROOT"),
        from_env("ZBM_SELECTED_MOUNTPOINT"),
    );
    let selected = Kernel::new(
        &from_env("ZBM_SELECTED_BE"),
        &from_env("ZBM_SELECTED_KERNEL"),
        &from_env("ZBM_SELECTED_INITRAMFS"),
    );
    let stage = match stage {
        HookStage::EarlySetup => StageEnv::EarlySetup,
        HookStage::Setup => StageEnv::Setup,
        HookStage::LoadKey => StageEnv::LoadKey {
            fs: &fs,
            root: &encryption_root,
        },
        HookStage::BootSel => StageEnv::BootSel {
            selected: &selected,
            mountpoint: &mountpoint,
        },
        // The legacy shell loaded the kernel: what it loaded is not known.
        HookStage::Teardown => StageEnv::Teardown {
            selected: &selected,
            loaded: None,
        },
    };
    let env = HookEnv {
        variables: &variables,
        stage,
    };
    let steps = zbm_app::steps::built_in(&records);
    let ran = run_stage(
        context,
        &root.join(HOOKS.trim_start_matches('/')),
        &env,
        &steps,
        once,
    );
    if once {
        zbm_app::steps::record_ran(&records, &steps);
    }
    Some(ran)
}

#[cfg(test)]
mod tests {
    use zbm_testkit::fakes::FakeSystem;

    use super::*;

    /// An image with its configuration and a teardown hook.
    fn image() -> FakeSystem {
        let fake = FakeSystem::new();
        fake.files
            .file(
                "/etc/zfsbootmenu.conf",
                "export BASE=\"/zfsbootmenu\"\nexport control_term=\"/dev/tty1\"\n",
            )
            .dir("/zfsbootmenu")
            .file_with_mode("/libexec/hooks/teardown.d/10-down", "#!/bin/sh\n", 0o755)
            .file_with_mode("/usr/bin/cryptsetup", "", 0o755);
        fake.environment
            .set("PATH", "/usr/bin")
            .set("ZBM_SELECTED_BE", "tank/ROOT/void");
        fake
    }

    #[test]
    fn the_teardown_closes_luks_among_the_hooks() {
        let fake = image();
        let ran = run_hooks(&fake.context(), Path::new("/"), HookStage::Teardown, false);
        // The hook, the LUKS close and the USB unbind.
        assert_eq!(ran, Some(3));
        assert_eq!(
            fake.runner.lines(),
            ["/libexec/hooks/teardown.d/10-down", "/bin/zluks close"]
        );
        let calls = fake.runner.calls();
        let env = calls[0].get_env();
        assert!(
            env.iter().any(|(name, value)| name == "ZBM_SELECTED_BE"
                && value.as_deref() == Some("tank/ROOT/void".as_ref())),
            "{env:?}"
        );
        assert!(
            env.iter().any(|(name, _)| name == "control_term"),
            "{env:?}"
        );
    }

    #[test]
    fn a_stage_without_hooks_or_steps_runs_nothing() {
        let fake = image();
        assert_eq!(
            run_hooks(&fake.context(), Path::new("/"), HookStage::LoadKey, false),
            Some(0)
        );
        assert!(fake.runner.lines().is_empty());
        // Without the configuration, nothing runs.
        let bare = FakeSystem::new();
        assert_eq!(
            run_hooks(&bare.context(), Path::new("/"), HookStage::Teardown, false),
            None
        );
    }

    #[test]
    fn early_steps_run_once() {
        let fake = image();
        let context = fake.context();
        let first = run_hooks(&context, Path::new("/"), HookStage::EarlySetup, true);
        assert!(first.is_some_and(|ran| ran > 0), "{first:?}");
        assert_eq!(
            run_hooks(&context, Path::new("/"), HookStage::EarlySetup, true),
            Some(0)
        );
    }

    #[test]
    fn wrong_usage_is_said() {
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let mut io = Io {
            stdout: &mut stdout,
            stderr: &mut stderr,
        };
        for args in [
            &[][..],
            &["-x".to_owned()],
            &["a".to_owned(), "b".to_owned()],
        ] {
            assert_eq!(run(args, &mut io), crate::USAGE);
        }
        assert_eq!(run(&["teardown".to_owned()], &mut io), crate::USAGE);
        let stderr = String::from_utf8(stderr).unwrap();
        assert!(stderr.contains("Usage: zbm run-hooks"), "{stderr}");
        assert!(stderr.contains("teardown is not a hook stage"), "{stderr}");
    }
}
