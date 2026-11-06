//! `zpool status -j -p -P` (OpenZFS 2.3 and later): the `pools` map, from
//! names to pools, each with its `state`, the `status` and `action` of a
//! pool with a problem, `error_count`, and `vdevs`, a map holding the root
//! vdev, whose `vdevs` are the top-level vdevs of the data. The vdevs of
//! the other classes are in maps of their own beside it (`dedup`,
//! `special`, `logs`, `l2cache`, `spares`). Each vdev has a `name`, a
//! `vdev_type`, a `state`, error counts (strings, or numbers with
//! `--json-int`), the path a missing device had (`was`) or why it cannot
//! be used (`aux`), and `vdevs` below it.
//!
//! The `state` of a vdev is the name of the kernel's state, such as
//! `CANT_OPEN`, where the text has the word `zpool` makes of it and `aux`,
//! such as `UNAVAIL`; the reason in `aux`, such as `OPEN_FAILED`, is a
//! note in the text, such as `cannot open`. Both are read here as the text
//! gives them (`zpool_state_to_name` and `print_status_config` in
//! OpenZFS). The notes the text adds after the reason, such as
//! `(resilvering)`, have no counterpart.
//!
//! The workspace's `serde_json` keeps no order in its maps, so the maps
//! here are read with a visitor that keeps the order of the document,
//! which is the order of the text.

use std::fmt;
use std::marker::PhantomData;

use serde::Deserialize;
use serde::de::{Deserializer, MapAccess, Visitor};
use zbm_types::Untrusted;

use super::{JsonError, document, invalid};
use crate::model::{Health, PoolStatus, Vdev, VdevClass, VdevErrors, VdevKind};
use crate::rows::Readable as _;

/// The entries of a JSON object, in the order of the document.
struct Ordered<T>(Vec<(String, T)>);

impl<T> Default for Ordered<T> {
    fn default() -> Ordered<T> {
        Ordered(Vec::new())
    }
}

/// Reads an object into [`Ordered`].
struct Entries<T>(PhantomData<T>);

impl<'de, T: Deserialize<'de>> Visitor<'de> for Entries<T> {
    type Value = Ordered<T>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Ordered<T>, A::Error> {
        let mut entries = Vec::new();
        while let Some(entry) = map.next_entry()? {
            entries.push(entry);
        }
        Ok(Ordered(entries))
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Ordered<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Ordered<T>, D::Error> {
        deserializer.deserialize_map(Entries(PhantomData))
    }
}

/// A count, as a string or, with `--json-int`, a number.
#[derive(Deserialize)]
#[serde(untagged)]
enum Count {
    Number(u64),
    Text(String),
}

impl Count {
    fn value(&self) -> Option<u64> {
        match self {
            Count::Number(number) => Some(*number),
            Count::Text(text) => text.parse().ok(),
        }
    }
}

#[derive(Deserialize)]
struct RawVdev {
    name: String,
    #[serde(default)]
    vdev_type: String,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    read_errors: Option<Count>,
    #[serde(default)]
    write_errors: Option<Count>,
    #[serde(default)]
    checksum_errors: Option<Count>,
    #[serde(default)]
    slow_ios: Option<Count>,
    /// The path a missing device had.
    #[serde(default)]
    was: Option<String>,
    /// Why the vdev cannot be used, such as `OPEN_FAILED`.
    #[serde(default)]
    aux: Option<String>,
    /// The pool that uses a spare, when another pool does.
    #[serde(default)]
    used_by: Option<String>,
    #[serde(default)]
    vdevs: Ordered<RawVdev>,
}

#[derive(Deserialize)]
struct RawStatus {
    name: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    error_count: Option<Count>,
    #[serde(default)]
    vdevs: Ordered<RawVdev>,
    #[serde(default)]
    dedup: Ordered<RawVdev>,
    #[serde(default)]
    special: Ordered<RawVdev>,
    #[serde(default)]
    logs: Ordered<RawVdev>,
    #[serde(default)]
    l2cache: Ordered<RawVdev>,
    #[serde(default)]
    spares: Ordered<RawVdev>,
}

#[derive(Deserialize)]
struct Statuses {
    #[serde(default)]
    pools: Ordered<RawStatus>,
}

/// The word of the text for a vdev's `state` and `aux`.
fn state_word<'a>(state: &'a str, aux: Option<&str>) -> &'a str {
    match (state, aux) {
        ("CLOSED", _) => "OFFLINE",
        ("CANT_OPEN", Some("CORRUPT_DATA" | "BAD_LOG")) => "FAULTED",
        ("CANT_OPEN", Some("SPLIT_POOL")) => "SPLIT",
        ("CANT_OPEN", _) => "UNAVAIL",
        (other, _) => other,
    }
}

