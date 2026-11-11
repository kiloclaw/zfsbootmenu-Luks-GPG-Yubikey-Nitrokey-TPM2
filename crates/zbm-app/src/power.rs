//! Rebooting, powering off and rebooting into the firmware setup, as the
//! recovery shell's `reboot`, `poweroff` and `firmware-setup` and the
//! menu's power actions do (ADR 0018, point 11): the pools `zpool list`
//! lists that are imported read/write are exported first, as the legacy
//! `reboot` exported them.

use zbm_platform::System;
use zbm_zfs::Zfs as _;

use crate::boot;
use crate::model::Event;

/// How the machine resets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Reset {
    /// It reboots.
    Reboot,
    /// It powers off.
    PowerOff,
    /// It reboots into the firmware's setup, which only a firmware that
    /// says it can open it does.
    FirmwareSetup,
}

/// Resets the machine of `system` as `reset` says, after exporting the
/// imported pools that are writable; returns only when that failed, with
/// the failure as the last event.
pub fn reset(system: &System<'_>, reset: Reset) -> Vec<Event> {
    let pools = zbm_zfs::ZfsCli::new(system.runner)
        .pool_names()
        .unwrap_or_default();
    match reset {
        Reset::Reboot => boot::reset(system, &pools, "b"),
        Reset::PowerOff => boot::reset(system, &pools, "o"),
        Reset::FirmwareSetup => boot::firmware_setup(system, &pools),
    }
}

#[cfg(test)]
mod tests {
    use zbm_testkit::fakes::FakeSystem;

    use super::*;
    use crate::model::Outcome;

    #[test]
    fn power_lists_the_imported_pools_as_zpool_list_does() {
        let fake = FakeSystem::new();
        fake.files.dir("/proc");
        fake.runner
            .respond("zpool list -H -o name", 0, "tank\nrpool\n");
        fake.runner
            .respond("zpool get -H -o value readonly tank", 0, "off\n");
        fake.runner
            .respond("zpool get -H -o value readonly rpool", 0, "on\n");

        let events = reset(&fake.system(), Reset::PowerOff);

        let ran = fake.runner.lines();
        assert!(ran.contains(&"zpool list -H -o name".to_owned()), "{ran:?}");
        assert!(ran.contains(&"zpool export tank".to_owned()), "{ran:?}");
        assert!(!ran.contains(&"zpool export rpool".to_owned()), "{ran:?}");
        assert_eq!(fake.files.text("/proc/sysrq-trigger").as_deref(), Some("o"));
        assert!(matches!(
            events.last(),
            Some(Event::Finished(Outcome::Failed { .. }))
        ));
    }

    #[test]
    fn a_reboot_triggers_b_and_the_firmware_setup_needs_the_firmware() {
        let fake = FakeSystem::new();
        fake.files.dir("/proc");
        reset(&fake.system(), Reset::Reboot);
        assert_eq!(fake.files.text("/proc/sysrq-trigger").as_deref(), Some("b"));

        let bios = FakeSystem::new();
        bios.files.dir("/proc");
        let events = reset(&bios.system(), Reset::FirmwareSetup);
        assert!(bios.files.text("/proc/sysrq-trigger").is_none());
        assert!(matches!(
            events.last(),
            Some(Event::Finished(Outcome::Failed { what, .. }))
                if what.contains("firmware setup")
        ));
    }
}
