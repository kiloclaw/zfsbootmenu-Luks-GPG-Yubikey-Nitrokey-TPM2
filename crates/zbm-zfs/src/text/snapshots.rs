//! The text a review of a change to snapshots reads: the snapshots and
//! bookmarks of a dataset with what keeps them (`zfs list -o
//! name,createtxg,used,userrefs,clones`), the holds on snapshots (`zfs
//! holds -H -p`), and the space a destroy would free (`zfs destroy -n -v
//! -p`).

use zbm_types::{SnapshotName, Untrusted};

use super::{TextError, rows};
use crate::model::{Hold, SnapshotDetail, SnapshotOrBookmark};
use crate::rows::Readable as _;

/// A number of `zfs list -p`, where `-` is none.
fn optional_number(
    field: &str,
    line: &str,
    reason: &'static str,
) -> Result<Option<u64>, TextError> {
    match field {
        "-" => Ok(None),
        number => number
            .parse()
            .map(Some)
            .map_err(|_| TextError::new(line, reason)),
    }
}

/// The name of a snapshot, or of a bookmark (`<dataset>#<name>`), whose
/// own name has the characters a snapshot's has.
fn snapshot_or_bookmark(name: &str, line: &str) -> Result<SnapshotOrBookmark, TextError> {
    let invalid = |_| TextError::new(line, "invalid snapshot or bookmark name");
    if name.contains('@') {
        return Ok(SnapshotOrBookmark::Snapshot(name.parse().map_err(invalid)?));
    }
    let checked: SnapshotName = name.replacen('#', "@", 1).parse().map_err(invalid)?;
    Ok(SnapshotOrBookmark::Bookmark {
        dataset: checked.dataset().clone(),
        name: checked.name().to_owned(),
    })
}

/// `zfs list -H -p -t snapshot,bookmark -o
/// name,createtxg,used,userrefs,clones`: each snapshot and bookmark, in
/// the order listed. A bookmark has `-` for its space, holds and clones;
/// the clones of a snapshot are names separated by commas, `-` or nothing
/// when it has none.
///
/// # Errors
///
/// [`TextError`] when no row could be read, for a line without five
/// fields, an invalid name, or a transaction group, a size or a count that
/// is not a number.
pub fn snapshot_details(text: &str) -> Result<Vec<SnapshotDetail>, TextError> {
    rows(text)
        .map(|(line, fields)| {
            let [name, createtxg, used, userrefs, clones] = fields.as_slice() else {
                return Err(TextError::new(line, "not five fields"));
            };
            Ok(SnapshotDetail {
                name: snapshot_or_bookmark(name, line)?,
                createtxg: createtxg
                    .parse()
                    .map_err(|_| TextError::new(line, "invalid transaction group"))?,
                used: optional_number(used, line, "invalid size")?,
                userrefs: optional_number(userrefs, line, "invalid number of holds")?
                    .unwrap_or_default(),
                clones: clones
                    .split(',')
                    .filter(|clone| !clone.is_empty() && *clone != "-")
                    .map(|clone| Untrusted::new(clone.to_owned()))
                    .collect(),
            })
        })
        .readable()
}

/// `zfs holds -H -p`: the snapshot, the tag and the time of each hold. A
/// tag may hold tabs: it is every field between the name and the time.
///
/// # Errors
///
/// [`TextError`] when no row could be read, for a line without three
/// fields, an invalid snapshot name or a time that is not a number.
pub fn holds(text: &str) -> Result<Vec<Hold>, TextError> {
    rows(text)
        .map(|(line, fields)| {
            let [name, tag @ .., time] = fields.as_slice() else {
                return Err(TextError::new(line, "not three fields"));
            };
            if tag.is_empty() {
                return Err(TextError::new(line, "not three fields"));
            }
            time.parse::<u64>()
                .map_err(|_| TextError::new(line, "invalid time"))?;
            Ok(Hold {
                snapshot: name
                    .parse()
                    .map_err(|_| TextError::new(line, "invalid snapshot name"))?,
                tag: Untrusted::new(tag.join("\t")),
            })
        })
        .readable()
}

/// `zfs destroy -n -v -p`: the space, in bytes, that destroying the
/// snapshots it names (`destroy` lines) would free (`reclaim`).
///
/// # Errors
///
/// [`TextError`] for text without a `reclaim` line, or with one whose
/// space is not a number.
pub fn reclaim(text: &str) -> Result<u64, TextError> {
    let (line, space) = rows(text)
        .find_map(|(line, fields)| match fields.as_slice() {
            ["reclaim", space] => Some((line, *space)),
            _ => None,
        })
        .ok_or_else(|| TextError::new(text.lines().next().unwrap_or_default(), "no reclaim"))?;
    space
        .parse()
        .map_err(|_| TextError::new(line, "invalid space"))
}

#[cfg(test)]
mod tests;
