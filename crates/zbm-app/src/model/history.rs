//! The boot history (ADR 0016, point 7; ROADMAP.md #9): the boots whose
//! logs `zbm.save_logs` kept on the EFI system partition, as
//! [`Action::LoadHistory`](super::Action::LoadHistory) reads them.
//!
//! The partition is a FAT file system anyone with the disk can write: the
//! names of the saved boots and of their files, and every text of their
//! records, are [`Untrusted`].

use zbm_types::{BootRecord, DatasetName, Untrusted};

/// The boot history.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum History {
    /// No history is kept: `zbm.save_logs` is not given, and the partition
    /// is not read.
    Off,
    /// The partition could not be found or mounted.
    Unavailable {
        /// Why, for people.
        why: String,
        /// The error's code, for `zbm explain`: 624 for `ZBM-E0624`.
        code: u16,
    },
    /// The saved boots, newest first: the newest
    /// [`READ_BOOTS`](zbm_boot::saved_logs::READ_BOOTS) of them.
    Saved(Vec<SavedBoot>),
}

/// A saved boot.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SavedBoot {
    /// Its directory's name, the time of the save in UTC, such as
    /// `2026-10-24T01-29-15Z`.
    pub name: Untrusted<String>,
    /// Its files and their sizes in bytes, by name: the first
    /// [`FILES_READ`](zbm_boot::saved_logs::FILES_READ) of them.
    pub files: Vec<(Untrusted<String>, u64)>,
    /// What its `boot.json` says.
    pub record: Recorded,
}

/// What a saved boot's `boot.json` says.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Recorded {
    /// It has none: it was saved from the shell or after a crash, or
    /// before records were written.
    Missing,
    /// It has one this runtime does not read, and why, which never quotes
    /// the record.
    Unreadable(String),
    /// Its record.
    Read(BootRecord),
}

impl History {
    /// The newest record of a boot of `environment`, if the history has
    /// one.
    pub fn last_boot_of(&self, environment: &DatasetName) -> Option<&BootRecord> {
        let History::Saved(boots) = self else {
            return None;
        };
        boots.iter().find_map(|boot| match &boot.record {
            Recorded::Read(record) if record.environment.get() == environment.as_str() => {
                Some(record)
            }
            _ => None,
        })
    }
}
