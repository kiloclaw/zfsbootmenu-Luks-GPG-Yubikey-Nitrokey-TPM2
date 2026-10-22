//! The start-up stages, one [`Step`] each, in the order of [`Stage::ALL`]:
//! kernel modules, the host ID, waiting for devices, early hooks, LUKS
//! unlocking and pool import.

pub mod devices;
pub mod hooks;
pub mod hostid;
pub mod modules;

use std::path::Path;

use zbm_boot::import;
use zbm_boot::prompt::tput;
use zbm_pool::ImportSettings;
use zbm_types::Stage;

use crate::shell::{Attempt, zluks_unlock};
use crate::step::{Step, StepContext};

/// `init.d/10-kmods`: loads `spl` and `zfs`.
pub struct Modules;

impl Step for Modules {
    fn stage(&self) -> Stage {
        Stage::Modules
    }

    fn run(&self, context: &mut StepContext<'_, '_>) {
        modules::stage(context.system.runner, context.recovery);
    }
}

/// `init.d/20-hostid`: writes `/etc/hostid` from `spl_hostid`.
pub struct HostId<'o> {
    /// The host ID the command line gives (`spl_hostid`), or empty.
    pub spl_hostid: &'o str,
}

impl Step for HostId<'_> {
    fn stage(&self) -> Stage {
        Stage::HostId
    }

    fn run(&self, context: &mut StepContext<'_, '_>) {
        hostid::stage(context.system.files, self.spl_hostid);
    }
}

/// `init.d/30-device-wait`: waits for the devices of `zbm.wait_for`.
pub struct DeviceWait<'o> {
    /// `zbm.wait_for`: the devices, comma-separated.
    pub wait_for: &'o str,
    /// `zbm.retry_delay`: the seconds between checks.
    pub retry_delay: &'o str,
}

impl Step for DeviceWait<'_> {
    fn stage(&self) -> Stage {
        Stage::DeviceWait
    }

    fn run(&self, context: &mut StepContext<'_, '_>) {
        devices::stage(context.system, self.wait_for, self.retry_delay);
    }
}

/// `init.d/40-early-hooks`: imports `zbm.hookroot`, applies
/// `zbm.skip_hooks` and runs the `early-setup.d` hooks once.
pub struct EarlyHooks<'o> {
    /// The hook directories of the image.
    pub root: &'o Path,
    /// `zbm.hookroot`, or empty.
    pub hook_root: &'o str,
    /// `zbm.skip_hooks`, if given.
    pub skip: Option<String>,
}

impl Step for EarlyHooks<'_> {
    fn stage(&self) -> Stage {
        Stage::EarlyHooks
    }

    fn run(&self, context: &mut StepContext<'_, '_>) {
        hooks::early_stage_in(
            context.system,
            context.state,
            self.root,
            self.hook_root,
            self.skip.as_deref(),
        );
    }
}

/// `init.d/45-luks-unlock`: unlocks LUKS volumes with `zluks unlock`, when
/// the image has `cryptsetup`. A hook may have provided key files or unlocked
/// volumes itself.
pub struct LuksUnlock;

impl Step for LuksUnlock {
    fn stage(&self) -> Stage {
        Stage::LuksUnlock
    }

    fn run(&self, context: &mut StepContext<'_, '_>) {
        let system = context.system;
        if system.command_exists("cryptsetup") {
            let _ = zluks_unlock(system, Attempt::First);
            tput(system, &["clear"]);
        }
    }
}

/// `init.d/50-import-pools`: imports pools and finds the default boot
/// environment.
pub struct ImportPools {
    /// The settings of the import.
    pub settings: ImportSettings,
}

impl Step for ImportPools {
    fn stage(&self) -> Stage {
        Stage::ImportPools
    }

    fn run(&self, context: &mut StepContext<'_, '_>) {
        let imported = import::stage(
            context.system,
            context.state,
            &self.settings,
            context.recovery,
        );
        context.bootfs = imported.bootfs;
    }
}
