//! The VM tests of ZFSBootMenu: images built by the build container, booted
//! in QEMU, driven through their serial console.
//!
//! The harness ([`vm::Vm`]) starts QEMU with the serial console on a Unix
//! socket. A thread reads the console into the test's log, into a terminal
//! emulator ([`console::Console::screen`]: the text a user sees) and into
//! the text that [`console::Console::expect`] matches, with the terminal's
//! escape sequences removed. The scenarios ([`scenarios`]) create pools in
//! the recovery shell of a first boot, then boot again and check what
//! ZFSBootMenu and the test boot environment report.
//!
//! `zbm-vmtest run` runs them; see `docs/luks/testing.rst`.

pub mod boot;
pub mod console;
pub mod disks;
pub mod error;
pub mod expect;
pub mod gpt;
pub mod images;
pub mod pyfmt;
pub mod qemu;
pub mod record;
pub mod scenarios;
pub mod settings;
pub mod swtpm;
#[cfg(test)]
mod testing;
pub mod text;
pub mod vm;

pub use error::{Error, Result};
pub use record::Test;
pub use settings::Settings;
