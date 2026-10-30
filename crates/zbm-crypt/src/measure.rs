//! Measured boot (ROADMAP.md SE10, ADR 0008): what ZFSBootMenu extends
//! into the TPM before `kexec`, and the log of it.
//!
//! The command line the boot environment's kernel gets goes into
//! [`COMMAND_LINE_PCR`] (8), its kernel and its initramfs, as they are on
//! the boot environment, into [`FILES_PCR`] (9), as GRUB does; once the
//! last volume of the boot is open, the separator [`UNSEALED`] goes into
//! PCR 9 too, so that a TPM2 token bound to PCR 9 unseals for ZFSBootMenu
//! alone. Each event is extended into every active bank with that bank's
//! digest ([`Digests`]), and recorded in a TCG Canonical Event Log, as
//! JSON text sequences (RFC 7464) of CEL-JSON records ([`record`]), which
//! [`replay`] turns back into the values the PCRs must hold.
//!
//! The log starts with what PCRs 8 and 9 held before ZFSBootMenu extended
//! them ([`Measured::Start`]): on a UEFI machine, the firmware or the
//! kernel's EFI stub may have extended PCR 9 already, with ZFSBootMenu's
//! own initramfs, so a replay from zeros would not give what the TPM holds.
//!
//! The log is not measured: it is evidence, which the PCRs check. It goes
//! to the boot environment in a cpio archive appended to its initramfs
//! ([`cpio`]), at [`LOG_PATH`].

use std::fmt::Write as _;

use serde_json::{Value, json};
use sha1::Sha1;
use sha2::{Sha256, Sha384, Sha512};

/// The PCR of the command line.
pub const COMMAND_LINE_PCR: u32 = 8;

/// The PCR of the kernel, the initramfs and the separator.
pub const FILES_PCR: u32 = 9;

/// The separator's event: what ZFSBootMenu unsealed, it unsealed.
pub const UNSEALED: &str = "zbm-unsealed";

/// Where the log is in the initramfs of the boot environment: `/.extra`,
/// where systemd-stub leaves what the boot loader gives the system.
pub const LOG_PATH: &str = ".extra/zfsbootmenu/tpm-measure.json";

/// The content type of the records.
const CONTENT_TYPE: &str = "zfsbootmenu";

/// The record separator of JSON text sequences.
const RS: char = '\u{1e}';

/// A PCR bank: the hash algorithm of its values.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Bank {
    /// SHA-1, often active beside SHA-256.
    Sha1,
    /// SHA-256.
    Sha256,
    /// SHA-384.
    Sha384,
    /// SHA-512.
    Sha512,
}

impl Bank {
    /// Every bank ZFSBootMenu can extend.
    pub const ALL: [Bank; 4] = [Bank::Sha1, Bank::Sha256, Bank::Sha384, Bank::Sha512];

    /// Its name, as the TPM tools and CEL-JSON write it.
    pub fn name(self) -> &'static str {
        match self {
            Bank::Sha1 => "sha1",
            Bank::Sha256 => "sha256",
            Bank::Sha384 => "sha384",
            Bank::Sha512 => "sha512",
        }
    }

    /// The bank named `name`.
    pub fn named(name: &str) -> Option<Bank> {
        Bank::ALL.into_iter().find(|bank| bank.name() == name)
    }

    /// The length of its values, in bytes.
    pub fn length(self) -> usize {
        match self {
            Bank::Sha1 => 20,
            Bank::Sha256 => 32,
            Bank::Sha384 => 48,
            Bank::Sha512 => 64,
        }
    }

    /// The digest of `parts`, one after the other.
    fn digest(self, parts: &[&[u8]]) -> Vec<u8> {
        fn of<D: sha2::Digest>(parts: &[&[u8]]) -> Vec<u8> {
            let mut hasher = D::new();
            for part in parts {
                hasher.update(part);
            }
            hasher.finalize().to_vec()
        }
        match self {
            Bank::Sha1 => of::<Sha1>(parts),
            Bank::Sha256 => of::<Sha256>(parts),
            Bank::Sha384 => of::<Sha384>(parts),
            Bank::Sha512 => of::<Sha512>(parts),
        }
    }
}

/// The digests of one measured thing, one for each bank.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Digests(Vec<(Bank, Vec<u8>)>);

