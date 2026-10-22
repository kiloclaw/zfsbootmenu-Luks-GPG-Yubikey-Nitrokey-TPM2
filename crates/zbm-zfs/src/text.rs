//! The text that `zfs` and `zpool` print with `-H -p`: one line per row,
//! fields separated by tabs, numbers in full. Versions before OpenZFS 2.3
//! have only this format.

use std::collections::BTreeMap;

use crate::model::{Dataset, Health, Pool, Property, Snapshot, Source, Version};

/// What was wrong with a line of output.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("{reason} in {line:?}")]
pub struct TextError {
    /// The line.
    pub line: String,
    /// What was wrong with it.
    pub reason: &'static str,
}

impl TextError {
    fn new(line: &str, reason: &'static str) -> TextError {
        TextError {
            line: line.to_owned(),
            reason,
        }
    }
}

/// The fields of each line.
fn rows(text: &str) -> impl Iterator<Item = (&str, Vec<&str>)> {
    text.lines()
        .filter(|line| !line.is_empty())
        .map(|line| (line, line.split('\t').collect()))
}

/// `zfs version`: the tools on the first line, the kernel module on the
/// second, if it is loaded.
///
/// # Errors
///
/// [`TextError`] when the first line does not name a `zfs-` release.
pub fn version(text: &str) -> Result<Version, TextError> {
    let mut lines = text.lines().map(str::trim);
    let userland = lines.next().unwrap_or_default();
    if !userland.starts_with("zfs-") {
        return Err(TextError::new(userland, "no zfs release"));
    }
    Ok(Version {
        userland: userland.to_owned(),
        kernel: lines.next().unwrap_or_default().to_owned(),
    })
}

/// `zpool list -H -p -o name,health,readonly,bootfs,guid`.
///
/// # Errors
///
/// [`TextError`] for a line without five fields or with an invalid name or
/// identifier.
pub fn pools(text: &str) -> Result<Vec<Pool>, TextError> {
    rows(text)
        .map(|(line, fields)| {
            let [name, health, read_only, bootfs, guid] = fields.as_slice() else {
                return Err(TextError::new(line, "not five fields"));
            };
            Ok(Pool {
                name: name
                    .parse()
                    .map_err(|_| TextError::new(line, "invalid pool name"))?,
                health: Health::parse(health),
                read_only: *read_only == "on",
                bootfs: match *bootfs {
                    "-" | "" => None,
                    bootfs => Some(
                        bootfs
                            .parse()
                            .map_err(|_| TextError::new(line, "invalid bootfs"))?,
                    ),
                },
                guid: guid
                    .parse()
                    .map_err(|_| TextError::new(line, "invalid guid"))?,
            })
        })
        .collect()
}

/// `zfs get` or `zpool get -H -p -o name,property,value,source`: the
/// property of each name.
///
/// # Errors
///
/// [`TextError`] for a line without four fields.
pub fn properties(text: &str) -> Result<Vec<(String, String, Property)>, TextError> {
    rows(text)
        .map(|(line, fields)| {
            let [name, property, value, source] = fields.as_slice() else {
                return Err(TextError::new(line, "not four fields"));
            };
            Ok((
                (*name).to_owned(),
                (*property).to_owned(),
                Property::new(*value, Source::parse(source)),
            ))
        })
        .collect()
}

/// `zfs list -H -p -o name,<properties>`: each dataset with `properties`, in
/// that order. Sources are not printed; they are [`Source::None`].
///
/// # Errors
///
/// [`TextError`] for a line with another number of fields or an invalid
/// name.
pub fn datasets(text: &str, properties: &[&str]) -> Result<Vec<Dataset>, TextError> {
    rows(text)
        .map(|(line, fields)| {
            let Some((name, values)) = fields.split_first() else {
                return Err(TextError::new(line, "no fields"));
            };
            if values.len() != properties.len() {
                return Err(TextError::new(line, "wrong number of fields"));
            }
            let properties: BTreeMap<String, Property> = properties
                .iter()
                .zip(values)
                .map(|(property, value)| {
                    ((*property).to_owned(), Property::new(*value, Source::None))
                })
                .collect();
            Ok(Dataset {
                name: name
                    .parse()
                    .map_err(|_| TextError::new(line, "invalid dataset name"))?,
                properties,
            })
        })
        .collect()
}

/// `zfs list -H -p -t snapshot -o name,creation`.
///
/// # Errors
///
/// [`TextError`] for a line without two fields, an invalid name or a
/// creation time that is not a number.
pub fn snapshots(text: &str) -> Result<Vec<Snapshot>, TextError> {
    rows(text)
        .map(|(line, fields)| {
            let [name, creation] = fields.as_slice() else {
                return Err(TextError::new(line, "not two fields"));
            };
            Ok(Snapshot {
                name: name
                    .parse()
                    .map_err(|_| TextError::new(line, "invalid snapshot name"))?,
                creation: creation
                    .parse()
                    .map_err(|_| TextError::new(line, "invalid creation time"))?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_lines_are_errors_that_quote_them() {
        assert_eq!(
            version("ZFS 2.4\n").unwrap_err().to_string(),
            "no zfs release in \"ZFS 2.4\""
        );
        assert_eq!(version("zfs-2.1.5\n").unwrap().kernel, "");
        assert!(pools("tank\tONLINE\n").is_err());
        assert!(pools("mirror\tONLINE\toff\t-\t1\n").is_err());
        assert!(pools("tank\tONLINE\toff\ttank/\t1\n").is_err());
        assert!(pools("tank\tONLINE\toff\t-\tx\n").is_err());
        assert!(
            pools("tank\tONLINE\toff\t\t1\n").unwrap()[0]
                .bootfs
                .is_none()
        );
        assert!(properties("tank\treadonly\n").is_err());
        assert!(datasets("tank\t/\n", &["mountpoint", "canmount"]).is_err());
        assert!(datasets("tank/\t/\n", &["mountpoint"]).is_err());
        assert!(snapshots("tank@a\n").is_err());
        assert!(snapshots("tank\t1\n").is_err());
        assert!(snapshots("tank@a\tsoon\n").is_err());
    }
}
