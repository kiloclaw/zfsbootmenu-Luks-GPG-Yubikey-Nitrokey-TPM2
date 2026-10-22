//! The kernel modules: the start-up stage `init.d/10-kmods`.

use zbm_core::shell::command_substitution;
use zbm_platform::Runner;
use zbm_platform::error::{Error, Result};
use zbm_tools::modules::Modules;
use zbm_tools::{Shown, udev};

use crate::recovery::Recovery;

/// What `$( <command> 2>&1 )` gives: whether the command succeeded, and its
/// output without trailing newlines. A command that cannot start counts as a
/// failure with the error as its output.
fn merged(run: std::io::Result<zbm_platform::Output>) -> (bool, String) {
    match run {
        Ok(output) => (
            output.status.is_success(),
            command_substitution(&output.stdout_text()).to_owned(),
        ),
        Err(error) => (false, error.to_string()),
    }
}

/// Loads SPL and ZFS as `init.d/10-kmods` does, then waits for udev. When
/// `modprobe spl` fails, SPL is loaded from its file with `spl_hostid=0`, so
/// that `/etc/hostid` counts; a failure there is only a warning. When ZFS
/// cannot be loaded, the recovery shell opens, and the stage carries on after
/// it, as the legacy stage does.
pub fn stage(runner: &dyn Runner, recovery: &mut dyn Recovery) {
    if let Err(error) = load(runner) {
        recovery.emergency_shell(&error.to_string());
    }
    udev::settle(runner, Shown::Console);
}

/// Loads the modules; an error means ZFS could not be loaded.
fn load(runner: &dyn Runner) -> Result<()> {
    let modules = Modules::new(runner);
    let (loaded, output) = merged(modules.load("spl"));
    if !loaded {
        zbm_trace::debug!("{}", output);
        let filename = modules
            .filename("spl")
            .map(|output| command_substitution(&output.stdout_text()).to_owned())
            .unwrap_or_default();
        if !filename.is_empty() {
            zbm_trace::info!("loading {filename}");
            let (loaded, output) = merged(modules.insert(&filename, &["spl_hostid=0"]));
            if !loaded {
                zbm_trace::warn!("{}", output);
                zbm_trace::warn!("unable to load SPL kernel module; attempting to load ZFS anyway");
            }
        }
    }

    let (loaded, output) = merged(modules.load("zfs"));
    if !loaded {
        zbm_trace::error!("{}", output);
        return Err(Error::invalid("unable to load ZFS kernel modules"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use zbm_platform::exec::Stream;
    use zbm_platform::{Installed, KernelLog};
    use zbm_testkit::fakes::FakeSystem;

    /// Records the recovery shells.
    #[derive(Default)]
    struct Shells(Vec<String>);

    impl Recovery for Shells {
        fn unlock_late(&mut self) -> std::result::Result<(), crate::error::BootError> {
            Err(crate::error::BootError::NoCryptsetup)
        }

        fn emergency_shell(&mut self, message: &str) {
            self.0.push(message.to_owned());
        }
    }

    fn log(fake: &FakeSystem) -> Installed {
        fake.files.dir("/zfsbootmenu");
        KernelLog::new(&fake.files, "7", Path::new("/zfsbootmenu"), 1).install()
    }

    #[test]
    fn loads_spl_and_zfs() {
        let fake = FakeSystem::new();
        let _log = log(&fake);
        stage(&fake.runner, &mut Shells::default());
        assert_eq!(
            fake.runner.lines(),
            ["modprobe spl", "modprobe zfs", "udevadm settle"]
        );
        assert!(fake.runner.calls()[0].merges_stderr());
        assert_eq!(fake.runner.calls()[2].get_stdout(), Stream::Inherit);
    }

    #[test]
    fn falls_back_to_insmod_with_a_zero_host_id() {
        let fake = FakeSystem::new();
        let _log = log(&fake);
        fake.runner
            .respond("modprobe spl", 1, "modprobe: FATAL: bad parameter\n");
        fake.runner.respond(
            "modinfo -F filename spl",
            0,
            "/lib/modules/6.1/extra/spl.ko\n",
        );
        fake.runner.respond(
            "insmod /lib/modules/6.1/extra/spl.ko spl_hostid=0",
            1,
            "insmod: File exists\n",
        );
        stage(&fake.runner, &mut Shells::default());
        assert_eq!(
            fake.runner.lines(),
            [
                "modprobe spl",
                "modinfo -F filename spl",
                "insmod /lib/modules/6.1/extra/spl.ko spl_hostid=0",
                "modprobe zfs",
                "udevadm settle",
            ]
        );
        let kmsg = fake.kmsg();
        assert!(kmsg.contains("modprobe: FATAL: bad parameter"));
        assert!(kmsg.contains("<6>ZFSBootMenu: loading /lib/modules/6.1/extra/spl.ko"));
        assert!(kmsg.contains("<4>ZFSBootMenu: insmod: File exists"));
        assert!(kmsg.contains("unable to load SPL kernel module; attempting to load ZFS anyway"));

        // Without a file name, ZFS is loaded at once.
        let fake = FakeSystem::new();
        fake.runner.respond("modprobe spl", 1, "");
        stage(&fake.runner, &mut Shells::default());
        assert_eq!(
            fake.runner.lines(),
            [
                "modprobe spl",
                "modinfo -F filename spl",
                "modprobe zfs",
                "udevadm settle",
            ]
        );
    }

    #[test]
    fn a_missing_zfs_module_opens_the_recovery_shell() {
        let fake = FakeSystem::new();
        let _log = log(&fake);
        fake.runner
            .respond("modprobe zfs", 1, "modprobe: FATAL: Module zfs not found\n");
        let mut shells = Shells::default();
        stage(&fake.runner, &mut shells);
        assert_eq!(shells.0, ["unable to load ZFS kernel modules"]);
        assert!(
            fake.kmsg()
                .contains("<3>ZFSBootMenu: modprobe: FATAL: Module zfs not found")
        );
        // The stage carries on after the recovery shell.
        assert_eq!(
            fake.runner.lines(),
            ["modprobe spl", "modprobe zfs", "udevadm settle"]
        );
    }
}
