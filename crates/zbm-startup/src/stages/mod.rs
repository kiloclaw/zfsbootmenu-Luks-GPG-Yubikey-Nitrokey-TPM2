//! The start-up stages, one [`Step`] each, in the order of [`Stage::ALL`]:
//! kernel modules, the host ID, waiting for devices, early hooks, LUKS
//! unlocking and pool import.

pub mod devices;
pub mod hooks;
pub mod hostid;
pub mod modules;

use std::path::Path;

use zbm_boot::hooks::executable_hooks;
use zbm_platform::System;
use zbm_pool::ImportSettings;
use zbm_types::{HookStage, RetryDelay, Stage};

use crate::error::StartupError;
use crate::step::{Step, StepContext, StepOutcome};

/// `init.d/10-kmods`: loads `spl` and `zfs`.
pub struct Modules;

impl Step for Modules {
    fn stage(&self) -> Stage {
        Stage::Modules
    }

    fn run(&self, context: &mut StepContext<'_, '_>) -> Result<StepOutcome, StartupError> {
        modules::stage(context.system.runner, context.front.recovery())?;
        Ok(StepOutcome::Done)
    }
}

/// `init.d/20-hostid`: writes `/etc/hostid` from `spl_hostid`.
pub struct HostId<'o> {
    /// The host ID the command line gives (`spl_hostid`), if any.
    pub spl_hostid: Option<&'o zbm_types::HostId>,
}

impl Step for HostId<'_> {
    fn stage(&self) -> Stage {
        Stage::HostId
    }

    fn run(&self, context: &mut StepContext<'_, '_>) -> Result<StepOutcome, StartupError> {
        hostid::stage(context.system.files, self.spl_hostid);
        Ok(StepOutcome::Done)
    }
}

/// `init.d/30-device-wait`: waits for the devices of `zbm.wait_for`.
pub struct DeviceWait<'o> {
    /// `zbm.wait_for`: the devices, comma-separated.
    pub wait_for: &'o str,
    /// `zbm.retry_delay`: the seconds between checks.
    pub retry_delay: RetryDelay,
}

impl Step for DeviceWait<'_> {
    fn stage(&self) -> Stage {
        Stage::DeviceWait
    }

    fn run(&self, context: &mut StepContext<'_, '_>) -> Result<StepOutcome, StartupError> {
        let (system, front) = (context.system, &mut *context.front);
        devices::stage_with(system, self.wait_for, self.retry_delay, &mut |delay| {
            front.devices_missing(system, delay)
        });
        Ok(StepOutcome::Done)
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

    /// The hooks run with the console, which a front end that draws on it
    /// hands over only when there are hooks to run.
    fn run(&self, context: &mut StepContext<'_, '_>) -> Result<StepOutcome, StartupError> {
        let system = context.system;
        // The image's own hooks run even when those of `zbm.hookroot`
        // could not be imported; the stage then fails after them.
        let prepared = hooks::prepare_early_stage(
            system,
            context.state,
            self.root,
            self.hook_root,
            self.skip.as_deref(),
        );
        let mut run = |system: &System<'_>| hooks::run_early_stage(system, self.root);
        if executable_hooks(system.files, self.root, HookStage::EarlySetup).is_empty() {
            run(system);
        } else {
            context.front.with_console(system, &mut run);
        }
        prepared.map(|()| StepOutcome::Done)
    }
}

/// `init.d/45-luks-unlock`: unlocks LUKS volumes through the front end
/// ([`crate::front::Front::unlock_luks`]).
pub struct LuksUnlock;

impl Step for LuksUnlock {
    fn stage(&self) -> Stage {
        Stage::LuksUnlock
    }

    fn run(&self, context: &mut StepContext<'_, '_>) -> Result<StepOutcome, StartupError> {
        context.front.unlock_luks(context.system);
        Ok(StepOutcome::Done)
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

    fn run(&self, context: &mut StepContext<'_, '_>) -> Result<StepOutcome, StartupError> {
        let imported = context
            .front
            .import(context.system, context.state, &self.settings);
        context.bootfs = imported.bootfs;
        Ok(StepOutcome::Done)
    }
}