impl Digests {
    /// The digests `values`, a value for each bank.
    pub fn new(values: Vec<(Bank, Vec<u8>)>) -> Digests {
        Digests(values)
    }

    /// The digests of `bytes`.
    pub fn of_bytes(bytes: &[u8]) -> Digests {
        Digests(
            Bank::ALL
                .into_iter()
                .map(|bank| (bank, bank.digest(&[bytes])))
                .collect(),
        )
    }

    /// The digest for `bank`.
    pub fn get(&self, bank: Bank) -> Option<&[u8]> {
        self.0
            .iter()
            .find(|(each, _)| *each == bank)
            .map(|(_, digest)| digest.as_slice())
    }
}

/// What an event measured.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Measured {
    /// The kernel, at this path inside the boot environment.
    Kernel(String),
    /// The initramfs, at this path inside the boot environment.
    Initramfs(String),
    /// The command line the kernel gets.
    CommandLine(String),
    /// The separator, [`UNSEALED`].
    Unsealed,
    /// What this PCR held before ZFSBootMenu extended it, read from the
    /// TPM: no extension, but where the replay starts; its digests are the
    /// PCR's values.
    Start(u32),
}

impl Measured {
    /// Its name in a record.
    pub fn kind(&self) -> &'static str {
        match self {
            Measured::Kernel(_) => "kernel",
            Measured::Initramfs(_) => "initramfs",
            Measured::CommandLine(_) => "command-line",
            Measured::Unsealed => "separator",
            Measured::Start(_) => "start",
        }
    }

    /// What it names: a path, the command line, or the separator's text;
    /// nothing for a start.
    pub fn value(&self) -> &str {
        match self {
            Measured::Kernel(value) | Measured::Initramfs(value) | Measured::CommandLine(value) => {
                value
            }
            Measured::Unsealed => UNSEALED,
            Measured::Start(_) => "",
        }
    }

    /// The PCR it goes into.
    pub fn pcr(&self) -> u32 {
        match self {
            Measured::CommandLine(_) => COMMAND_LINE_PCR,
            Measured::Start(pcr) => *pcr,
            _ => FILES_PCR,
        }
    }
}

/// One extension: what was measured, and its digests.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Event {
    /// What was measured.
    pub measured: Measured,
    /// Its digests.
    pub digests: Digests,
}

impl Event {
    /// The command line `text`, its bytes without a terminating NUL.
    pub fn command_line(text: &str) -> Event {
        Event {
            digests: Digests::of_bytes(text.as_bytes()),
            measured: Measured::CommandLine(text.to_owned()),
        }
    }

    /// The separator.
    pub fn unsealed() -> Event {
        Event {
            digests: Digests::of_bytes(UNSEALED.as_bytes()),
            measured: Measured::Unsealed,
        }
    }

    /// What `pcr` held before ZFSBootMenu extended it: `values`.
    pub fn start(pcr: u32, values: Digests) -> Event {
        Event {
            measured: Measured::Start(pcr),
            digests: values,
        }
    }

    /// The PCR it goes into.
    pub fn pcr(&self) -> u32 {
        self.measured.pcr()
    }
}

/// The value of `pcr` in `bank` after `events`, as the TPM computes it:
/// from all zeros, or the value of a start; each extension is the digest
/// of the old value and the event's.
pub fn replay(events: &[Event], pcr: u32, bank: Bank) -> Vec<u8> {
    events
        .iter()
        .filter(|event| event.pcr() == pcr)
        .filter_map(|event| Some((event, event.digests.get(bank)?)))
        .fold(vec![0; bank.length()], |value, (event, digest)| {
            if matches!(event.measured, Measured::Start(_)) {
                digest.to_vec()
            } else {
                bank.digest(&[&value, digest])
            }
        })
}

/// The lower-case hexadecimal of `bytes`.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

/// The bytes of the hexadecimal `text`, if it is that.
pub(crate) fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(text.get(at..at + 2)?, 16).ok())
        .collect()
}

