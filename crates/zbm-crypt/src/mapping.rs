//! Mappings, closed or open, as types.

use std::fmt;
use std::marker::PhantomData;

use zbm_types::{DevicePath, MappingName};

use crate::CryptError;

/// The state of a [`Mapping`] that is not open.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Closed {}

/// The state of a [`Mapping`] that is open: `/dev/mapper/NAME` exists.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Open {}

/// A LUKS volume and the name of its device-mapper mapping, in the state `S`:
/// [`Closed`] or [`Open`].
///
/// Anyone can describe a closed mapping with [`Mapping::new`]; only
/// [`LuksVolumes::open`](crate::LuksVolumes::open) produces an open one.
#[derive(Debug, Eq, PartialEq)]
pub struct Mapping<S> {
    /// The LUKS volume.
    device: DevicePath,
    /// The name of the mapping.
    name: MappingName,
    /// The state, which exists only in the type.
    state: PhantomData<S>,
}

impl<S> Mapping<S> {
    /// The LUKS volume.
    pub fn device(&self) -> &DevicePath {
        &self.device
    }

    /// The name of the mapping.
    pub fn name(&self) -> &MappingName {
        &self.name
    }

    /// The same mapping in the state `T`; only this crate changes states.
    pub(crate) fn into_state<T>(self) -> Mapping<T> {
        Mapping {
            device: self.device,
            name: self.name,
            state: PhantomData,
        }
    }
}

impl Mapping<Closed> {
    /// The volume `device`, to be opened as `/dev/mapper/NAME`.
    pub fn new(device: DevicePath, name: MappingName) -> Mapping<Closed> {
        Mapping {
            device,
            name,
            state: PhantomData,
        }
    }
}

/// A closed mapping is only a description, which every attempt to open the
/// volume may copy; an open one is not, as it stands for the mapping that
/// exists.
impl Clone for Mapping<Closed> {
    fn clone(&self) -> Mapping<Closed> {
        Mapping::new(self.device.clone(), self.name.clone())
    }
}

impl Mapping<Open> {
    /// The device node of the open mapping: `/dev/mapper/NAME`.
    pub fn path(&self) -> String {
        self.name.mapper_path()
    }
}

/// An operation that failed, with the mapping it was given, unchanged.
///
/// Handing the mapping back lets the caller try again, for example with the
/// next key method, without having to rebuild it.
#[derive(Debug)]
pub struct Failed<S> {
    /// The mapping, in the state it was in before the operation.
    pub mapping: Mapping<S>,
    /// Why the operation failed.
    pub error: CryptError,
    /// What the program printed, trimmed; empty when it did not run.
    pub output: String,
}

impl<S> fmt::Display for Failed<S> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.mapping.name, self.error)
    }
}

impl<S: fmt::Debug> std::error::Error for Failed<S> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failure_names_the_mapping_and_keeps_its_cause() {
        let mapping = Mapping::<Closed>::new("/dev/sda2".parse().unwrap(), "v".parse().unwrap());
        let failed = Failed {
            mapping,
            error: CryptError::NotInstalled,
            output: String::new(),
        };

        assert_eq!(failed.to_string(), "v: cryptsetup is not installed");
        let source = std::error::Error::source(&failed).map(ToString::to_string);
        assert_eq!(source.as_deref(), Some("cryptsetup is not installed"));
    }
}
