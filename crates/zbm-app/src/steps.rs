//! The built-in steps of the hook stages, which replace the hooks the image
//! installed (ADR 0017, points 1 to 5): each keeps its hook's file name and
//! runs where the hook ran, among the stage's hooks. The table is here
//! because both boots, the menu's and the countdown's, reach `zbm-app`,
//! and the LUKS close needs what `zbm-boot` may not use.
//!
//! `zbm.skip_hooks` skips a step by its name, with or without `.sh`; what
//! it skipped, and which early-setup steps ran, are recorded in the state
//! directory ([`State::step_records`]), so that every process that runs a
//! stage, start-up's and the menu's, builds the same table from them
//! ([`built_in`]).

use std::path::Path;

use zbm_bootenv::BootEnvError;
use zbm_bootenv::hooks::{BootStep, BootSteps, HookEnv};
use zbm_platform::Context;
use zbm_platform::files::FileKind;
use zbm_state::State;
use zbm_tools::modules::Modules;
use zbm_tools::programs;
use zbm_types::HookStage;

/// The record of a step `zbm.skip_hooks` left out.
const SKIPPED: &str = "skipped ";

/// The record of an early-setup step that ran.
const RAN: &str = "ran ";

/// The built-in steps, in the order of their names.
static TABLE: [&dyn BootStep; 6] = [
    &ScreenReaders,
    &EfiInit,
    &DracutConsoleInit,
    &ConsoleAutosize,
    &LuksClose,
    &XhciUnbind,
];

/// The steps a boot runs: every built-in step but those `zbm.skip_hooks`
/// left out and the early-setup steps that ran, as `state` records them.
pub fn built_in(state: &State<'_>) -> BootSteps {
    let records = state.step_records();
    let recorded = |prefix: &str, name: &str| {
        records
            .iter()
            .any(|record| record.strip_prefix(prefix) == Some(name))
    };
    BootSteps::from(TABLE.to_vec()).retain(|step| {
        let ran = step.stage() == HookStage::EarlySetup && recorded(RAN, step.name());
        !ran && !recorded(SKIPPED, step.name())
    })
}

/// Whether `wanted`, an entry of `zbm.skip_hooks`, names the step `name`:
/// its file name, or that name without `.sh`.
fn names(wanted: &str, name: &str) -> bool {
    wanted == name || name.strip_suffix(".sh") == Some(wanted)
}

/// Records the steps the comma-separated `skip` list of `zbm.skip_hooks`
/// names as skipped, but those kept when `guarded`, a locked image or one
/// under lockdown or Secure Boot, which run anyway: the names of those,
/// for the caller to warn about.
pub fn skip(state: &State<'_>, skip: &str, guarded: bool) -> Vec<&'static str> {
    let mut kept = Vec::new();
    for wanted in skip.split(',').filter(|name| !name.is_empty()) {
        for step in TABLE.iter().filter(|step| names(wanted, step.name())) {
            let name = step.name();
            if guarded && step.is_kept_when_guarded() {
                kept.push(name);
                continue;
            }
            tracing::info!(step = name, "Disabling built-in step");
            if let Err(error) = state.add_step_record(&format!("{SKIPPED}{name}")) {
                tracing::warn!(%error, "unable to record a skipped step");
            }
        }
    }
    kept
}

/// The steps that may not be replaced when guarded, by their hook names
/// below their stages (`teardown.d/80-luks-close.sh`).
pub fn kept() -> Vec<String> {
    TABLE
        .iter()
        .filter(|step| step.is_kept_when_guarded())
        .map(|step| format!("{}/{}", step.stage(), step.name()))
        .collect()
}

/// Records each early-setup step of `steps` as run, so that start-up does
/// not run it again.
pub fn record_ran(state: &State<'_>, steps: &BootSteps) {
    for step in steps
        .iter()
        .filter(|step| step.stage() == HookStage::EarlySetup)
    {
        if let Err(error) = state.add_step_record(&format!("{RAN}{}", step.name())) {
            tracing::warn!(%error, "unable to record a step that ran");
        }
    }
}

/// The value of the variable `name` of `env`.
fn var<'e>(env: &'e HookEnv<'_>, name: &str) -> Option<&'e str> {
    env.variables
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value)
}

/// A step's error.
fn failed(step: &dyn BootStep, why: impl std::fmt::Display) -> BootEnvError {
    BootEnvError::Step {
        step: step.name().to_owned(),
        why: why.to_string(),
    }
}

/// `05-screen-readers.sh`: starts the screen readers of the image (ADR
/// 0011), as its build options ask: `brltty` (`ZBM_BRLTTY`), the `speakup`
/// modules of a synthesiser (`ZBM_SPEAKUP`), and `espeakup` for the soft one
/// (`ZBM_ESPEAKUP`).
#[derive(Debug)]
struct ScreenReaders;

impl BootStep for ScreenReaders {
    fn name(&self) -> &'static str {
        "05-screen-readers.sh"
    }

    fn stage(&self) -> HookStage {
        HookStage::EarlySetup
    }

    fn run(&self, context: &Context<'_>, env: &HookEnv<'_>) -> Result<(), BootEnvError> {
        if var(env, "ZBM_BRLTTY").is_some_and(|value| !value.is_empty()) {
            programs::run_quietly(context.runner(), "brltty", &["-q"]);
        }
        if let Some(synth) = var(env, "ZBM_SPEAKUP").filter(|synth| !synth.is_empty()) {
            Modules::new(context.runner(), context.files(), context.modules())
                .probe_quietly(&format!("speakup_{synth}"));
            if var(env, "ZBM_ESPEAKUP").is_some_and(|value| !value.is_empty()) {
                programs::run_quietly(context.runner(), "espeakup", &[]);
            }
        }
        Ok(())
    }
}

