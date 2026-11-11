//! `logs --trace`, which `ztrace` is: the debug lines of the kernel log
//! ([`debug`]), each with its calls, in the legacy `ztrace`'s layout.
//!
//! Each line starts with the record's time since the boot, `[S.U]`, where
//! the legacy script printed `dmesg`'s relative time. What the kernel log
//! holds is escaped ([`Printable`]), and the lines are not coloured: the
//! pager shows every character it is given, escapes too. The kernel log
//! has debug lines only with `loglevel=7` on the kernel command line:
//! without any, it says so.

use std::fmt::Write as _;

use zbm_platform::Files;
use zbm_platform::dmesg::{self, Record, debug};
use zbm_tui::menu::pager::Topic;
use zbm_types::Printable;

use super::Paged;
use crate::Io;

/// What `ztrace` prints without debug lines.
pub(super) const NONE: &str = "no debug lines in the kernel log: ZFSBootMenu writes them \
                               only with loglevel=7 on the kernel command line\n";

/// Shows the debug lines of the kernel log in `ztrace`'s layout with
/// `paged`; without any, says why.
///
/// The exit status is 1 when `/dev/kmsg` cannot be read, once said, and
/// as [`crate::help::shown`] says otherwise.
pub(super) fn show(files: &dyn Files, io: &mut Io, paged: &mut Paged<'_>) -> i32 {
    let records = match dmesg::read_after(files, &[debug::LEVEL], None) {
        Ok((records, _)) => records,
        Err(error) => {
            let _ = writeln!(io.stderr, "logs: unable to read the kernel log: {error}");
            return 1;
        }
    };
    let lines = layout(&records);
    if lines.is_empty() {
        let _ = io.stdout.write_all(NONE.as_bytes());
        return 0;
    }
    paged.show(Topic::Trace, lines, io)
}

/// The debug lines of `records` in `ztrace`'s layout: a line with the
/// time, the prefix and the message, then one for each call, each further
/// in.
fn layout(records: &[Record]) -> Vec<String> {
    let mut shown = Vec::new();
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
        shown.push(format!("{time} {prefix:>11} {}", Printable(line.text)));
        let mut pad = String::from(" ");
        for call in &line.calls {
            let mut text = format!("{time} {:>11}{pad}", "trace:");
            if let Some(function) = call.function {
                let _ = write!(text, "{}@", Printable(function));
            }
            let _ = write!(text, "{}#{}", Printable(call.file), Printable(call.line));
            shown.push(text);
            pad.push(' ');
        }
    }
    shown
}

#[cfg(test)]
mod tests;
