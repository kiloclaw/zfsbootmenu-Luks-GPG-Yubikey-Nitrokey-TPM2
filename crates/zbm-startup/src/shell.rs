//! The recovery shell of the start-up program, and late LUKS unlocking
//! through the `zluks` applet: what the start-up stages and the boot code
//! need from the program that runs them ([`Recovery`]).

use std::fmt::Write as _;
use std::path::Path;

use zbm_boot::boot;
use zbm_boot::error::BootError;
use zbm_boot::prompt::{colorize, tput};
use zbm_boot::recovery::Recovery;
use zbm_boot::support::{self, Unsupported};
use zbm_platform::System;
use zbm_platform::dmesg::read_logs;
use zbm_state::State;
use zbm_tools::mount::Mount;
use zbm_tools::{Shown, programs, terminal};
use zbm_types::Printable;

use crate::error::StartupError;

/// Which unlocking of LUKS volumes a run of `zluks unlock` is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Attempt {
    /// The start-up stage.
    First,
    /// A retry of the import loop, for volumes that appeared late
    /// (`LUKS_RETRY=1`): it fails unless it opens a volume.
    Retry,
}

/// Runs `zluks unlock` in a process of its own, as the legacy stage unlocks in
/// a subshell, so that passphrases entered at the console do not stay in this
/// program's memory.
///
/// # Errors
///
/// [`BootError::Zluks`] with its exit status when it fails.
pub fn zluks_unlock(system: &System<'_>, attempt: Attempt) -> Result<(), BootError> {
    let env: &[(&str, &str)] = match attempt {
        Attempt::Retry => &[("LUKS_RETRY", "1")],
        Attempt::First => &[],
    };
    let status = programs::applet(system.runner, "zluks", &["unlock"], env)
        .map_or(127, |output| output.status.shell_code());
    if status == 0 {
        Ok(())
    } else {
        Err(BootError::Zluks { status })
    }
}

/// The recovery shell and late LUKS volumes of the start-up program.
pub struct Shell<'s, 'a> {
    system: System<'a>,
    state: &'s State<'a>,
}

impl<'s, 'a> Shell<'s, 'a> {
    /// The recovery shell of `system`.
    pub fn new(system: System<'a>, state: &'s State<'a>) -> Shell<'s, 'a> {
        Shell { system, state }
    }

    /// Logs each thing the runtime cannot handle yet as an error and opens
    /// the recovery shell, if there is any.
    ///
    /// # Errors
    ///
    /// [`StartupError::Unsupported`] with what there was, after the shell.
    pub fn refuse(&mut self, list: &[Unsupported]) -> Result<(), StartupError> {
        if list.is_empty() {
            return Ok(());
        }
        for item in list {
            zbm_trace::error!("{support}: {item}"; support = %support::MESSAGE, %item);
        }
        self.emergency_shell(support::MESSAGE);
        Err(StartupError::Unsupported {
            items: list.iter().map(ToString::to_string).collect(),
        })
    }
}

impl Recovery for Shell<'_, '_> {
    /// Unlocks LUKS volumes on disks that appeared late: `zluks unlock` as a
    /// retry, which succeeds only if it opened a volume.
    fn unlock_late(&mut self) -> Result<(), BootError> {
        if !self.system.command_exists("cryptsetup") {
            return Err(BootError::NoCryptsetup);
        }
        zluks_unlock(&self.system, Attempt::Retry)
    }

    /// The recovery shell: a banner with the message and the errors logged since
    /// the last shell, then an interactive Bash with the default `SIGINT`
    /// action; afterwards, ZFS file systems mounted in it are unmounted.
    fn emergency_shell(&mut self, message: &str) {
        let system = &self.system;
        tput(system, &["clear"]);
        tput(system, &["cnorm"]);
        terminal::stty(system.runner, &["echo"]);

        let suffix = if message.is_empty() {
            String::new()
        } else {
            format!(": {message}")
        };
        let banner = format!(
            "{}{suffix}\n\ntype '{}' for online documentation\n\
             type '{}' to return to ZFSBootMenu\n\n",
            colorize("green", "emergency shell"),
            colorize("red", "help"),
            colorize("red", "exit"),
        );
        let _ = system.console.write(banner.as_bytes());
        if self.state.has_errors() {
            if let Ok(errors) = read_logs(system, "err") {
                // Reading the log back undoes the kernel's escapes: what a
                // message named, such as a file of a boot environment, is
                // shown escaped, so that it cannot steer the console.
                let mut shown = String::new();
                for line in errors.lines() {
                    let _ = writeln!(shown, "{}", Printable(line));
                }
                let _ = system.console.write(shown.as_bytes());
            }
            let _ = system.console.write(b"\n");
            self.state.clear_errors();
        }
        if system.command_exists("efibootmgr") {
            boot::mount_efivarfs(system, "rw");
        }

        let _ = programs::interactive_shell(system.runner);

        let mounts = system
            .files
            .read_to_string(Path::new("/proc/self/mounts"))
            .unwrap_or_default();
        for line in mounts.lines() {
            let mut fields = line.split_whitespace();
            let (Some(mountpoint), Some("zfs")) = (fields.nth(1), fields.next()) else {
                continue;
            };
            let _ = Mount::new(system.runner, system.files)
                .unmount(Path::new(mountpoint), Shown::Console);
            zbm_trace::debug!("unmounting: {mountpoint}"; %mountpoint);
        }
        boot::mount_efivarfs(system, "ro");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbm_platform::KernelLog;
    use zbm_testkit::fakes::FakeSystem;

    #[test]
    fn the_shell_banner_matches_the_legacy_one() {
        use zbm_testkit::legacy::{Bash, LOG_STUBS};
        let mut parity = zbm_testkit::parity!("the_shell_banner_matches_the_legacy_one");
        for message in ["", "unable to mount \x1b[0;36mtank\x1b[0m"] {
            let legacy: String = parity.legacy(format!("{message:?}"), || {
                Bash::new()
                    .code(LOG_STUBS)
                    .code(
                        r#"source() {
                        case "$1" in
                          /lib/*) builtin source "$LEGACY/zfsbootmenu/lib/${1#/lib/}" ;;
                          *) builtin source "$@" ;;
                        esac
                      }"#,
                    )
                    .source("zfsbootmenu/lib/zfsbootmenu-core.sh")
                    .code("tput() { :; }; stty() { :; }; mount_efivarfs() { :; }")
                    .env("BASE", "/nonexistent")
                    .call("emergency_shell", &[message], b"exit\n")
                    .stdout_text()
            });
            let fake = FakeSystem::new();
            let state = State::new(&fake.files, "/zfsbootmenu");
            let _log = KernelLog::new(&fake.files, "4", Path::new("/zfsbootmenu"), 1).install();
            Shell::new(fake.system(), &state).emergency_shell(message);
            assert_eq!(fake.console.output_text(), legacy, "{message:?}");
        }
    }
}
