//! The recovery shell of the start-up program, and late LUKS unlocking
//! through the `zluks` applet: what the start-up stages and the boot code
//! need from the program that runs them ([`Recovery`]).

use std::path::Path;

use crate::support::{self, Unsupported};
use zbm_app::auth::{self, Authority};
use zbm_boot::BootError;
use zbm_boot::Recovery;
use zbm_platform::console::Screen;
use zbm_platform::dmesg::{ERR, escaped, read_after, read_logs, texts};
use zbm_platform::{Context, System};
use zbm_state::State;
use zbm_tools::console::colorize;
use zbm_tools::mount::Mount;
use zbm_tools::{Shown, programs};

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
    /// Who may open the shell: the lock of this process's image
    /// (ROADMAP.md SE9).
    authority: &'s Authority,
}

impl<'s, 'a> Shell<'s, 'a> {
    /// The recovery shell of `system`, behind the lock of this process.
    pub fn new(system: System<'a>, state: &'s State<'a>) -> Shell<'s, 'a> {
        Shell {
            system,
            state,
            authority: auth::process(),
        }
    }

    /// The same shell behind the lock of `authority`, for tests.
    #[must_use]
    pub fn with_authority(self, authority: &'s Authority) -> Shell<'s, 'a> {
        Shell { authority, ..self }
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

    /// Shows every error logged since the boot, each line escaped
    /// ([`escaped`]) so that none can steer the console, when errors were
    /// logged since they were last shown (`have_errors`), as the legacy
    /// banner did, and records the number of the last record of the kernel log
    /// (`errors_since`), whether it showed any or not: the shell's prompt
    /// (`logs --prompt`) shows only the errors after it, so none that the
    /// banner showed or that were here already. Without the kernel log's
    /// records, `dmesg` shows the errors, and nothing is recorded.
    fn show_errors(&self) {
        let system = &self.system;
        let read = read_after(system.files, &[ERR], None);
        if self.state.has_errors() {
            let errors = match &read {
                Ok((records, _)) => Some(texts(records)),
                Err(_) => read_logs(system, "err").ok(),
            };
            if let Some(errors) = errors {
                let _ = system.console.write(escaped(&errors).as_bytes());
            }
            let _ = system.console.write(b"\n");
            self.state.clear_errors();
        }
        if let Ok((_, Some(last))) = read {
            let _ = self.state.set_errors_since(last);
        }
    }
}

impl Recovery for Shell<'_, '_> {
    /// Unlocks LUKS volumes on disks that appeared late: `zluks unlock` as a
    /// retry, which succeeds only if it opened a volume.
    fn unlock_late(&self) -> Result<(), BootError> {
        if !self.system.command_exists("cryptsetup") {
            return Err(BootError::NoCryptsetup);
        }
        zluks_unlock(&self.system, Attempt::Retry)
    }

    /// The recovery shell: a banner with the message and the errors logged since
    /// the last shell, then an interactive Bash with the default `SIGINT`
    /// action; afterwards, ZFS file systems mounted in it are unmounted.
    fn emergency_shell(&self, message: &str) {
        let system = &self.system;
        system.console.screen(Screen::Clear);
        system.console.screen(Screen::ShowCursor);
        system.console.set_echo(true);

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
        self.show_errors();
        if system.command_exists("efibootmgr") {
            zbm_boot::mount_efivarfs(&Context::new(*system), "rw");
        }

        let _ = auth::recovery_shell(
            self.authority,
            self.state,
            system.runner,
            system.clock.now(),
        );

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
        zbm_boot::mount_efivarfs(&Context::new(*system), "ro");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbm_platform::{Files as _, KernelLog};
    use zbm_testkit::fakes::FakeSystem;

    #[test]
    fn the_banner_writes_the_stamp_of_the_last_record() {
        let fake = FakeSystem::new();
        fake.files.dir("/zfsbootmenu");
        let state = State::new(&fake.files, "/zfsbootmenu");
        // Records 0 to 2: an error, then an error and a notice that come
        // after it.
        fake.files
            .append(
                Path::new("/dev/kmsg"),
                b"<3>ZFSBootMenu: broken \x1b[31mpool\n<3>ZFSBootMenu: again\n<5>later\n",
            )
            .unwrap();
        fake.files.file("/zfsbootmenu/have_errors", "");
        Shell::new(fake.system(), &state).emergency_shell("");
        let shown = fake.console.output_text();
        assert!(
            shown.ends_with("ZFSBootMenu: broken \\x1b[31mpool\nZFSBootMenu: again\n\n"),
            "{shown:?}"
        );
        assert!(!state.has_errors());
        assert_eq!(state.errors_since(), Some(2));

        // Without errors to show, the stamp is written all the same.
        fake.files
            .append(Path::new("/dev/kmsg"), b"<6>ZFSBootMenu: more\n")
            .unwrap();
        Shell::new(fake.system(), &state).emergency_shell("");
        assert_eq!(state.errors_since(), Some(3));

        // Without the kernel log's records, `dmesg` shows the errors, and
        // no stamp is written.
        fake.files.file("/zfsbootmenu/have_errors", "");
        fake.files
            .fail("/dev/kmsg", std::io::ErrorKind::PermissionDenied);
        fake.runner.respond(
            "dmesg -f user --color=never -l err",
            0,
            "[ 1.0] ZFSBootMenu: from dmesg\n",
        );
        Shell::new(fake.system(), &state).emergency_shell("");
        assert!(
            fake.console
                .output_text()
                .ends_with("ZFSBootMenu: from dmesg\n\n")
        );
        assert_eq!(state.errors_since(), Some(3));
    }

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
