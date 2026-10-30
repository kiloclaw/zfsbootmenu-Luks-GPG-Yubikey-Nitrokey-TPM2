//! A volume being unlocked (ROADMAP.md DM6): its mapping, closed until a
//! key method opens it, and what its crypttab entry says about it. Only an
//! open mapping, which only the LUKS port makes, turns a
//! [`Volume<Closed>`] into a [`Volume<Open>`].

use std::path::{Path, PathBuf};

use zbm_crypt::{Closed, Mapping, Open};
use zbm_types::{DevicePath, MappingName};

use crate::config::Options;
use crate::keys::Target;
use crate::mappings::Origin;

/// What a crypttab entry, or discovery, says about a volume.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Entry {
    /// The LUKS header: the device, or a detached header.
    pub header: PathBuf,
    /// The key file specification, or empty.
    pub keyspec: String,
    /// The options, read.
    pub options: Options,
    /// The options as written, for key scripts.
    pub options_text: String,
    /// How the volume was found.
    pub origin: Origin,
}

/// A volume being unlocked, its mapping in the state `S`: [`Closed`] or
/// [`Open`].
#[derive(Debug)]
pub struct Volume<S> {
    mapping: Mapping<S>,
    /// Boxed: a volume moves through the key methods and back.
    entry: Box<Entry>,
}

impl<S> Volume<S> {
    /// The mapping.
    pub fn mapping(&self) -> &Mapping<S> {
        &self.mapping
    }

    /// The mapping name.
    pub fn name(&self) -> &MappingName {
        self.mapping.name()
    }

    /// The LUKS device.
    pub fn device(&self) -> &DevicePath {
        self.mapping.device()
    }

    /// What its entry says.
    pub fn entry(&self) -> &Entry {
        &self.entry
    }

    /// The LUKS header: the device, or a detached header.
    pub fn header(&self) -> &Path {
        &self.entry.header
    }

    /// The key file specification, or empty.
    pub fn keyspec(&self) -> &str {
        &self.entry.keyspec
    }

    /// The options of its entry.
    pub fn options(&self) -> &Options {
        &self.entry.options
    }
}

impl Volume<Closed> {
    /// The volume of `mapping`, as `entry` describes it.
    pub fn new(mapping: Mapping<Closed>, entry: Entry) -> Volume<Closed> {
        Volume {
            mapping,
            entry: Box::new(entry),
        }
    }

    /// The volume as the key methods open it.
    pub fn target(&self) -> Target<'_> {
        Target::new(&self.mapping, &self.entry.options.args)
    }

    /// The volume, open as `mapping`, which a key method opened.
    pub(crate) fn opened(self, mapping: Mapping<Open>) -> Volume<Open> {
        Volume {
            mapping,
            entry: self.entry,
        }
    }
}
