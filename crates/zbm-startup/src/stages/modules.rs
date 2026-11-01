//! The kernel modules: the start-up stage `init.d/10-kmods`.

use zbm_platform::System;
use zbm_platform::error::{Error, Result};
use zbm_tools::modules::Modules;
use zbm_tools::{Shown, udev};

use zbm_boot::recovery::Recovery;

use crate::error::StartupError;

/// Loads SPL and ZFS as `init.d/10-kmods` does, then waits for udev. When
/// SPL does not load, its file is inserted alone with `spl_hostid=0`, so
/// that `/etc/hostid` counts; a failure there is only a warning. When ZFS
/// cannot be loaded, the recovery shell opens, and the stage carries on after
/// it, as the legacy stage does. The modules load without `modprobe` where
/// the runtime can (ROADMAP.md CD4).
///
/// # Errors
///
/// [`StartupError::ZfsModules`], after the shell and udev, when ZFS could
/// not be loaded.
pub fn stage(
    system: &System<'_>,
    recovery: &dyn Recovery,
) -> std::result::Result<(), StartupError> {
    let loaded = load(&Modules::of(system));
    if let Err(error) = &loaded {
        recovery.emergency_shell(&error.to_string());
    }
    udev::settle(system.runner, Shown::Console);
    loaded.map_err(|source| StartupError::ZfsModules { source })
}

/// Loads the modules; an error means ZFS could not be loaded.
fn load(modules: &Modules<'_>) -> Result<()> {
    if let Err(output) = modules.load("spl") {
        zbm_trace::debug!("{output}"; %output);
        zbm_trace::info!("loading spl alone, with spl_hostid=0");
        if let Err(output) = modules.insert_alone("spl", &["spl_hostid=0"]) {
            zbm_trace::warn!("{output}"; %output);
            zbm_trace::warn!("unable to load SPL kernel module; attempting to load ZFS anyway");
        }
    }
    if let Err(output) = modules.load("zfs") {
        zbm_trace::error!("{output}"; %output);
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
    struct Shells(std::sync::Mutex<Vec<String>>);

    impl Recovery for Shells {
        fn unlock_late(&self) -> std::result::Result<(), zbm_boot::error::BootError> {
            Err(zbm_boot::error::BootError::NoCryptsetup)
        }

        fn emergency_shell(&self, message: &str) {
            self.0.lock().unwrap().push(message.to_owned());
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
        assert!(stage(&fake.system(), &Shells::default()).is_ok());
        assert_eq!(
            fake.runner.lines(),
            ["modprobe spl", "modprobe zfs", "udevadm settle"]
        );
        assert!(fake.runner.calls()[0].merges_stderr());
        assert_eq!(fake.runner.calls()[2].get_stdout(), Stream::Inherit);
    }

    #[test]
    fn with_the_kernel_s_index_the_runtime_loads_them_itself() {
        let fake = FakeSystem::new();
        let _log = log(&fake);
        fake.files.file("/proc/sys/kernel/osrelease", "6.12.8\n");
        fake.files.file(
            "/lib/modules/6.12.8/modules.dep",
            "extra/zfs.ko: extra/spl.ko\nextra/spl.ko:\n",
        );
        assert!(stage(&fake.system(), &Shells::default()).is_ok());
        let inserted: Vec<String> = fake
            .modules
            .inserted()
            .iter()
            .map(|(path, _)| path.display().to_string())
            .collect();
        assert_eq!(
            inserted,
            [
                "/lib/modules/6.12.8/extra/spl.ko",
                "/lib/modules/6.12.8/extra/zfs.ko"
            ]
        );
        assert_eq!(fake.runner.lines(), ["udevadm settle"]);
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
        assert!(stage(&fake.system(), &Shells::default()).is_ok());
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
        assert!(kmsg.contains("<6>ZFSBootMenu: loading spl alone, with spl_hostid=0"));
        assert!(kmsg.contains("<4>ZFSBootMenu: insmod: File exists"));
        assert!(kmsg.contains("unable to load SPL kernel module; attempting to load ZFS anyway"));

        // Without a file name, ZFS is loaded at once.
        let fake = FakeSystem::new();
        fake.runner.respond("modprobe spl", 1, "");
        assert!(stage(&fake.system(), &Shells::default()).is_ok());
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
        let shells = Shells::default();
        let error = stage(&fake.system(), &shells).unwrap_err();
        assert_eq!(
            *shells.0.lock().unwrap(),
            ["unable to load ZFS kernel modules"]
        );
        assert_eq!(
            zbm_types::code::report(&error),
            "ZBM-E0704: the kernel modules stage failed: unable to load ZFS kernel modules"
        );
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
