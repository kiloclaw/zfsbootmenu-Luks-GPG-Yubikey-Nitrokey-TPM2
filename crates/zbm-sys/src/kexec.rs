//! Booting another kernel without a program (ROADMAP.md, SB4): the
//! kernel and initramfs loaded from their files (`kexec_file_load`), which
//! the kernel checks itself, as lockdown and Secure Boot require; and the
//! loaded kernel started (`reboot(LINUX_REBOOT_CMD_KEXEC)`), whatever
//! loaded it, as a teardown hook may load its own.

use std::ffi::{CStr, c_int, c_long, c_ulong};
use std::io;
use std::os::fd::{AsRawFd as _, BorrowedFd};

unsafe extern "C" {
    // `long syscall(long number, ...)`: glibc has no `kexec_file_load`.
    fn syscall(number: c_long, ...) -> c_long;
}

/// The number of `kexec_file_load`, where the architecture has it.
#[cfg(target_arch = "x86_64")]
const KEXEC_FILE_LOAD: Option<c_long> = Some(320);
#[cfg(any(
    target_arch = "aarch64",
    target_arch = "riscv64",
    target_arch = "loongarch64"
))]
const KEXEC_FILE_LOAD: Option<c_long> = Some(294);
#[cfg(not(any(
    target_arch = "x86_64",
    target_arch = "aarch64",
    target_arch = "riscv64",
    target_arch = "loongarch64"
)))]
const KEXEC_FILE_LOAD: Option<c_long> = None;

/// Loads the kernel read from `kernel`, with the initramfs read from
/// `initrd` and `command_line`, to start at the next
/// [`start_loaded`]; it replaces a kernel loaded before.
///
/// # Errors
///
/// `ENOSYS` (`Unsupported`) where the kernel or the architecture has no
/// such call; `ENOEXEC` for a kernel of a format it does not take, as a
/// compressed arm64 `Image.gz`; `EKEYREJECTED` or `EPERM` for a kernel
/// that lockdown refuses unsigned; `EPERM` without `CAP_SYS_BOOT`.
pub fn load_file(
    kernel: BorrowedFd<'_>,
    initrd: BorrowedFd<'_>,
    command_line: &CStr,
) -> io::Result<()> {
    let Some(number) = KEXEC_FILE_LOAD else {
        return Err(io::ErrorKind::Unsupported.into());
    };
    // The length counts the terminating NUL, as the kernel wants it.
    let length =
        c_ulong::try_from(command_line.to_bytes_with_nul().len()).map_err(io::Error::other)?;
    // SAFETY: the descriptors are open for the call, the command line is
    // NUL-terminated and `length` long, and no flags are given; the kernel
    // reads the files and the string, and writes nothing here.
    let result = unsafe {
        syscall(
            number,
            c_int::from(kernel.as_raw_fd()),
            c_int::from(initrd.as_raw_fd()),
            length,
            command_line.as_ptr(),
            c_ulong::MIN,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(rustix::io::Errno::NOSYS.raw_os_error()) {
            Err(io::Error::new(io::ErrorKind::Unsupported, error))
        } else {
            Err(error)
        }
    }
}

/// Writes what is cached to disk, then starts the loaded kernel, as
/// `kexec -e -i` does: it returns only when that fails.
///
/// # Errors
///
/// `EINVAL` when no kernel is loaded; `EPERM` without `CAP_SYS_BOOT`.
pub fn start_loaded() -> io::Error {
    rustix::fs::sync();
    match rustix::system::reboot(rustix::system::RebootCommand::Kexec) {
        Ok(()) => io::Error::other("the loaded kernel did not start"),
        Err(error) => error.into(),
    }
}

#[cfg(test)]
mod tests;
