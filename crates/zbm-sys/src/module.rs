//! Kernel modules inserted by the runtime itself (ROADMAP.md CD4):
//! `finit_module` on the module's file, which the kernel decompresses
//! itself when asked (`MODULE_INIT_COMPRESSED_FILE`, Linux 5.17), so that
//! the image needs no `insmod` or `modprobe`.

use std::ffi::CString;
use std::io;
use std::os::fd::AsFd;

/// `MODULE_INIT_COMPRESSED_FILE`: the kernel decompresses the file.
const COMPRESSED_FILE: i32 = 4;

/// Inserts the module open as `file` with `parameters`, as `insmod` does,
/// the kernel decompressing it when `compressed`.
///
/// # Errors
///
/// The kernel's: `EEXIST` (`AlreadyExists`) for a module loaded already,
/// `EINVAL` for a compressed module a kernel cannot decompress, `ENOEXEC`
/// for a file that is no module of this kernel, `EPERM` without
/// `CAP_SYS_MODULE`; `InvalidInput` for parameters that hold a NUL.
pub fn insert(file: impl AsFd, parameters: &str, compressed: bool) -> io::Result<()> {
    let parameters = CString::new(parameters)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "a NUL in the parameters"))?;
    let flags = if compressed { COMPRESSED_FILE } else { 0 };
    Ok(rustix::system::finit_module(file, &parameters, flags)?)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::disallowed_methods,
        reason = "the null device, handed over as a module is"
    )]

    use super::*;

    #[test]
    fn a_file_that_is_no_module_or_parameters_with_a_nul_are_refused() {
        let null = std::fs::File::open("/dev/null").unwrap();
        let error = insert(&null, "a=\0", false).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        // Not a module, or not allowed here: the kernel says no either way.
        assert!(insert(&null, "", false).is_err());
        assert!(insert(&null, "", true).is_err());
    }
}