/// Where the escape sequences of the console go: the control terminal of
/// the configuration, or the kernel's console.
fn console_of<'e>(env: &'e HookEnv<'_>) -> &'e str {
    var(env, "control_term")
        .filter(|term| !term.is_empty())
        .unwrap_or("/dev/console")
}

/// `10-generic-efi-init.sh`: prints a black ` ... ` on the console, which
/// makes the EFI framebuffer find its size.
#[derive(Debug)]
struct EfiInit;

impl BootStep for EfiInit {
    fn name(&self) -> &'static str {
        "10-generic-efi-init.sh"
    }

    fn stage(&self) -> HookStage {
        HookStage::EarlySetup
    }

    fn run(&self, context: &Context<'_>, env: &HookEnv<'_>) -> Result<(), BootEnvError> {
        let console = console_of(env);
        context
            .files()
            .write(Path::new(console), b"\x1b[0;30m ... \x1b[0m\n")
            .map_err(|error| failed(self, format!("{console}: {error}")))
    }
}

/// dracut's console set-up, which release builds (`rd.hostonly=0`) skip.
const CONSOLE_INIT: &str = "/lib/udev/console_init";

/// `20-dracut-console-init.sh`: runs dracut's console set-up on the
/// control terminal, in images built by dracut.
#[derive(Debug)]
struct DracutConsoleInit;

impl BootStep for DracutConsoleInit {
    fn name(&self) -> &'static str {
        "20-dracut-console-init.sh"
    }

    fn stage(&self) -> HookStage {
        HookStage::EarlySetup
    }

    fn run(&self, context: &Context<'_>, env: &HookEnv<'_>) -> Result<(), BootEnvError> {
        let dracut =
            var(env, "ZBM_BUILDSTYLE").is_some_and(|style| style.eq_ignore_ascii_case("dracut"));
        let files = context.files();
        let runnable = files
            .metadata(Path::new(CONSOLE_INIT))
            .is_ok_and(|metadata| metadata.is_executable());
        let Some(term) = var(env, "control_term").filter(|_| dracut && runnable) else {
            return Ok(());
        };
        let is_terminal = files
            .metadata(Path::new(term))
            .is_ok_and(|metadata| metadata.kind == FileKind::CharDevice);
        if is_terminal {
            let name = term.rsplit('/').next().unwrap_or(term);
            programs::run_quietly(context.runner(), CONSOLE_INIT, &[name]);
        }
        Ok(())
    }
}

/// `30-console-autosize.sh`: sizes the console font to the display
/// (`zbm.autosize`), with the applet that holds the console's fonts.
#[derive(Debug)]
struct ConsoleAutosize;

impl BootStep for ConsoleAutosize {
    fn name(&self) -> &'static str {
        "30-console-autosize.sh"
    }

    fn stage(&self) -> HookStage {
        HookStage::EarlySetup
    }

    fn run(&self, context: &Context<'_>, _env: &HookEnv<'_>) -> Result<(), BootEnvError> {
        programs::applet(context.runner(), programs::ZBM, &["console-autosize"], &[])
            .map(drop)
            .map_err(|error| failed(self, error))
    }
}

/// `80-luks-close.sh`: closes the LUKS mappings ZFSBootMenu opened, the
/// pools on them exported first, so that the kernel wipes their keys
/// before the new kernel runs; with `zluks close`, which reads the
/// configuration of the LUKS volumes.
#[derive(Debug)]
struct LuksClose;

impl BootStep for LuksClose {
    fn name(&self) -> &'static str {
        "80-luks-close.sh"
    }

    fn stage(&self) -> HookStage {
        HookStage::Teardown
    }

    fn is_kept_when_guarded(&self) -> bool {
        true
    }

    fn run(&self, context: &Context<'_>, _env: &HookEnv<'_>) -> Result<(), BootEnvError> {
        let closed = programs::applet(context.runner(), "/bin/zluks", &["close"], &[])
            .map_err(|error| failed(self, error))?;
        if closed.status.is_success() {
            Ok(())
        } else {
            Err(failed(self, "zluks close left mappings open"))
        }
    }
}

/// The USB controllers' driver.
const XHCI: &str = "/sys/bus/pci/drivers/xhci_hcd";

/// Whether `name` is a PCI address, as `????:??:??.?` matches it.
fn is_pci_address(name: &str) -> bool {
    let at = |index: usize| name.as_bytes().get(index).copied();
    name.len() == 12 && at(4) == Some(b':') && at(7) == Some(b':') && at(10) == Some(b'.')
}

/// `90-xhci-unbind.sh`: unbinds every USB controller from its driver, so
/// that the new kernel finds them as the firmware left them.
#[derive(Debug)]
struct XhciUnbind;

impl BootStep for XhciUnbind {
    fn name(&self) -> &'static str {
        "90-xhci-unbind.sh"
    }

    fn stage(&self) -> HookStage {
        HookStage::Teardown
    }

    fn run(&self, context: &Context<'_>, _env: &HookEnv<'_>) -> Result<(), BootEnvError> {
        let files = context.files();
        let driver = Path::new(XHCI);
        let mut devices: Vec<String> = files
            .read_dir(driver)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|name| name.into_string().ok())
            .filter(|name| is_pci_address(name))
            .filter(|name| files.read_link(&driver.join(name)).is_ok())
            .collect();
        devices.sort();
        for device in devices {
            tracing::info!(%device, "Tearing down a USB controller");
            files
                .write(&driver.join("unbind"), device.as_bytes())
                .map_err(|error| failed(self, format!("{device}: {error}")))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