/// The note of the text for `aux`, the reason a vdev cannot be used;
/// `slow` when its only errors are slow I/Os, `used_by` the pool that uses
/// it as a spare.
fn aux_note(aux: &str, slow: bool, used_by: Option<&str>) -> String {
    let note = match aux {
        "OPEN_FAILED" => "cannot open",
        "BAD_GUID_SUM" => "missing device",
        "NO_REPLICAS" => "insufficient replicas",
        "VERSION_NEWER" => "newer version",
        "UNSUP_FEAT" => "unsupported feature(s)",
        "ASHIFT_TOO_BIG" => "unsupported minimum blocksize",
        "SPARED" => {
            return used_by.map_or_else(
                || "currently in use".to_owned(),
                |pool| format!("in use by pool '{pool}'"),
            );
        }
        "ERR_EXCEEDED" if slow => "too many slow I/Os",
        "ERR_EXCEEDED" => "too many errors",
        "IO_FAILURE" => "experienced I/O failures",
        "BAD_LOG" => "bad intent log",
        "EXTERNAL" => "external device fault",
        "SPLIT_POOL" => "split into new pool",
        "ACTIVE" => "currently in use",
        "CHILDREN_OFFLINE" => "all children offline",
        "BAD_LABEL" => "invalid label",
        _ => "corrupted data",
    };
    note.to_owned()
}

/// Text of `zpool` on one line, as the text format's reader joins it.
fn one_line(text: Option<String>) -> Option<Untrusted<String>> {
    let text = text?;
    let joined = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    (!joined.is_empty()).then(|| Untrusted::new(joined))
}

impl RawVdev {
    /// The vdev, in `class`, with the vdevs below it.
    fn into_vdev(self, class: VdevClass) -> Vdev {
        let count = |count: Option<&Count>| count.and_then(Count::value);
        let errors = match (
            count(self.read_errors.as_ref()),
            count(self.write_errors.as_ref()),
            count(self.checksum_errors.as_ref()),
        ) {
            (Some(read), Some(write), Some(cksum)) => Some(VdevErrors { read, write, cksum }),
            _ => None,
        };
        let children: Vec<Vdev> = self
            .vdevs
            .0
            .into_iter()
            .map(|(_, child)| child.into_vdev(class))
            .collect();
        let aux = self.aux.as_deref();
        let slow = children.is_empty()
            && errors.is_some_and(|errors| errors == VdevErrors::default())
            && count(self.slow_ios.as_ref()).is_some_and(|slow| slow > 0);
        let note = match (self.was, aux) {
            (Some(path), _) => Some(format!("was {path}")),
            (None, Some(aux)) => Some(aux_note(aux, slow, self.used_by.as_deref())),
            (None, None) => None,
        };
        Vdev {
            name: Untrusted::new(self.name),
            kind: VdevKind::of_type(&self.vdev_type, !children.is_empty()),
            class,
            state: self
                .state
                .as_deref()
                .map(|state| Health::parse(state_word(state, aux))),
            errors,
            note: note.map(Untrusted::new),
            children,
        }
    }
}

impl RawStatus {
    /// The top-level vdevs: those below the root, the data's, then those
    /// of each class, in the order of the text.
    fn top_level(self) -> Vec<Vdev> {
        let mut top: Vec<Vdev> = Vec::new();
        for (_, vdev) in self.vdevs.0 {
            if vdev.vdev_type == "root" {
                top.extend(
                    vdev.vdevs
                        .0
                        .into_iter()
                        .map(|(_, child)| child.into_vdev(VdevClass::Data)),
                );
            } else {
                top.push(vdev.into_vdev(VdevClass::Data));
            }
        }
        let classes = [
            (VdevClass::Dedup, self.dedup),
            (VdevClass::Special, self.special),
            (VdevClass::Log, self.logs),
            (VdevClass::Cache, self.l2cache),
            (VdevClass::Spare, self.spares),
        ];
        for (class, vdevs) in classes {
            top.extend(vdevs.0.into_iter().map(|(_, vdev)| vdev.into_vdev(class)));
        }
        top
    }
}

/// `zpool status -j -p -P`: each imported pool and its vdevs, in the
/// order printed.
///
/// # Errors
///
/// [`JsonError`] for a document of another shape, or when no pool has a
/// valid name.
pub fn status(text: &str) -> Result<Vec<PoolStatus>, JsonError> {
    let document: Statuses = document(text)?;
    document
        .pools
        .0
        .into_iter()
        .map(|(_, mut raw)| {
            let name = raw
                .name
                .parse()
                .map_err(|_| invalid("pool name", &raw.name))?;
            let state = Health::parse(&raw.state);
            let errors = raw.error_count.as_ref().and_then(Count::value);
            let status = one_line(raw.status.take());
            let action = one_line(raw.action.take());
            Ok(PoolStatus {
                name,
                state,
                status,
                action,
                errors,
                vdevs: raw.top_level(),
            })
        })
        .readable()
}

#[cfg(test)]
mod tests;
