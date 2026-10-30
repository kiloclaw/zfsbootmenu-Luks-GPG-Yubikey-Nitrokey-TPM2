//! The LUKS port (ADR 0003, ROADMAP.md DM6): what unlocking asks of the
//! machine's LUKS volumes, whatever answers it. [`crate::Cryptsetup`]
//! answers through `cryptsetup`; the native LUKS2 code of CD9 will answer
//! too, behind the same trait.

use std::path::Path;

use zbm_platform::Secret;
use zbm_types::MappingName;

use crate::CryptError;
use crate::mapping::{Closed, Failed, Mapping, Open};
use crate::token::Token;

/// Where the key that opens a volume comes from.
#[derive(Clone, Copy, Debug)]
pub enum Key<'k> {
    /// A key file on a mounted file system, read where it is.
    File(&'k Path),
    /// Key material, given on standard input.
    Secret(&'k Secret),
    /// The LUKS2 token `id`, through cryptsetup's plugin for its type, with
    /// a PIN when the token needs one.
    Token {
        /// The token's ID.
        id: &'k str,
        /// The PIN.
        pin: Option<&'k Secret>,
    },
}

/// What unlocking asks of LUKS volumes. A header is the volume itself or a
/// detached header file.
pub trait LuksVolumes {
    /// Whether `header` is a LUKS volume or detached header.
    fn is_luks(&self, header: &Path) -> bool;

    /// The UUID of the LUKS volume of `header`, or `None` when it cannot be
    /// read.
    fn uuid(&self, header: &Path) -> Option<String>;

    /// The LUKS2 tokens of `header`, with their fields where they can be
    /// read; none when it cannot be read.
    fn tokens(&self, header: &Path) -> Vec<Token>;

    /// Opens `mapping` with `key`, with the further arguments of
    /// `cryptsetup open` in `args` (from a crypttab: `--header=`,
    /// `--allow-discards`).
    ///
    /// # Errors
    ///
    /// [`Failed`] with the mapping, still closed, why, and what the
    /// program said: [`CryptError::WrongKey`] when no key slot accepts the
    /// key.
    fn open(
        &self,
        mapping: Mapping<Closed>,
        key: Key<'_>,
        args: &[String],
    ) -> Result<Mapping<Open>, Failed<Closed>>;

    /// Closes `mapping`.
    ///
    /// # Errors
    ///
    /// [`Failed`] with the mapping, still open: [`CryptError::Busy`] when
    /// something still uses it.
    fn close(&self, mapping: Mapping<Open>) -> Result<Mapping<Closed>, Failed<Open>>;

    /// Closes the mapping `name`, which this process did not open, as one
    /// recorded in the state directory.
    ///
    /// # Errors
    ///
    /// [`CryptError`] when it cannot be closed, for example because it is
    /// in use.
    fn close_named(&self, name: &MappingName) -> Result<(), CryptError>;
}

/// A reference answers as what it refers to.
impl<T: LuksVolumes + ?Sized> LuksVolumes for &T {
    fn is_luks(&self, header: &Path) -> bool {
        (**self).is_luks(header)
    }

    fn uuid(&self, header: &Path) -> Option<String> {
        (**self).uuid(header)
    }

    fn tokens(&self, header: &Path) -> Vec<Token> {
        (**self).tokens(header)
    }

    fn open(
        &self,
        mapping: Mapping<Closed>,
        key: Key<'_>,
        args: &[String],
    ) -> Result<Mapping<Open>, Failed<Closed>> {
        (**self).open(mapping, key, args)
    }

    fn close(&self, mapping: Mapping<Open>) -> Result<Mapping<Closed>, Failed<Open>> {
        (**self).close(mapping)
    }

    fn close_named(&self, name: &MappingName) -> Result<(), CryptError> {
        (**self).close_named(name)
    }
}

#[cfg(test)]
mod tests {
    use zbm_testkit::FakeRunner;

    use super::*;
    use crate::Cryptsetup;

    /// The lines `volumes` runs for each call of the port.
    fn every_call(volumes: impl LuksVolumes) {
        let header = Path::new("/dev/sda2");
        let closed = Mapping::new("/dev/sda2".parse().unwrap(), "v".parse().unwrap());
        assert!(volumes.is_luks(header));
        assert_eq!(volumes.uuid(header).as_deref(), Some("0b5c-41"));
        assert!(volumes.tokens(header).is_empty());
        let open = volumes
            .open(closed, Key::File(Path::new("/k")), &[])
            .unwrap();
        assert!(volumes.close(open).is_ok());
        assert!(volumes.close_named(&"w".parse().unwrap()).is_ok());
    }

    #[test]
    fn a_reference_answers_as_what_it_refers_to() {
        let runner = FakeRunner::new();
        runner.respond("cryptsetup luksUUID /dev/sda2", 0, "0b5c-41\n");
        let cryptsetup = Cryptsetup::new(&runner);

        let by_reference: &Cryptsetup<'_> = &cryptsetup;
        every_call(by_reference);

        assert_eq!(
            runner.lines(),
            [
                "cryptsetup isLuks /dev/sda2",
                "cryptsetup luksUUID /dev/sda2",
                "cryptsetup luksDump --dump-json-metadata /dev/sda2",
                "cryptsetup luksDump /dev/sda2",
                "cryptsetup open --type luks --key-file=/k /dev/sda2 v",
                "cryptsetup close v",
                "cryptsetup close w",
            ]
        );
    }
}
