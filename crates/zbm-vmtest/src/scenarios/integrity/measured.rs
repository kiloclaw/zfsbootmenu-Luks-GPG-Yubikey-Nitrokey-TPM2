//! `integrity-measured` (ADR 0008): with the TPM, `tauto`'s boot
//! environment boots measured, and the event log its initramfs kept
//! replays to its PCRs 8 and 9.

use std::fs;

use sha2::{Digest as _, Sha256};

use super::{BASE, start};
use crate::boot::{Boot, Follow, check_be, follow_boot};
use crate::error::{Error, Result};
use crate::images::be_kver;
use crate::record::Test;
use crate::settings::Settings;

/// `measured`: with the TPM, `tauto`'s boot environment boots by itself,
/// measured.
pub(super) fn run(test: &Test) -> Result<()> {
    let append = format!("{BASE} zbm.prefer=tauto");
    let mut vm = start(test, "measured", (&append, &[0]), (true, None))?;
    let boot = follow_boot(&mut vm, Follow::default())?;
    test.check(
        !boot.has_prompt("luks"),
        "the TPM unlocked auto0, bound to PCR 9, before the boot was measured",
    );
    check_be(test, &boot, "tauto", &["tauto"], &[]);
    let payload = Payload::read(test.settings(), &be_kver(test.settings())?)?;
    check_measured(test, &boot, &payload);
    Ok(())
}

/// The SHA-256 digests of the boot environment's kernel and initramfs, as
/// they are on its dataset: the payload's.
#[derive(Debug, Eq, PartialEq)]
pub(super) struct Payload {
    pub(super) kernel: Vec<u8>,
    pub(super) initramfs: Vec<u8>,
}

impl Payload {
    /// The digests of the payload's files, of the kernel `kver`.
    pub(super) fn read(settings: &Settings, kver: &str) -> Result<Payload> {
        let boot = settings.work.join("payload/boot");
        let digest = |name: String| {
            let path = boot.join(name);
            let bytes =
                fs::read(&path).map_err(Error::io(format!("reading {}", path.display())))?;
            Ok::<_, Error>(Sha256::digest(&bytes).to_vec())
        };
        Ok(Payload {
            kernel: digest(format!("vmlinuz-{kver}"))?,
            initramfs: digest(format!("initramfs-{kver}.img"))?,
        })
    }
}

/// One record of the event log, read apart from the runtime's reader, as
/// ADR 0008 describes it: its PCR, its event, the value it names, and its
/// SHA-256 digest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Record {
    pub(super) pcr: u64,
    pub(super) event: String,
    pub(super) value: String,
    pub(super) sha256: Vec<u8>,
}

/// The bytes of the hexadecimal `text`, if it is that.
pub(super) fn unhex(text: &str) -> Option<Vec<u8>> {
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(text.get(at..at + 2)?, 16).ok())
        .collect()
}

/// The records of the event log the boot environment printed (`BE-ML| `
/// lines, a JSON record each, whose record separators it printed as
/// spaces), as far as they read.
pub(super) fn printed_log(be: &str) -> Vec<Record> {
    be.lines()
        .filter_map(|line| line.split_once("BE-ML| ").map(|(_, record)| record.trim()))
        .map_while(|record| {
            let json: serde_json::Value = serde_json::from_str(record).ok()?;
            let sha256 = json.get("digests")?.as_array()?.iter().find_map(|digest| {
                (digest.get("hashAlg")?.as_str()? == "sha256")
                    .then(|| unhex(digest.get("digest")?.as_str()?))?
            })?;
            Some(Record {
                pcr: json.get("pcr")?.as_u64()?,
                event: json.pointer("/content/event")?.as_str()?.to_owned(),
                value: json.pointer("/content/value")?.as_str()?.to_owned(),
                sha256,
            })
        })
        .collect()
}

/// The SHA-256 value of `pcr` after `records`: from zeros, or a start's
/// value, each extension the digest of the old value and the record's.
pub(super) fn replay(records: &[Record], pcr: u64) -> Vec<u8> {
    records
        .iter()
        .filter(|record| record.pcr == pcr)
        .fold(vec![0; 32], |value, record| {
            if record.event == "start" {
                record.sha256.clone()
            } else {
                Sha256::new()
                    .chain_update(&value)
                    .chain_update(&record.sha256)
                    .finalize()
                    .to_vec()
            }
        })
}

/// The value of `pcr` in the SHA-256 bank that the boot environment read
/// (`BE-PCR: 9:sha256=...`), if it did.
pub(super) fn be_pcr(boot: &Boot, pcr: u64) -> Option<Vec<u8>> {
    let prefix = format!("{pcr}:sha256=");
    let text = boot
        .be_lines
        .get("BE-PCR")?
        .iter()
        .find_map(|line| line.strip_prefix(&prefix))?;
    unhex(text)
}

/// The checks of [`measured`], on its `boot` and the `payload`'s digests.
pub(super) fn check_measured(test: &Test, boot: &Boot, payload: &Payload) {
    let records = printed_log(&boot.be);
    let events: Vec<&str> = records.iter().map(|record| record.event.as_str()).collect();
    test.check(
        events
            == [
                "start",
                "start",
                "kernel",
                "initramfs",
                "command-line",
                "separator",
            ],
        &format!("the boot environment kept ZFSBootMenu's event log ({events:?})"),
    );
    for pcr in [8, 9] {
        test.check(
            be_pcr(boot, pcr) == Some(replay(&records, pcr)),
            &format!("PCR {pcr} is what the log replays to"),
        );
    }
    let digest = |event: &str| {
        records
            .iter()
            .find(|record| record.event == event)
            .map(|record| record.sha256.as_slice())
    };
    test.check(
        digest("kernel") == Some(payload.kernel.as_slice())
            && digest("initramfs") == Some(payload.initramfs.as_slice()),
        "the kernel and initramfs were measured as they are on the dataset (as predicted)",
    );
    let words = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
    let measured_line = records
        .iter()
        .find(|record| record.event == "command-line")
        .map(|record| words(&record.value));
    test.check(
        measured_line == Some(words(boot.be_line("BE-CMDLINE"))),
        "the command line measured is the one the kernel got",
    );
    let start = records
        .iter()
        .find(|record| record.event == "start" && record.pcr == 9);
    test.check(
        start.is_some_and(|start| be_pcr(boot, 9).is_some_and(|now| now != start.sha256)),
        "PCR 9 moved on from the value auto0's token is bound to",
    );
}
