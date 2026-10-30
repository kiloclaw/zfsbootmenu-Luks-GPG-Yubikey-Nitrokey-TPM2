//! Helpers for the tests of this crate.

pub use zbm_testkit::legacy::{LINE_LOG_STUBS, stub_lines};
pub use zbm_testkit::redirect::Redirect;

/// The count of secrets that the boot checks before `kexec` is the whole
/// process's, every test's at once: a test that holds one takes this to
/// write, and one that boots, to read, so that neither sees the other's.
pub static SECRETS: std::sync::RwLock<()> = std::sync::RwLock::new(());

/// Taken to boot: no other test holds a secret meanwhile.
pub fn booting() -> std::sync::RwLockReadGuard<'static, ()> {
    SECRETS
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Taken to hold a secret: no other test boots meanwhile.
pub fn holding_a_secret() -> std::sync::RwLockWriteGuard<'static, ()> {
    SECRETS
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// `text` parsed as a name, which a test knows is valid.
pub fn name<T>(text: &str) -> T
where
    T: std::str::FromStr<Err = zbm_types::ParseError>,
{
    text.parse()
        .unwrap_or_else(|error| panic!("{text} is not a valid name: {error}"))
}
