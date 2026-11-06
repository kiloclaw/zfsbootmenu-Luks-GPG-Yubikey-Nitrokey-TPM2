//! The demo's boot history ([`Action::LoadHistory`]): the boots whose logs
//! `zbm.save_logs` kept, newest first. The last boot loaded the default,
//! measured; one before it was saved from the shell, without a record; one
//! has a record a newer runtime wrote; and one was saved when the default
//! failed to boot, its record's kernel named with an escape sequence, as
//! a hostile disk could. Without `zbm.save_logs` the history is off.
//!
//! [`Action::LoadHistory`]: crate::menu::world::Action::LoadHistory

use zbm_types::BootRecord;

use super::data::NOW;
use super::{DemoOptions, untrusted};
use crate::menu::world::{History, Recorded, SavedBoot};

/// An hour in seconds.
const HOUR: u64 = 3_600;

/// The history of the demo machine.
pub(super) fn history(options: &DemoOptions) -> History {
    if !options.save_logs {
        return History::Off;
    }
    History::Saved(vec![
        saved(
            "2026-10-22T19-00-00Z",
            Recorded::Read(record(NOW - 13 * HOUR, "/boot/vmlinuz-6.12.9_1", true)),
        ),
        saved("2026-10-22T08-31-05Z", Recorded::Missing),
        saved(
            "2026-10-21T22-04-51Z",
            Recorded::Unreadable("the record has schema 2, not 1".to_owned()),
        ),
        saved(
            "2026-10-20T08-00-00Z",
            Recorded::Read(record(
                NOW - 72 * HOUR,
                "/boot/vmlinuz-6.12.8\u{1b}[2J_1",
                false,
            )),
        ),
    ])
}

/// A saved boot named `name`, with its journal, kernel log and record.
fn saved(name: &str, record: Recorded) -> SavedBoot {
    let mut files = vec![
        (untrusted("journal.jsonl"), 48_211),
        (untrusted("kernel.log"), 61_003),
    ];
    if record != Recorded::Missing {
        files.insert(0, (untrusted("boot.json"), 312));
    }
    SavedBoot {
        name: untrusted(name),
        files,
        record,
    }
}

/// The record of a boot of the default environment with `kernel`, at
/// `time`, measured or not.
fn record(time: u64, kernel: &str, measured: bool) -> BootRecord {
    BootRecord {
        time,
        environment: untrusted("zroot/ROOT/void"),
        kernel: untrusted(kernel),
        initramfs: untrusted("/boot/initramfs-6.12.9_1.img"),
        command_line: untrusted("quiet loglevel=4"),
        devicetree: None,
        loaded_by: untrusted("kexec_file_load"),
        measured,
        command_line_edited: false,
    }
}