/// The CEL-JSON record numbered `recnum` of `event`, with its digests in
/// `banks`, as one JSON text of a sequence: the record separator, the
/// record, a line feed.
pub fn record(recnum: u64, event: &Event, banks: &[Bank]) -> String {
    let digests: Vec<Value> = banks
        .iter()
        .filter_map(|&bank| {
            event
                .digests
                .get(bank)
                .map(|digest| json!({"hashAlg": bank.name(), "digest": hex(digest)}))
        })
        .collect();
    let value = json!({
        "recnum": recnum,
        "pcr": event.pcr(),
        "digests": digests,
        "content_type": CONTENT_TYPE,
        "content": {"event": event.measured.kind(), "value": event.measured.value()},
    });
    format!("{RS}{value}\n")
}

/// The events of the log `text`, with the digests it gives, in order.
///
/// # Errors
///
/// The first record that is none of ZFSBootMenu's, by its number in the
/// log, from 0.
pub fn parse_log(text: &str) -> Result<Vec<Event>, usize> {
    text.split(RS)
        .map(str::trim)
        .filter(|record| !record.is_empty())
        .enumerate()
        .map(|(index, record)| parse_record(record).ok_or(index))
        .collect()
}

/// The event of one record.
fn parse_record(record: &str) -> Option<Event> {
    let value: Value = serde_json::from_str(record).ok()?;
    if value.get("content_type")?.as_str()? != CONTENT_TYPE {
        return None;
    }
    let content = value.get("content")?;
    let text = content.get("value")?.as_str()?.to_owned();
    let measured = match content.get("event")?.as_str()? {
        "kernel" => Measured::Kernel(text),
        "initramfs" => Measured::Initramfs(text),
        "command-line" => Measured::CommandLine(text),
        "separator" if text == UNSEALED => Measured::Unsealed,
        "start" if text.is_empty() => Measured::Start(0),
        _ => return None,
    };
    let pcr = u32::try_from(value.get("pcr")?.as_u64()?).ok()?;
    let measured = match measured {
        Measured::Start(_) => Measured::Start(pcr),
        measured => measured,
    };
    if ![COMMAND_LINE_PCR, FILES_PCR].contains(&pcr) || pcr != measured.pcr() {
        return None;
    }
    let digests = value
        .get("digests")?
        .as_array()?
        .iter()
        .map(|digest| {
            let bank = Bank::named(digest.get("hashAlg")?.as_str()?)?;
            let bytes = unhex(digest.get("digest")?.as_str()?)?;
            (bytes.len() == bank.length()).then_some((bank, bytes))
        })
        .collect::<Option<Vec<_>>>()?;
    Some(Event {
        measured,
        digests: Digests(digests),
    })
}

/// A cpio archive (`newc`) holding `contents` at `path`, relative to the
/// root, with its parent directories, ended by its trailer: the kernel
/// unpacks it after the initramfs it is appended to.
pub fn cpio(path: &str, contents: &[u8]) -> Vec<u8> {
    let mut archive = Vec::new();
    let mut number = 1;
    let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    for end in 1..parts.len() {
        let parent = parts.get(..end).unwrap_or_default().join("/");
        entry(&mut archive, number, &parent, 0o040_755, &[]);
        number += 1;
    }
    entry(&mut archive, number, &parts.join("/"), 0o100_644, contents);
    entry(&mut archive, 0, "TRAILER!!!", 0, &[]);
    pad(&mut archive, 512);
    archive
}

/// Appends one `newc` entry to `archive`.
fn entry(archive: &mut Vec<u8>, inode: u32, name: &str, mode: u32, contents: &[u8]) {
    let links = if mode & 0o040_000 == 0 { 1 } else { 2 };
    let size = u32::try_from(contents.len()).unwrap_or(u32::MAX);
    let name_size = u32::try_from(name.len() + 1).unwrap_or(u32::MAX);
    // magic, inode, mode, uid, gid, links, mtime, size, the devices, the
    // name's size and the checksum, in eight hexadecimal digits each.
    let fields = [inode, mode, 0, 0, links, 0, size, 0, 0, 0, 0, name_size, 0];
    archive.extend_from_slice(b"070701");
    for field in fields {
        archive.extend_from_slice(format!("{field:08x}").as_bytes());
    }
    archive.extend_from_slice(name.as_bytes());
    archive.push(0);
    pad(archive, 4);
    archive.extend_from_slice(contents);
    pad(archive, 4);
}

/// Pads `archive` with zeros to a multiple of `block`.
fn pad(archive: &mut Vec<u8>, block: usize) {
    while !archive.len().is_multiple_of(block) {
        archive.push(0);
    }
}

#[cfg(test)]
mod tests;
