//! The text that `zfs` and `zpool` print with `-H -p`: one line per row,
//! fields separated by tabs, numbers in full. Versions before OpenZFS 2.3
//! have only this format.

use std::collections::BTreeMap;

use zbm_types::{PoolName, SnapshotName};

use crate::model::{
    Dataset, DiffChange, Difference, Health, Pool, Property, Snapshot, Source, Version,
};
use crate::rows::Readable as _;

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

/// `zpool list -H -o name`: one pool name per line.
///
/// # Errors
///
/// [`TextError`] for an invalid pool name.
pub fn pool_names(text: &str) -> Result<Vec<PoolName>, TextError> {
    rows(text)
        .map(|(line, _)| {
            line.parse()
                .map_err(|_| TextError::new(line, "invalid pool name"))
        })
        .readable()
}

/// The device paths in `zpool status -L -P`: every field that starts with
/// `/`, in the order printed.
pub fn status_devices(text: &str) -> Vec<String> {
    text.split_whitespace()
        .filter(|field| field.starts_with('/'))
        .map(str::to_owned)
        .collect()
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
        .readable()
}

/// Whether `field` is what `zfs get` prints as a property's source.
fn is_source(field: &str) -> bool {
    matches!(field, "-" | "local" | "default" | "received" | "temporary")
        || field.starts_with("inherited from ")
}

/// `zfs get` or `zpool get -H -p -o name,property,value,source`: the
/// property of each name. A value may hold tabs and line breaks, such as a
/// command line written over several lines: a record ends at the first
/// line whose last field is a source, and its value is everything between
/// the property and the source.
///
/// # Errors
///
/// [`TextError`] for a record without four fields.
pub fn properties(text: &str) -> Result<Vec<(String, String, Property)>, TextError> {
    let mut records = Vec::new();
    let mut record = String::new();
    for line in text.lines() {
        if record.is_empty() && line.is_empty() {
            continue;
        }
        if !record.is_empty() {
            record.push('\n');
        }
        record.push_str(line);
        let fields: Vec<&str> = record.split('\t').collect();
        let (Some((source, rest)), true) = (fields.split_last(), fields.len() >= 4) else {
            continue;
        };
        if !is_source(source) {
            continue;
        }
        let (name, property, value) = match rest {
            [name, property, value @ ..] => (*name, *property, value.join("\t")),
            _ => continue,
        };
        records.push((
            name.to_owned(),
            property.to_owned(),
            Property::new(value, Source::parse(source)),
        ));
        record.clear();
    }
    if record.is_empty() {
        Ok(records)
    } else {
        Err(TextError::new(&record, "not four fields"))
    }
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
        .readable()
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
        .readable()
}

/// `zfs list -H -p -t snapshot -o name,used`.
///
/// # Errors
///
/// [`TextError`] for a line without two fields, an invalid name or a size
/// that is not a number.
pub fn snapshot_space(text: &str) -> Result<Vec<(SnapshotName, u64)>, TextError> {
    rows(text)
        .map(|(line, fields)| {
            let [name, used] = fields.as_slice() else {
                return Err(TextError::new(line, "not two fields"));
            };
            let name = name
                .parse()
                .map_err(|_| TextError::new(line, "invalid snapshot name"))?;
            let used = used
                .parse()
                .map_err(|_| TextError::new(line, "invalid size"))?;
            Ok((name, used))
        })
        .collect()
}

/// `zfs diff -F -H`: the change, the kind of file and the path of each
/// line, and the new path of a rename.
///
/// # Errors
///
/// [`TextError`] for a line with an unknown change or kind, or without the
/// fields of its change.
pub fn differences(text: &str) -> Result<Vec<Difference>, TextError> {
    rows(text)
        .map(|(line, fields)| {
            let (change, file_type, path, renamed_to) = match fields.as_slice() {
                [change, file_type, path] => (change, file_type, path, None),
                [change, file_type, path, to] => (change, file_type, path, Some(*to)),
                _ => return Err(TextError::new(line, "not three or four fields")),
            };
            let change =
                DiffChange::parse(change).ok_or_else(|| TextError::new(line, "unknown change"))?;
            let mut chars = file_type.chars();
            let (Some(file_type), None) = (chars.next(), chars.next()) else {
                return Err(TextError::new(line, "unknown kind of file"));
            };
            if (change == DiffChange::Renamed) != renamed_to.is_some() {
                return Err(TextError::new(line, "a rename has a new path, and only it"));
            }
            Ok(Difference {
                change,
                file_type,
                path: unescape(path),
                renamed_to: renamed_to.map(unescape),
            })
        })
        .collect()
}

