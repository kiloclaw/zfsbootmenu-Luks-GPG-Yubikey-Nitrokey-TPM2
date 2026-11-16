//! Helpers for the tests of this crate.

pub(crate) use zbm_testkit::legacy::{LINE_LOG_STUBS, stub_lines};
pub(crate) use zbm_testkit::redirect::Redirect;

/// The count of secrets that the boot checks before `kexec` is the whole
/// process's, every test's at once: a test that holds one takes this to
/// write, and one that boots, to read, so that neither sees the other's.
pub(crate) static SECRETS: std::sync::RwLock<()> = std::sync::RwLock::new(());

/// Taken to boot: no other test holds a secret meanwhile.
pub(crate) fn booting() -> std::sync::RwLockReadGuard<'static, ()> {
    SECRETS
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Taken to hold a secret: no other test boots meanwhile.
pub(crate) fn holding_a_secret() -> std::sync::RwLockWriteGuard<'static, ()> {
    SECRETS
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// `text` parsed as a name, which a test knows is valid.
pub(crate) fn name<T>(text: &str) -> T
where
    T: std::str::FromStr<Err = zbm_types::ParseError>,
{
    text.parse()
        .unwrap_or_else(|error| panic!("{text} is not a valid name: {error}"))
}

/// Writes the kernel 6.1.10 and its initramfs into the boot environment
/// `fs` of `state`, where the boot finds them, through `files`. The legacy
/// function never looks at them, so a parity case gives them to the Rust
/// boot alone.
pub(crate) fn kernel_files(
    files: &dyn zbm_platform::Files,
    state: &zbm_state::State<'_>,
    fs: &str,
) {
    let boot = state
        .environment(fs)
        .unwrap_or_else(|error| panic!("the state of {fs}: {error}"))
        .join("mnt/boot");
    let write = |name: &str, contents: &[u8]| {
        files
            .create_dir_all(&boot)
            .and_then(|()| files.write(&boot.join(name), contents))
            .unwrap_or_else(|error| panic!("{name}: {error}"));
    };
    write("vmlinuz-6.1.10", b"KERNEL");
    write("initramfs-6.1.10.img", b"INITRD");
}
