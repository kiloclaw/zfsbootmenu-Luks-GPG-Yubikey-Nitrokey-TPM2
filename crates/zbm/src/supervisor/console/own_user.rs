//! The console on the display as its own user (ROADMAP.md SE11, ADR 0012):
//! the supervisor makes the console's records directory and gives it to the
//! console's user, binds its control socket, opens the kernel log for it,
//! starts the broker that opens its devices, and has it change its user
//! before it runs, keeping `CAP_SYS_TTY_CONFIG` alone, with its locked
//! memory unbounded.

use std::fs::OpenOptions;
use std::io;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use zbm_platform::sandbox::CONSOLE_USER;
use zbm_sys::sandbox::{Capability, RunAs, run_as};
use zbm_sys::socket::{Connection, Listener};

/// Where a console that runs as its own user gets what its user may not
/// open: in the image, the system's places.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnUser {
    /// Its user and group.
    pub uid: u32,
    /// The directory of its records, which becomes its user's.
    pub records: PathBuf,
    /// Its control socket, which the supervisor binds.
    pub socket: PathBuf,
    /// The kernel log, opened for it.
    pub kmsg: PathBuf,
    /// The directory below which the broker opens its devices.
    pub dev: PathBuf,
}

impl OwnUser {
    /// The system's.
    pub fn system() -> OwnUser {
        OwnUser {
            uid: CONSOLE_USER,
            records: Path::new(zbm_state::BASE).join(zbm_state::CONSOLE),
            socket: PathBuf::from(zbm_app::console::SOCKET),
            kmsg: PathBuf::from("/dev/kmsg"),
            dev: PathBuf::from("/dev"),
        }
    }

    /// Has `command`, the console's, run as this user, with the descriptors
    /// it needs passed; the supervisor's end of the broker, which
    /// [`serve_devices`] then serves.
    ///
    /// # Errors
    ///
    /// When the records directory, the socket, the kernel log or the
    /// broker cannot be had.
    #[allow(
        clippy::disallowed_methods,
        reason = "the console's directory, made and given to its user, and the kernel log, \
                  opened for it: no file the Files interface reads or writes"
    )]
    pub fn prepare(&self, command: &mut Command) -> io::Result<Connection> {
        std::fs::create_dir_all(&self.records)?;
        std::os::unix::fs::chown(&self.records, Some(self.uid), Some(self.uid))?;
        std::fs::set_permissions(&self.records, std::fs::Permissions::from_mode(0o755))?;
        let listener = Listener::bind(&self.socket)?;
        let kmsg = OpenOptions::new().append(true).open(&self.kmsg)?;
        let (console, supervisor) = Connection::pair()?;
        let control = zbm_sys::pass_fd(command, listener.into_fd());
        let devices = zbm_sys::pass_fd(command, console.into());
        let kmsg = zbm_sys::pass_fd(command, kmsg.into());
        command.args([
            "--control-fd",
            &control.to_string(),
            "--devices-fd",
            &devices.to_string(),
            "--kmsg-fd",
            &kmsg.to_string(),
        ]);
        let user = RunAs {
            uid: self.uid,
            gid: self.uid,
            kept: vec![Capability::SysTtyConfig],
            memlock_unlimited: true,
        };
        run_as(command, &user);
        Ok(supervisor)
    }
}

/// Opens the devices the console asks for on `broker`, below `dev`, on a
/// thread of its own, until the console closes its end; each refusal goes
/// to `log`.
///
/// # Errors
///
/// When the thread cannot start.
pub fn serve_devices(
    broker: Connection,
    dev: PathBuf,
    log: impl Fn(&str) + Send + 'static,
) -> io::Result<()> {
    zbm_platform::event::spawn_traced("console-devices", move || {
        zbm_app::devices::serve(
            &broker,
            |kind, path| zbm_app::devices::open_checked(kind, path, &dev),
            log,
        );
    })
    .map(drop)
}

/// The largest crash report taken from the console's records.
const MAX_REPORT: u64 = 1 << 20;

/// Moves the crash reports the console wrote in its records, `records`,
/// into the state directory `base`, where `zbm debug`, `zbm report` and
/// the saved logs find them: once it ended, and as root, only regular files
/// of at most [`MAX_REPORT`] bytes, copied, so that what the console left
/// cannot be a link to a file of root's. Returns how many.
#[allow(
    clippy::disallowed_methods,
    reason = "files the console left, checked as files before they are read"
)]
pub fn adopt_crash_reports(records: &Path, base: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(records) else {
        return 0;
    };
    let mut adopted = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if crate::debug::report_order(name).is_none() {
            continue;
        }
        let source = records.join(name);
        let is_report = std::fs::symlink_metadata(&source)
            .is_ok_and(|metadata| metadata.is_file() && metadata.len() <= MAX_REPORT);
        let target = base.join(name);
        if is_report
            && !target.exists()
            && let Ok(text) = std::fs::read(&source)
            && std::fs::write(&target, text).is_ok()
        {
            adopted += 1;
        }
        let _ = std::fs::remove_file(&source);
    }
    adopted
}

#[cfg(test)]
mod tests;
