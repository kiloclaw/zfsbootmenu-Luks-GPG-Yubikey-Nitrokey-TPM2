//! The key file method.

use std::io;
use std::path::{Path, PathBuf};

use zbm_crypt::{Closed, Cryptsetup, Failed, LuksVolumes as _, Mapping, OpenOptions};
use zbm_platform::files::FileKind;
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

/// The contents of the regular file at `path`; a named pipe or a device
/// is refused before it is read, as a read of it may wait for ever or not
/// end.
fn read_regular(context: &Context<'_>, path: &Path) -> io::Result<Vec<u8>> {
    let kind = context.files().metadata(path)?.kind;
    if kind != FileKind::File {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    context.files().read(path)
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
        let key = match read_regular(context, &self.path) {
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
        let opened = Cryptsetup::new(context.runner()).open(
            mapping,
            zbm_crypt::Key::Secret(&key),
            &options.args(),
        );
        match opened {
            Ok(open) => Attempt::Opened(open),
            Err(Failed { mapping, error, .. }) => Attempt::Refused { mapping, error },
        }
    }
}
