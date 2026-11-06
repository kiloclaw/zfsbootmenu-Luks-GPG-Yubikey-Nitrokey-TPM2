//! The record of a boot that the saved logs keep beside it, as `boot.json`
//! (ADR 0016, point 7; ROADMAP.md #9): what was booted, from which boot
//! environment, with which command line, and how it was loaded.
//!
//! The file is one JSON object with `"schema": 1`. Fields may be added,
//! never changed: a reader ignores the fields it does not know, and refuses
//! a schema it does not know. The ESP holds it, a FAT file system anyone
//! with the disk can write (input I15 of `docs/dev/threat-model.md`), so
//! what is read back is capped in size, and every text in it is
//! [`Untrusted`], shown only as [`Printable`](crate::Printable).
//!
//! ```
//! use zbm_types::{BootRecord, Untrusted};
//!
//! let record = BootRecord {
//!     time: 1_793_246_955,
//!     environment: Untrusted::new("zroot/ROOT/void".to_owned()),
//!     kernel: Untrusted::new("/boot/vmlinuz-6.12.9_1".to_owned()),
//!     initramfs: Untrusted::new("/boot/initramfs-6.12.9_1.img".to_owned()),
//!     command_line: Untrusted::new("root=zfs:zroot/ROOT/void".to_owned()),
//!     devicetree: None,
//!     loaded_by: Untrusted::new("kexec_file_load".to_owned()),
//!     measured: false,
//!     command_line_edited: false,
//! };
//! let text = record.to_json()?;
//! assert!(text.starts_with(r#"{"schema":1,"time":1793246955,"#));
//! assert_eq!(BootRecord::parse(text.as_bytes()), Ok(record));
//! # Ok::<(), std::io::Error>(())
//! ```

use serde::{Deserialize, Serialize};

use crate::Untrusted;

/// What the saved logs of a boot say about it, in `boot.json`.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct BootRecord {
    /// When the logs were saved, in seconds since the epoch (UTC): the time
    /// the name of their directory gives.
    pub time: u64,
    /// The boot environment booted, as its dataset's name.
    pub environment: Untrusted<String>,
    /// The kernel, as a path inside the boot environment.
    pub kernel: Untrusted<String>,
    /// The initramfs, as a path inside the boot environment.
    pub initramfs: Untrusted<String>,
    /// The kernel's command line, as it was loaded.
    pub command_line: Untrusted<String>,
    /// The device tree loaded with the kernel, if any, as a path inside the
    /// boot environment: `null`, or left out, when there was none.
    pub devicetree: Option<Untrusted<String>>,
    /// What loaded the kernel: `kexec_file_load`, or the `kexec` program.
    pub loaded_by: Untrusted<String>,
    /// Whether the kernel and the initramfs loaded were the copies measured
    /// into the TPM with the command line (ADR 0008). False without a TPM,
    /// and when the files or their copies could not be read or written,
    /// though the TPM may then hold their events; an extension the TPM
    /// refused is left out of the event log saved beside, `tpm-measure.json`,
    /// which says what the TPM took.
    pub measured: bool,
    /// Whether the command line was typed in the menu for this boot only.
    pub command_line_edited: bool,
}

/// Why a `boot.json` is not a record this runtime reads.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum RecordError {
    /// The file is longer than a record can be.
    #[error("the record is longer than {max} bytes")]
    TooLong {
        /// The longest a record may be, in bytes.
        max: usize,
    },
    /// The record has a schema this runtime does not know.
    #[error("the record has schema {0}, not {schema}", schema = BootRecord::SCHEMA)]
    Schema(u64),
    /// The file is not a record: not JSON, or a field other than
    /// `devicetree` is missing, or a field is of another type.
    #[error("the record cannot be read (line {line}, column {column})")]
    Invalid {
        /// The line of the error, from 1.
        line: usize,
        /// The column of the error, from 1, or 0 at the start of a line,
        /// as at the end of a file that ends early.
        column: usize,
    },
}

impl From<serde_json::Error> for RecordError {
    fn from(error: serde_json::Error) -> RecordError {
        RecordError::Invalid {
            line: error.line(),
            column: error.column(),
        }
    }
}

/// The record as it is written: its schema first.
#[derive(Serialize)]
struct Written<'a> {
    schema: u64,
    #[serde(flatten)]
    record: &'a BootRecord,
}

/// The schema of a record, read before the rest.
#[derive(Deserialize)]
struct Schema {
    schema: u64,
}

impl BootRecord {
    /// The schema this runtime writes and reads.
    pub const SCHEMA: u64 = 1;

    /// The longest `boot.json` [`BootRecord::parse`] reads, in bytes.
    pub const MAX_LEN: usize = 64 * 1024;

    /// The record as `boot.json` holds it: one line of JSON, with
    /// `"schema": 1` first and the fields in their order.
    ///
    /// # Errors
    ///
    /// None in practice: a record is text, numbers and booleans, which
    /// JSON always takes.
    pub fn to_json(&self) -> std::io::Result<String> {
        let written = Written {
            schema: BootRecord::SCHEMA,
            record: self,
        };
        serde_json::to_string(&written).map_err(std::io::Error::other)
    }

    /// The record `bytes` hold, as read from a `boot.json`. Fields it does
    /// not know are left out.
    ///
    /// # Errors
    ///
    /// When `bytes` are longer than [`BootRecord::MAX_LEN`], have another
    /// schema than [`BootRecord::SCHEMA`], or are not a record.
    pub fn parse(bytes: &[u8]) -> Result<BootRecord, RecordError> {
        if bytes.len() > BootRecord::MAX_LEN {
            return Err(RecordError::TooLong {
                max: BootRecord::MAX_LEN,
            });
        }
        let Schema { schema } = serde_json::from_slice(bytes)?;
        if schema != BootRecord::SCHEMA {
            return Err(RecordError::Schema(schema));
        }
        Ok(serde_json::from_slice(bytes)?)
    }
}

#[cfg(test)]
mod tests;
