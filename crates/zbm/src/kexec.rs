//! `zkexec FILESYSTEM KERNEL INITRAMFS`: boots `/boot/KERNEL` and
//! `/boot/INITRAMFS` of a boot environment, as the legacy `zkexec` meant
//! to, through the boot of the menu (ADR 0018, point 9): the key loaded
//! first, the menu's settings from the image's configuration, its hooks and
//! the built-in steps ([`Startup::kexec_files`]). It runs in the recovery
//! shell, so a failed boot opens no other: it says why on standard error
//! and exits 1, as it does when it cannot read the configuration. A wrong
//! number of arguments prints the usage and exits 2.

use std::path::Path;
use std::sync::Mutex;

use zbm_app::{BootError, Recovery};
use zbm_bootenv::kernels::Kernel;
use zbm_config::conf::Conf;
use zbm_platform::System;
use zbm_platform::color::decolorize;
use zbm_startup::preinit;
use zbm_startup::program::Startup;
use zbm_startup::shell::Shell;
use zbm_state::State;
use zbm_types::{DatasetName, Printable};

use crate::applets::on_terminal;
use crate::table::ZKEXEC;
use crate::{Io, USAGE};

/// `zkexec FILESYSTEM KERNEL INITRAMFS` on this process's terminal.
pub(crate) fn run(args: &[String], io: &mut Io) -> i32 {
    let kernel = match files(args, io) {
        Ok(kernel) => kernel,
        Err(status) => return status,
    };
    on_terminal(io, |system, io| configured(system, &kernel, io))
}

/// The kernel of `args`: `/boot/KERNEL` and `/boot/INITRAMFS` of the boot
/// environment, as the legacy script named them.
///
/// # Errors
///
/// The exit status, once said on standard error: for a wrong number of
/// arguments, or a name that is no boot environment's, such as a
/// snapshot's.
fn files(args: &[String], io: &mut Io) -> Result<Kernel, i32> {
    let [fs, kernel, initramfs] = args else {
        let _ = io.stderr.write_all(ZKEXEC.usage.as_bytes());
        return Err(USAGE);
    };
    let Ok(environment) = fs.parse::<DatasetName>() else {
        let _ = writeln!(
            io.stderr,
            "zkexec: {} is not the name of a boot environment",
            Printable(fs)
        );
        return Err(1);
    };
    Ok(Kernel::new(
        environment.as_str(),
        &format!("/boot/{kernel}"),
        &format!("/boot/{initramfs}"),
    ))
}

/// Boots `kernel` on `system` with the image's configuration, as the
/// legacy script sourced it: 1 when it cannot be read, once said on
/// standard error, as the script exited 1.
fn configured(system: System<'_>, kernel: &Kernel, io: &mut Io) -> i32 {
    match preinit::read_conf(&system, Path::new("/")) {
        Ok(conf) => boot(system, &conf, kernel, io),
        Err(error) => {
            let _ = writeln!(io.stderr, "zkexec: {}", Printable(&error.to_string()));
            1
        }
    }
}

/// Boots `kernel` on `system` with the image's configuration `conf`, as
/// this process: start-up's kernel log takes its messages until it
/// returns, as the menu's do. 1 when it returns, once it said why on
/// standard error.
fn boot(system: System<'_>, conf: &Conf, kernel: &Kernel, io: &mut Io) -> i32 {
    // Start-up's kernel log, which lives as long as `startup`.
    let startup = Startup::new(system, conf);
    let state = State::new(system.files, zbm_startup::preinit::base(conf));
    let recovery = PrintRecovery {
        shell: Shell::new(system, &state),
        said: Mutex::default(),
    };
    let status = startup.kexec_files(kernel, &recovery);
    for message in recovery.said() {
        let _ = writeln!(io.stderr, "zkexec: {}", Printable(&message));
    }
    status
}

/// The recovery of `zkexec`, which runs in the recovery shell already: a
/// failure opens no other shell, its message is kept to be said on
/// standard error; late LUKS volumes are unlocked as the menu's recovery
/// unlocks them.
struct PrintRecovery<'s, 'a> {
    /// The start-up program's recovery, for its late volumes.
    shell: Shell<'s, 'a>,
    /// The messages of the failures, without their colours.
    said: Mutex<Vec<String>>,
}

impl PrintRecovery<'_, '_> {
    /// What it was told, in order.
    fn said(self) -> Vec<String> {
        self.said
            .into_inner()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Recovery for PrintRecovery<'_, '_> {
    fn unlock_late(&self) -> Result<(), BootError> {
        self.shell.unlock_late()
    }

    fn emergency_shell(&self, message: &str) {
        let mut said = self
            .said
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        said.push(decolorize(message));
    }
}

#[cfg(test)]
mod tests;
