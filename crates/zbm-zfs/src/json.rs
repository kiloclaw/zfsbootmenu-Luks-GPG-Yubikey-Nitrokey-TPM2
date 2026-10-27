//! The JSON that `zfs` and `zpool` print with `-j -p` (OpenZFS 2.3 and
//! later).
//!
//! Every document has `output_version` and one map, `pools` or `datasets`,
//! from names to objects with a `properties` map. Each property has a
//! `value` (a string; exact numbers with `-p`) and a `source` with a `type`
//! (`LOCAL`, `DEFAULT`, `INHERITED`, `RECEIVED`, `TEMPORARY` or `NONE`) and
//! `data` (the ancestor of an inherited value, otherwise `-`).

use std::collections::BTreeMap;

use serde::Deserialize;
use zbm_config::spec::properties::BOOTFS;

use crate::model::{Dataset, Health, Pool, Property, Snapshot, Source, Version};
use crate::rows::Readable as _;

/// What was wrong with a JSON document.
#[derive(Debug, thiserror::Error)]
pub enum JsonError {
    /// It is not JSON of the expected shape.
    #[error("{0}")]
    Syntax(#[from] serde_json::Error),
    /// A name or value in it is not valid.
    #[error("invalid {what} {value:?}")]
    Invalid {
        /// What the value is, such as `pool name`.
        what: &'static str,
        /// The value.
        value: String,
    },
}

#[derive(Deserialize)]
struct RawSource {
    #[serde(rename = "type")]
    kind: String,
    data: String,
}

#[derive(Deserialize)]
struct RawProperty {
    value: String,
    source: RawSource,
}

impl From<RawProperty> for Property {
    fn from(raw: RawProperty) -> Property {
        let source = match raw.source.kind.as_str() {
            "LOCAL" => Source::Local,
            "DEFAULT" => Source::Default,
            "INHERITED" => Source::Inherited(raw.source.data),
            "RECEIVED" => Source::Received,
            "TEMPORARY" => Source::Temporary,
            _ => Source::None,
        };
        Property::new(raw.value, source)
    }
}

#[derive(Deserialize)]
struct RawObject {
    name: String,
    #[serde(default)]
    properties: BTreeMap<String, RawProperty>,
}

#[derive(Deserialize)]
struct Pools {
    #[serde(default)]
    pools: BTreeMap<String, RawObject>,
}

#[derive(Deserialize)]
struct Datasets {
    #[serde(default)]
    datasets: BTreeMap<String, RawObject>,
}

#[derive(Deserialize)]
struct RawVersion {
    userland: String,
    #[serde(default)]
    kernel: String,
}

#[derive(Deserialize)]
struct Versions {
    zfs_version: RawVersion,
}

fn invalid(what: &'static str, value: &str) -> JsonError {
    JsonError::Invalid {
        what,
        value: value.to_owned(),
    }
}

/// `text` parsed, with blank output read as an empty document: a listing
/// with nothing in it.
fn document<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, JsonError> {
    let text = if text.trim().is_empty() { "{}" } else { text };
    Ok(serde_json::from_str(text)?)
}

/// The properties of an object, converted.
fn properties(raw: BTreeMap<String, RawProperty>) -> BTreeMap<String, Property> {
    raw.into_iter()
        .map(|(name, property)| (name, property.into()))
        .collect()
}

/// `zfs version -j`.
///
/// # Errors
///
/// [`JsonError`] when the document has no `zfs_version`.
pub fn version(text: &str) -> Result<Version, JsonError> {
    let versions: Versions = serde_json::from_str(text)?;
    Ok(Version {
        userland: versions.zfs_version.userland,
        kernel: versions.zfs_version.kernel,
    })
}

/// `zpool list -j -p -o name,health,readonly,bootfs,guid`.
///
/// # Errors
///
/// [`JsonError`] for a document of another shape, or an invalid name,
/// `bootfs` or `guid`.
pub fn pools(text: &str) -> Result<Vec<Pool>, JsonError> {
    let document: Pools = document(text)?;
    document
        .pools
        .into_values()
        .map(|raw| {
            let properties = properties(raw.properties);
            let value = |property: &str| {
                properties
                    .get(property)
                    .and_then(Property::value)
                    .unwrap_or_default()
            };
            let bootfs = value(BOOTFS.name);
            Ok(Pool {
                name: raw
                    .name
                    .parse()
                    .map_err(|_| invalid("pool name", &raw.name))?,
                health: Health::parse(value("health")),
                read_only: value("readonly") == "on",
                bootfs: if bootfs.is_empty() {
                    None
                } else {
                    Some(bootfs.parse().map_err(|_| invalid("bootfs", bootfs))?)
                },
                guid: value("guid")
                    .parse()
                    .map_err(|_| invalid("guid", value("guid")))?,
            })
        })
        .readable()
}

/// `zfs get` or `zpool get -j -p`: each (name, property) and its value.
///
/// # Errors
///
/// [`JsonError`] for a document of another shape.
pub fn properties_of(text: &str) -> Result<Vec<(String, String, Property)>, JsonError> {
    let value: serde_json::Value = document(text)?;
    let objects = value
        .get("datasets")
        .or_else(|| value.get("pools"))
        .cloned()
        .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new()));
    let objects: BTreeMap<String, RawObject> = serde_json::from_value(objects)?;
    Ok(objects
        .into_values()
        .flat_map(|raw| {
            let name = raw.name;
            raw.properties
                .into_iter()
                .map(move |(property, value)| (name.clone(), property, value.into()))
        })
        .collect())
}

