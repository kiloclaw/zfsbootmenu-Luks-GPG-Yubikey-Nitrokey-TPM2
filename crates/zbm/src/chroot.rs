//! `zfs-chroot FILESYSTEM`: a shell in a chroot of a filesystem
//! ([`zbm_app::chroot`]), as the legacy `zfs-chroot` opened one, through
//! the shell's gate (ADR 0018, point 8): in a locked image it asks for the
//! PIN first, as `zbm lock shell` does ([`lock::ask_then`]), and a locked
//! encryption root's key is loaded as a boot chosen in the menu loads it.
//! It exits 0 once a shell ran, whatever the shell's status, and 1 when
//! none could be opened, saying why on standard error. Without a
//! filesystem it does nothing and exits 0, as the legacy script did.
//!
//! The terminal's hang-up and interrupt, `SIGQUIT` and `SIGTERM` wait
//! until it ends, as the legacy script trapped them: the chroot's shell
//! gets them, and the applet unmounts what it mounted before it exits.

use std::error::Error as _;
use std::io;
use std::path::Path;

use zbm_app::BootSettings;
use zbm_app::auth::{self, Authority};
use zbm_app::chroot::ChrootError;
use zbm_app::keyholder::Keyholders;
use zbm_config::conf::{self, Conf};
use zbm_platform::{RealSystem, System};
use zbm_startup::program::Startup;
use zbm_state::State;
use zbm_sys::{Signal, SignalSet, Signals};
use zbm_types::{FilesystemName, Printable};

use crate::lock::{self, Open};
use crate::table::ZFS_CHROOT;
use crate::unlock::ProcessKeyholders;
use crate::{Io, USAGE};

/// The signals that would end the applet with the chroot mounted: they
/// wait, blocked, until it has unmounted it and exits.
const HELD: SignalSet = SignalSet::of(&[
    Signal::Hangup,
    Signal::Interrupt,
    Signal::Quit,
    Signal::Terminate,
]);

/// `zfs-chroot FILESYSTEM` on this process's terminal.
pub(crate) fn run(args: &[String], io: &mut Io) -> i32 {
    let fs = match filesystem(args, io) {
        Ok(Some(fs)) => fs,
        Ok(None) => return 0,
        Err(status) => return status,
    };
    // Blocked before any thread starts, so that every thread blocks them;
    // the programs it runs get them as usual.
    let _held = Signals::block(HELD).ok();
    let real = RealSystem::new();
    match real.terminal() {
        Ok(terminal) => open_here(&real.with_console(&terminal), &fs, io),
        Err(error) => no_terminal(&error, io),
    }
}

/// Opens a shell in a chroot of `fs` on `system`, as this process: with
/// the settings of a boot chosen in the menu, from the image's
/// configuration, and this process's authority and keyholders. Its
/// messages go to the kernel log until it returns, as the menu's do.
fn open_here(system: &System<'_>, fs: &FilesystemName, io: &mut Io) -> i32 {
    let conf = system
        .files
        .read_to_string(Path::new(conf::PATH))
        .map(|text| Conf::parse(&text))
        .unwrap_or_default();
    // Start-up's kernel log, which lives as long as `startup`.
    let startup = Startup::new(*system, &conf);
    let settings = startup.menu_settings();
    zbm_trace::debug!("zfs-chroot started with {fs}"; %fs);
    let state = State::new(system.files, Path::new(zbm_state::BASE));
    let keyholders = ProcessKeyholders::new();
    let authority = (auth::process(), &keyholders as &dyn Keyholders);
    open(system, &state, authority, &settings, fs, io)
}

/// The filesystem of `args`: none for no argument or an empty one, which
/// the legacy script took as nothing to do.
///
/// # Errors
///
/// The exit status, once said on standard error: for more than one
/// argument, or a name that is no filesystem's.
fn filesystem(args: &[String], io: &mut Io) -> Result<Option<FilesystemName>, i32> {
    let name = match args {
        [] => return Ok(None),
        [name] if name.is_empty() => return Ok(None),
        [name] => name,
        _ => {
            let _ = io.stderr.write_all(ZFS_CHROOT.usage.as_bytes());
            return Err(USAGE);
        }
    };
    name.parse().map(Some).map_err(|_| {
        let _ = writeln!(
            io.stderr,
            "zfs-chroot: {} is not the name of a filesystem",
            Printable(name)
        );
        1
    })
}

/// Says on standard error that there is no terminal to ask and open the
/// shell on, `error` keeping it: 1.
fn no_terminal(error: &io::Error, io: &mut Io) -> i32 {
    zbm_trace::error!("zfs-chroot: no terminal to open a shell on: {error}"; %error);
    let _ = writeln!(
        io.stderr,
        "zfs-chroot: no terminal to open a shell on: {error}"
    );
    1
}

/// Opens a shell in a chroot of `fs` on the console of `system`, with
/// `settings`: once `authority` authorises it, asking for the PIN as
/// `zbm lock shell` does. 0 once a shell ran, else 1, said on standard
/// error when the chroot failed.
fn open(
    system: &System<'_>,
    state: &State<'_>,
    authority: (&Authority, &dyn Keyholders),
    settings: &BootSettings,
    fs: &FilesystemName,
    io: &mut Io,
) -> i32 {
    match lock::ask_then(system, state, authority, Open::Chroot(fs, settings)) {
        Ok(status) => status,
        Err(error) => {
            say(&error, io);
            1
        }
    }
}

/// Says on standard error why the chroot opened no shell: `error`, then
/// each of its causes.
fn say(error: &ChrootError, io: &mut Io) {
    let _ = writeln!(io.stderr, "zfs-chroot: {}", Printable(&error.to_string()));
    let mut cause = error.source();
    while let Some(reason) = cause {
        let _ = writeln!(io.stderr, "  {}", Printable(&reason.to_string()));
        cause = reason.source();
    }
}

#[cfg(test)]
mod tests;
