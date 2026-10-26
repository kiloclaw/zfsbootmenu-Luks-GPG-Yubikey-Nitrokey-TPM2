//! `brush`: the shell of the Rust images. See the [`zbm_brush`] library.

#![allow(
    clippy::disallowed_methods,
    reason = "the shell's own start: its arguments, its terminal and its restart"
)]

use std::io::IsTerminal as _;
use std::os::unix::process::CommandExt as _;
use std::path::Path;

fn main() {
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let link = Path::new("/proc/self/fd/0");
    let terminal = zbm_brush::terminal_of(link, std::io::stdin().is_terminal());
    let exe = Path::new("/proc/self/exe");
    if let Some(mut command) = zbm_brush::restart(exe, &args, terminal.as_deref()) {
        // Only returns when the restart fails; the shell then starts as asked.
        let _ = command.exec();
    }
    brush_shell::entry::run();
}
