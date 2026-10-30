#![allow(
    clippy::disallowed_methods,
    reason = "the call is tried on a real file, which is no kernel"
)]

use std::fs::File;
use std::os::fd::AsFd as _;

use super::*;

#[test]
fn what_is_no_kernel_is_never_loaded() {
    // Refused: without `CAP_SYS_BOOT` (`EPERM`), or as no kernel
    // (`ENOEXEC`); a kernel loaded before stays, as a failed load keeps it.
    let nothing = File::open("/dev/null").unwrap();
    let error = load_file(nothing.as_fd(), nothing.as_fd(), c"quiet").unwrap_err();
    assert!(
        error.raw_os_error().is_some() || error.kind() == io::ErrorKind::Unsupported,
        "{error}"
    );
}