/// A path as `zfs diff` prints it, where each byte that is not printable,
/// a space or a backslash is `\` and four octal digits.
fn unescape(path: &str) -> String {
    let mut rest = path.as_bytes();
    let mut decoded = Vec::with_capacity(rest.len());
    while let Some((&first, after)) = rest.split_first() {
        let code = after
            .get(..4)
            .filter(|digits| first == b'\\' && digits.iter().all(|d| (b'0'..=b'7').contains(d)))
            .and_then(|digits| {
                digits.iter().try_fold(0_u8, |code, digit| {
                    code.checked_mul(8)?.checked_add(digit - b'0')
                })
            });
        match code {
            Some(byte) => {
                decoded.push(byte);
                rest = after.get(4..).unwrap_or_default();
            }
            None => {
                decoded.push(first);
                rest = after;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
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
        assert_eq!(snapshot_space("tank@a\t512\n").unwrap()[0].1, 512);
        assert!(snapshot_space("tank@a\n").is_err());
        assert!(snapshot_space("tank\t1\n").is_err());
        assert!(snapshot_space("tank@a\tbig\n").is_err());
        assert!(pool_names("tank\n\n").is_ok());
        assert!(pool_names("-tank\n").is_err());
    }

    #[test]
    fn status_devices_are_the_absolute_paths() {
        let status = "  pool: tank\nconfig:\n\ttank  ONLINE\n\t  /dev/dm-3p1  ONLINE 0 0 0\n\
                      \t  /dev/vda2  ONLINE 0 0 0\nerrors: No known data errors\n";
        assert_eq!(status_devices(status), ["/dev/dm-3p1", "/dev/vda2"]);
    }

    #[test]
    fn a_value_with_tabs_and_line_breaks_is_one_property() {
        let text = "tank/a\torg.zfsbootmenu:commandline\tquiet\n\tloglevel=4\tx\tlocal\n\
                    tank/b\tcanmount\ton\tdefault\n\
                    tank/c\torg.zfsbootmenu:commandline\t-\t-\n";

        let rows = properties(text).unwrap();

        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].0, "tank/a");
        assert_eq!(rows[0].2.value, "quiet\n\tloglevel=4\tx");
        assert_eq!(rows[0].2.source, Source::Local);
        assert_eq!(rows[1].2, Property::new("on", Source::Default));
        assert_eq!(rows[2].2.value, "-");
        let unfinished = properties("tank/a\tp\tvalue\n").unwrap_err();
        assert_eq!(unfinished.reason, "not four fields");
        assert!(is_source("inherited from tank"));
        assert!(!is_source("loglevel=4"));
    }

    #[test]
    fn differences_decode_paths_and_renames() {
        let text = "M\t/\t/mnt/etc\n\
                    +\tF\t/mnt/etc/new\\0040file\n\
                    R\tF\t/mnt/a\t/mnt/b\\0134c\n\
                    -\t@\t/mnt/caf\\0303\\0251\n";

        let found = differences(text).unwrap();

        assert_eq!(found.len(), 4);
        assert_eq!(found[0].change, DiffChange::Modified);
        assert_eq!(found[0].file_type, '/');
        assert_eq!(found[1].path, "/mnt/etc/new file");
        assert_eq!(found[2].change, DiffChange::Renamed);
        assert_eq!(found[2].renamed_to.as_deref(), Some("/mnt/b\\c"));
        assert_eq!(found[3].path, "/mnt/caf\u{e9}");
        assert_eq!(found[3].change, DiffChange::Removed);
        assert_eq!(unescape("a\\9999\\01"), "a\\9999\\01");
        for wrong in [
            "M\tF\n",
            "X\tF\t/a\n",
            "M\tFF\t/a\n",
            "R\tF\t/a\n",
            "M\tF\t/a\t/b\n",
        ] {
            assert!(differences(wrong).is_err(), "{wrong:?}");
        }
    }
}
