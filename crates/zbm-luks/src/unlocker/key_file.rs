//! The key file method.

use std::io;
use std::path::{Path, PathBuf};

use zbm_crypt::{Closed, Cryptsetup, Failed, Mapping, OpenOptions};
use zbm_platform::{Context, Secret};

use super::{Attempt, KeyMethod, Unavailable};

/// Opens a volume with the contents of a key file.
///
/// The file is read into a [`Secret`] and given to `cryptsetup` on standard
/// input, so the key never appears as a path in `cryptsetup`'s arguments and
/// no copy of it is left in a file. The legacy code passes the path instead
/// (`--key-file=PATH`); the result is the same key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyFile {
    /// The key file.
    path: PathBuf,
}

impl KeyFile {
    /// The method for the key file at `path`.
    pub fn new(path: PathBuf) -> KeyFile {
        KeyFile { path }
    }

    /// The key file.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl KeyMethod for KeyFile {
    fn name(&self) -> &'static str {
        "key file"
    }

    fn try_open(
        &self,
        context: &Context<'_>,
        mapping: Mapping<Closed>,
        options: OpenOptions,
    ) -> Attempt {
        let key = match context.files().read(&self.path) {
            Ok(bytes) => Secret::from_vec(bytes),
            Err(error) => {
                let path = self.path.clone();
                let reason = if error.kind() == io::ErrorKind::NotFound {
                    Unavailable::KeyFileMissing { path }
                } else {
                    Unavailable::KeyFileUnreadable {
                        path,
                        source: error,
                    }
                };
                return Attempt::Unavailable { mapping, reason };
            }
        };
        match Cryptsetup::new(context.runner()).open(mapping, &key, options) {
            Ok(open) => Attempt::Opened(open),
            Err(Failed { mapping, error }) => Attempt::Refused { mapping, error },
        }
    }
}