/// `zfs list -j -p -o name,<properties>`: each dataset with its properties.
///
/// # Errors
///
/// [`JsonError`] for a document of another shape or an invalid name.
pub fn datasets(text: &str) -> Result<Vec<Dataset>, JsonError> {
    let document: Datasets = document(text)?;
    document
        .datasets
        .into_values()
        .map(|raw| {
            Ok(Dataset {
                name: raw
                    .name
                    .parse()
                    .map_err(|_| invalid("dataset name", &raw.name))?,
                properties: properties(raw.properties),
            })
        })
        .readable()
}

/// `zfs list -j -p -t snapshot -o name,creation`.
///
/// # Errors
///
/// [`JsonError`] for a document of another shape, an invalid name, or a
/// creation time that is not a number (output without `-p`).
pub fn snapshots(text: &str) -> Result<Vec<Snapshot>, JsonError> {
    let document: Datasets = document(text)?;
    document
        .datasets
        .into_values()
        .map(|raw| {
            let creation = raw
                .properties
                .get("creation")
                .map(|property| property.value.clone())
                .unwrap_or_default();
            Ok(Snapshot {
                name: raw
                    .name
                    .parse()
                    .map_err(|_| invalid("snapshot name", &raw.name))?,
                creation: creation
                    .parse()
                    .map_err(|_| invalid("creation time", &creation))?,
            })
        })
        .readable()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool(name: &str, bootfs: &str, guid: &str) -> String {
        format!(r#"{{"pools":{{{}}}}}"#, entry(name, bootfs, guid))
    }

    /// A pool of the `pools` object.
    fn entry(name: &str, bootfs: &str, guid: &str) -> String {
        format!(
            r#""{name}":{{"name":"{name}","properties":{{
            "health":{{"value":"ONLINE","source":{{"type":"NONE","data":"-"}}}},
            "bootfs":{{"value":"{bootfs}","source":{{"type":"LOCAL","data":"-"}}}},
            "guid":{{"value":"{guid}","source":{{"type":"NONE","data":"-"}}}}}}}}"#
        )
    }

    #[test]
    fn invalid_values_are_errors_that_name_them() {
        assert!(pools(&pool("tank", "-", "1")).unwrap()[0].bootfs.is_none());
        assert_eq!(
            pools(&pool("tank", "tank/", "1")).unwrap_err().to_string(),
            "invalid bootfs \"tank/\""
        );
        assert!(pools(&pool("mirror", "-", "1")).is_err());
        // One unreadable pool is left out, not every pool.
        let two = format!(
            r#"{{"pools":{{{},{}}}}}"#,
            entry("tank", "-", "1"),
            entry("mirror", "-", "2")
        );
        let listed = pools(&two).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name.as_str(), "tank");
        assert!(pools(&pool("tank", "-", "many")).is_err());
        assert!(pools("\"pools\"").is_err());
        assert!(version("{}").is_err());
        let snapshot = r#"{"datasets":{"t@a":{"name":"t@a","properties":{
            "creation":{"value":"Thu Oct 22","source":{"type":"NONE","data":"-"}}}}}}"#;
        assert_eq!(
            snapshots(snapshot).unwrap_err().to_string(),
            "invalid creation time \"Thu Oct 22\""
        );
        assert!(datasets(r#"{"datasets":{"t/":{"name":"t/"}}}"#).is_err());
        assert!(snapshots(r#"{"datasets":{"t":{"name":"t"}}}"#).is_err());
    }

    #[test]
    fn every_source_type_is_read() {
        let document = r#"{"datasets":{"t":{"name":"t","properties":{
            "a":{"value":"1","source":{"type":"DEFAULT","data":"-"}},
            "b":{"value":"2","source":{"type":"RECEIVED","data":"-"}},
            "c":{"value":"3","source":{"type":"TEMPORARY","data":"-"}},
            "d":{"value":"4","source":{"type":"LOCAL","data":"-"}}}}}}"#;
        let sources: Vec<Source> = properties_of(document)
            .unwrap()
            .into_iter()
            .map(|(_, _, property)| property.source)
            .collect();
        assert_eq!(
            sources,
            [
                Source::Default,
                Source::Received,
                Source::Temporary,
                Source::Local
            ]
        );
        assert!(properties_of("{}").unwrap().is_empty());
        assert!(pools("\n").unwrap().is_empty());
        assert!(datasets("").unwrap().is_empty());
    }
}
