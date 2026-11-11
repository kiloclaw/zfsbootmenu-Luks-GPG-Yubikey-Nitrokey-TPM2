//! `logs --trace`, which `ztrace` is: the debug lines of the kernel log
//! ([`debug`]), each with its calls, in the legacy `ztrace`'s layout.
//!
//! Each line starts with the record's time since the boot, `[S.U]`, where
//! the legacy script printed `dmesg`'s relative time. The calls are in
//! colour on a terminal alone. What the kernel log holds is escaped
//! ([`Printable`]). The kernel log has debug lines only with `loglevel=7`
//! on the kernel command line: without any, it says so.

use std::fmt::Write as _;

use zbm_platform::Files;
use zbm_platform::dmesg::{self, Record, debug};
use zbm_types::Printable;

use crate::Io;

/// The colour of a call's function.
const RED: &str = "\x1b[0;31m";

/// The colour of the file of the first call, the outermost.
const GREEN: &str = "\x1b[0;32m";

/// The colour of the files of the calls after it, further in.
const ORANGE: &str = "\x1b[0;33m";

/// The end of a colour.
const RESET: &str = "\x1b[0m";

/// What `ztrace` prints without debug lines.
const NONE: &str = "no debug lines in the kernel log: ZFSBootMenu writes them only \
                    with loglevel=7 on the kernel command line\n";

/// Prints the debug lines of the kernel log in `ztrace`'s layout, in
/// colour or not; without any, says why.
///
/// The exit status is 1 when `/dev/kmsg` cannot be read, once said.
pub(super) fn show(files: &dyn Files, colour: bool, io: &mut Io) -> i32 {
    let records = match dmesg::read_after(files, &[debug::LEVEL], None) {
        Ok((records, _)) => records,
        Err(error) => {
            let _ = writeln!(io.stderr, "logs: unable to read the kernel log: {error}");
            return 1;
        }
    };
    let shown = layout(&records, colour);
    let shown = if shown.is_empty() { NONE } else { &shown };
    let _ = io.stdout.write_all(shown.as_bytes());
    0
}

/// The debug lines of `records` in `ztrace`'s layout: a line with the
/// time, the prefix and the message, then one for each call, each further
/// in.
fn layout(records: &[Record], colour: bool) -> String {
    let paint = |code: &'static str| if colour { code } else { "" };
    let mut shown = String::new();
    for record in records {
        let Some(line) = debug::line(&record.text) else {
            continue;
        };
        let time = format!(
            "[{:5}.{:06}]",
            record.micros / 1_000_000,
            record.micros % 1_000_000
        );
        let prefix = format!("{}:", Printable(line.prefix));
        let _ = writeln!(shown, "{time} {prefix:>11} {}", Printable(line.text));
        let mut pad = String::from(" ");
        for (index, call) in line.calls.iter().enumerate() {
            let function = call.function.map_or_else(String::new, |function| {
                format!("{}{}@", paint(RED), Printable(function))
            });
            let file = paint(if index == 0 { GREEN } else { ORANGE });
            let _ = writeln!(
                shown,
                "{time} {:>11}{pad}{function}{file}{}{}#{}",
                "trace:",
                Printable(call.file),
                paint(RESET),
                Printable(call.line),
            );
            pad.push(' ');
        }
    }
    shown
}

#[cfg(test)]
mod tests;
