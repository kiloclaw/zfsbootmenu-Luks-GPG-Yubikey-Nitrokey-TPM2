//! The console without the kernel's (ROADMAP.md #19, step 2).
//!
//! `zbm console` runs between the programs of the runtime and the display:
//! the supervisor gives it the controlling side of a pseudo-terminal, on
//! whose other side the start-up program and the recovery shell run, and it
//! draws what they write on the display through DRM and sends them what is
//! typed on the keyboards:
//!
//! - [`screen`]: Alacritty's terminal, drawn as pixels with the image's
//!   console font ([`colour`]: in the console's palette and beyond);
//! - [`keys`]: key presses as the bytes of xterm or the Linux console;
//! - [`typing`]: the keys the console takes for itself (Super+Space,
//!   Ctrl+Shift+U);
//! - [`run`]: the loop between the terminal, the display and the keyboards,
//!   on a [`run::Host`] that tests replace;
//! - [`host`]: the real one;
//! - [`relay`]: the relay that carries the terminal over to the kernel
//!   console once the display is handed back.
//!
//! Before `kexec`, `zbm console release` asks it to hand the display, the
//! keyboards and the virtual terminal back; it then relays between the
//! pseudo-terminal and the kernel console, so that the booted kernel takes
//! the console over as it does today, and a failed boot is still seen.

pub mod colour;
pub mod host;
pub mod keys;
pub mod mirror;
pub mod relay;
pub mod release;
pub mod run;
pub mod scheme;
pub mod screen;
pub mod serve;
pub mod typing;
