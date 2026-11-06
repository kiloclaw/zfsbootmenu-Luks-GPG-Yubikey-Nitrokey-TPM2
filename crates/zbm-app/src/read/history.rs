//! The boot history ([`Action::LoadHistory`]): the boots whose logs
//! `zbm.save_logs` kept on the EFI system partition, newest first, each
//! with its record when it has one ([`zbm_boot::saved_logs::history`]).
//!
//! Without the option the partition is not read at all: the history is
//! [`History::Off`]. With it, the partition is mounted read-only at a
//! mount point of its own, and unmounted by a runner nothing cancels, even
//! when the read is cancelled. What is read is bounded, so that a
//! crowded partition cannot hold the menu up nor outgrow a message to a
//! remote menu: at most [`READ_BOOTS`] boots of [`FILES_READ`] files each,
//! and records of [`BootRecord::MAX_LEN`] bytes, which escaped as JSON
//! stay below [`MAX_LENGTH`](crate::remote::MAX_LENGTH). Every name and
//! text read stays [`Untrusted`].
//!
//! [`Action::LoadHistory`]: crate::model::Action::LoadHistory
//! [`BootRecord::MAX_LEN`]: zbm_types::BootRecord::MAX_LEN
//! [`READ_BOOTS`]: zbm_boot::saved_logs::READ_BOOTS
//! [`FILES_READ`]: zbm_boot::saved_logs::FILES_READ

use zbm_boot::saved_logs::{self, Destination, PastBoot};
use zbm_platform::{Context, Runner};
use zbm_types::Untrusted;
use zbm_types::code::Coded as _;

use crate::model::{History, Recorded, SavedBoot};

/// The boot history on `context`'s machine, whose logs go to `save_logs`,
/// if anywhere; the partition is unmounted by `keep`.
pub(crate) fn history(
    context: &Context<'_>,
    save_logs: Option<&Destination>,
    keep: &dyn Runner,
) -> History {
    let Some(destination) = save_logs else {
        return History::Off;
    };
    match saved_logs::history(context, destination, keep) {
        Ok(boots) => History::Saved(boots.into_iter().map(saved_boot).collect()),
        Err(error) => History::Unavailable {
            why: zbm_types::code::chain(&error),
            code: error.code().number(),
        },
    }
}

/// `boot` as the menu shows it.
fn saved_boot(boot: PastBoot) -> SavedBoot {
    SavedBoot {
        name: Untrusted::new(boot.saved.name),
        files: boot
            .saved
            .files
            .into_iter()
            .map(|(name, size)| (Untrusted::new(name), size))
            .collect(),
        record: match boot.record {
            None => Recorded::Missing,
            Some(Ok(record)) => Recorded::Read(record),
            Some(Err(why)) => Recorded::Unreadable(why.to_string()),
        },
    }
}
