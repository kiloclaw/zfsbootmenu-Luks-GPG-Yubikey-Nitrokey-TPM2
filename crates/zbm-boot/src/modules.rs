//! The kernel modules: the start-up stage `init.d/10-kmods`.

use zbm_core::error::{Error, Result};
use zbm_core::exec::{Command, Stream};
use zbm_core::shell::command_substitution;
use zbm_system::{KernelLog, Runner};

use crate::recovery::Recovery;

/// What `$( <command> 2>&1 )` gives: whether the command succeeded, and its
/// output without trailing newlines. A command that cannot start counts as a
/// failure with the error as its output.
fn run_merged(runner: &dyn Runner, command: Command) -> (bool, String) {
    match runner.run(&command.merge_stderr()) {
        Ok(output) => (
            output.status.success(),
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
pub fn stage(runner: &dyn Runner, log: &KernelLog<'_>, recovery: &mut dyn Recovery) {
    if let Err(error) = load(runner, log) {
        recovery.emergency_shell(&error.to_string());
    }
    let settle = Command::new("udevadm")
        .arg("settle")
        .stdout(Stream::Inherit);
    let _ = runner.run(&settle);
}

/// Loads the modules; an error means ZFS could not be loaded.
fn load(runner: &dyn Runner, log: &KernelLog<'_>) -> Result<()> {
    let (loaded, output) = run_merged(runner, Command::new("modprobe").arg("spl"));
    if !loaded {
        log.debug(output);
        let filename = Command::new("modinfo").args(["-F", "filename", "spl"]);
        let filename = runner
            .run(&filename)
            .map(|output| command_substitution(&output.stdout_text()).to_owned())
            .unwrap_or_default();
        if !filename.is_empty() {
            log.info(format!("loading {filename}"));
            let insmod = Command::new("insmod").arg(&filename).arg("spl_hostid=0");
            let (loaded, output) = run_merged(runner, insmod);
            if !loaded {
                log.warning(output);
                log.warning("unable to load SPL kernel module; attempting to load ZFS anyway");
            }
        }
    }

    let (loaded, output) = run_merged(runner, Command::new("modprobe").arg("zfs"));
    if !loaded {
        log.error(output);
        return Err(Error::invalid("unable to load ZFS kernel modules"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use zbm_testkit::fakes::FakeSystem;

    /// Records the recovery shells.
    #[derive(Default)]
    struct Shells(Vec<String>);

    impl Recovery for Shells {
        fn unlock_late(&mut self) -> bool {
            false
        }

        fn emergency_shell(&mut self, message: &str) {
            self.0.push(message.to_owned());
        }
    }

    fn log(fake: &FakeSystem) -> KernelLog<'_> {
        fake.files.dir("/zfsbootmenu");
        KernelLog::new(&fake.files, "7", Path::new("/zfsbootmenu"), 1)
    }

    #[test]
    fn loads_spl_and_zfs() {
        let fake = FakeSystem::new();
        stage(&fake.runner, &log(&fake), &mut Shells::default());
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
        stage(&fake.runner, &log(&fake), &mut Shells::default());
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
        stage(&fake.runner, &log(&fake), &mut Shells::default());
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
        fake.runner
            .respond("modprobe zfs", 1, "modprobe: FATAL: Module zfs not found\n");
        let mut shells = Shells::default();
        stage(&fake.runner, &log(&fake), &mut shells);
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
