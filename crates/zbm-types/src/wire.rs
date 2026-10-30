//! The types as they cross a socket between ZFSBootMenu's processes
//! (ROADMAP.md, PM10; ADR 0002): a checked name is its text, and is
//! checked again as it is read, by the same parser as any other input; a
//! value read from a peer is no more trusted than one read from a pool.

use std::fmt;
use std::str::FromStr;

use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize, Serializer};

use crate::{
    Answer, DatasetName, DevicePath, ImportPolicy, MappingName, PoolName, SnapshotName, Untrusted,
};

/// Reads a name of type `T` from a string, with `T`'s parser.
struct Parsed<T>(std::marker::PhantomData<T>);

impl<T: FromStr> Visitor<'_> for Parsed<T>
where
    T::Err: fmt::Display,
{
    type Value = T;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a name as text")
    }

    fn visit_str<E: de::Error>(self, text: &str) -> Result<T, E> {
        text.parse().map_err(E::custom)
    }
}

/// Serialises the names as their text, and reads them with their parsers.
macro_rules! as_text {
    ($($name:ty),+ $(,)?) => {$(
        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.collect_str(self)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<$name, D::Error> {
                deserializer.deserialize_str(Parsed::<$name>(std::marker::PhantomData))
            }
        }
    )+};
}

as_text!(
    DatasetName,
    PoolName,
    SnapshotName,
    DevicePath,
    MappingName,
    ImportPolicy
);

impl Serialize for Answer {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(match self {
            Answer::Continue => "continue",
            Answer::Escape => "escape",
        })
    }
}

impl<'de> Deserialize<'de> for Answer {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Answer, D::Error> {
        match String::deserialize(deserializer)?.as_str() {
            "continue" => Ok(Answer::Continue),
            "escape" => Ok(Answer::Escape),
            other => Err(de::Error::unknown_variant(other, &["continue", "escape"])),
        }
    }
}

/// Untrusted text stays untrusted on the way: it is marked again as read.
impl<T: Serialize> Serialize for Untrusted<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.get().serialize(serializer)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Untrusted<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Untrusted<T>, D::Error> {
        T::deserialize(deserializer).map(Untrusted::new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip<T: Serialize + for<'de> Deserialize<'de> + PartialEq + fmt::Debug>(value: &T) {
        let text = serde_json::to_string(value).unwrap();
        assert_eq!(&serde_json::from_str::<T>(&text).unwrap(), value, "{text}");
    }

    #[test]
    fn names_cross_as_text_and_are_checked_as_they_are_read() {
        let dataset: DatasetName = "zroot/ROOT/void".parse().unwrap();
        assert_eq!(
            serde_json::to_string(&dataset).unwrap(),
            "\"zroot/ROOT/void\""
        );
        round_trip(&dataset);
        round_trip(&"zroot".parse::<PoolName>().unwrap());
        round_trip(&"zroot/ROOT/void@before".parse::<SnapshotName>().unwrap());
        round_trip(&"/dev/sda2".parse::<DevicePath>().unwrap());
        round_trip(&"root".parse::<MappingName>().unwrap());
        round_trip(&ImportPolicy::default());
        round_trip(&Answer::Escape);
        round_trip(&Untrusted::new("vmlinuz-6.1".to_owned()));
        for bad in ["\"\"", "\"/absolute\"", "3", "\"zroot//x\""] {
            assert!(serde_json::from_str::<DatasetName>(bad).is_err(), "{bad}");
        }
        assert!(serde_json::from_str::<MappingName>("\"-x\"").is_err());
        assert!(serde_json::from_str::<Answer>("\"maybe\"").is_err());
    }
}
