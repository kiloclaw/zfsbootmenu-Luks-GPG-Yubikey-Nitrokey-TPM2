//! `zbm lock`: the locked menu's side of the recovery shell (ROADMAP.md
//! SE9, ADR 0010).
//!
//! - `zbm lock shell` asks for the PIN of the image on the terminal, has a
//!   keyholder check it, and opens the recovery shell through its gate once
//!   it is right. The supervisor runs it in place of the shell while the
//!   image is locked, and the start-up program when nobody proved
//!   themselves yet. Without a lock, or after a person opened a volume this
//!   boot, it opens the shell at once. An empty PIN, or three wrong ones,
//!   go back: it ends, and the menu shows again. `zfs-chroot` asks so
//!   before its chroot.
//! - `zbm lock hash` asks for a PIN twice, on the machine that builds the
//!   image, and prints the line of its configuration that locks the image
//!   with it: `zfsbootmenu_lock_hash='$argon2id$…'`.
//! - `zbm lock status` says whether the image is locked.

use std::path::Path;

use zbm_app::BootSettings;
use zbm_app::auth::{self, Authority, Lock, ShellGate};
use zbm_app::chroot::{self, ChrootError};
use zbm_app::keyholder::Keyholders;
use zbm_platform::{RealSystem, System};
use zbm_state::State;
use zbm_types::FilesystemName;

use crate::unlock::ProcessKeyholders;
use crate::{Io, USAGE};

/// The usage of `zbm lock`.
const USAGE_TEXT: &str = crate::table::LOCK.usage;

/// The wrong PINs after which `zbm lock shell` goes back to the menu.
const TRIES: u32 = 3;

/// What the prompt says first.
const LOCKED: &str = "This ZFSBootMenu is locked. Type its PIN, or a blank line to go back.\n";

/// Runs `zbm lock` with `args`.
pub fn run(args: &[String], io: &mut Io) -> i32 {
    match args.first().map(String::as_str) {
        Some("shell") if args.len() == 1 => shell(),
        Some("hash") if args.len() == 1 => match RealSystem::new().terminal() {
            Ok(terminal) => {
                let real = RealSystem::new();
                hash(&real.with_console(&terminal), io)
            }
            Err(error) => {
                let _ = writeln!(io.stderr, "zbm lock: no terminal to ask on: {error}");
                1
            }
        },
        Some("status") if args.len() == 1 => {
            let state = if Lock::of(&zbm_platform::RealFiles).is_on() {
                "locked"
            } else {
                "not locked"
            };
            let _ = writeln!(io.stdout, "{state}");
            0
        }
        Some("-h") => {
            let _ = io.stdout.write_all(USAGE_TEXT.as_bytes());
            0
        }
        _ => {
            let _ = io.stderr.write_all(USAGE_TEXT.as_bytes());
            USAGE
        }
    }
}

/// `zbm lock hash`: asks for a PIN twice on `system`'s console, and
/// prints the line that locks an image with it.
fn hash(system: &System<'_>, io: &mut Io) -> i32 {
    let console = system.console;
    let read = |prompt: &[u8]| {
        let _ = console.write(prompt);
        let pin = console.read_secret().ok().flatten();
        let _ = console.write(b"\n");
        pin
    };
    let (Some(pin), Some(again)) = (read(b"New PIN: "), read(b"The PIN again: ")) else {
        let _ = writeln!(io.stderr, "zbm lock: no PIN was typed");
        return 1;
    };
    if !pin.ct_eq(again.expose()) {
        let _ = writeln!(io.stderr, "zbm lock: the two PINs differ");
        return 1;
    }
    match auth::hash_pin(system.files, &pin) {
        Ok(hash) => {
            let _ = writeln!(io.stdout, "zfsbootmenu_lock_hash='{hash}'");
            0
        }
        Err(why) => {
            let _ = writeln!(io.stderr, "zbm lock: {why}");
            1
        }
    }
}

/// `zbm lock shell` on this machine's terminal.
fn shell() -> i32 {
    let real = RealSystem::new();
    let terminal = match real.terminal() {
        Ok(terminal) => terminal,
        Err(error) => {
            zbm_trace::error!("zbm lock: no terminal to ask on: {error}"; %error);
            return 1;
        }
    };
    let system = real.with_console(&terminal);
    let state = State::new(system.files, Path::new(zbm_state::BASE));
    let keyholders = ProcessKeyholders::new();
    // Only a chroot fails with an error.
    ask_then(&system, &state, (auth::process(), &keyholders), Open::Shell).unwrap_or(1)
}

/// What [`ask_then`] opens once the person is authorised.
#[derive(Clone, Copy)]
pub(crate) enum Open<'a> {
    /// The recovery shell, in the image.
    Shell,
    /// A shell in a chroot of the filesystem, its key loaded with the
    /// settings as a boot chosen in the menu loads it
    /// ([`zbm_app::chroot::open`]).
    Chroot(&'a FilesystemName, &'a BootSettings),
}

/// Asks for the PIN on `system`'s console until `authority` authorises
/// what to `open`, `keyholders` checking it, and opens it then. The exit
/// status: the shell's, 0 once a chroot's shell ran, or 1 when the person
/// went back.
///
/// # Errors
///
/// Why a chroot opened no shell.
pub(crate) fn ask_then(
    system: &System<'_>,
    state: &State<'_>,
    (authority, keyholders): (&Authority, &dyn Keyholders),
    open: Open<'_>,
) -> Result<i32, ChrootError> {
    let console = system.console;
    for attempt in 0..=TRIES {
        let now = system.clock.now();
        if let Some(authorized) = authority.authorized(state.has_attended_unlock(), now) {
            return match open {
                Open::Shell => {
                    let ran = ShellGate::open(&authorized).run(system.runner);
                    authority.forget();
                    Ok(ran.map_or(1, |output| output.status.shell_code()))
                }
                // The chroot takes the proof just used, which lasts.
                Open::Chroot(fs, settings) => {
                    chroot::open(system, state, authority, settings, fs).map(|_| 0)
                }
            };
        }
        if attempt == TRIES {
            break;
        }
        let _ = console.write(if attempt == 0 { LOCKED } else { "" }.as_bytes());
        let _ = console.write(b"PIN: ");
        let pin = match console.read_secret() {
            Ok(Some(pin)) if !pin.is_empty() => pin,
            _ => break,
        };
        let _ = console.write(b"\n");
        match auth::check_pin(authority, keyholders, &pin, system.clock.now()) {
            Ok(true) => {}
            Ok(false) => {
                let _ = console.write(b"Wrong PIN.\n");
            }
            Err(error) => {
                zbm_trace::error!("zbm lock: the PIN could not be checked: {error}"; %error);
                let _ = console.write(b"The PIN could not be checked.\n");
                break;
            }
        }
    }
    let _ = console.write(b"Back to ZFSBootMenu.\n");
    Ok(1)
}

#[cfg(test)]
pub(crate) mod tests;
