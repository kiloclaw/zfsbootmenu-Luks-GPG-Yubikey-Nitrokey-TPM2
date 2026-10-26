//! `brush`: the shell of the Rust images. See the [`zbm_brush`] library.

#![allow(
    clippy::disallowed_methods,
    reason = "the shell's own start: its terminal, and its restart with other arguments"
)]

use std::io::IsTerminal as _;
use std::os::unix::process::CommandExt as _;

fn main() {
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let terminal = std::io::stdin()
        .is_terminal()
        .then(|| std::fs::read_link("/proc/self/fd/0").ok())
        .flatten();
    if let Some(changed) = zbm_brush::arguments(&args, terminal.as_deref())
        && let Some((program, rest)) = changed.split_first()
    {
        // Only returns when the restart fails; the shell then starts as asked.
        let _ = std::process::Command::new("/proc/self/exe")
            .arg0(program)
            .args(rest)
            .exec();
    }
    brush_shell::entry::run();
}
