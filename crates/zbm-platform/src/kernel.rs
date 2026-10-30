//! Starting another kernel (ROADMAP.md, SB4): the [`KernelLoader`] port.
//! The real one loads with `kexec_file_load`, which reads the files itself,
//! and starts with `reboot(LINUX_REBOOT_CMD_KEXEC)`; where it loads nothing,
//! as on a kernel without the call, the boot runs `kexec` instead
//! (`zbm_tools::kexec`).

use std::ffi::CString;
use std::fs::File;
use std::io;
use std::os::fd::AsFd as _;
use std::path::Path;

/// Loading a kernel and starting it.
pub trait KernelLoader: Send + Sync {
    /// Loads `kernel`, with the initramfs `initrd` and `command_line`, for
    /// [`KernelLoader::start`]; it replaces a kernel loaded before.
    ///
    /// # Errors
    ///
    /// `Unsupported` where it loads nothing itself; why the kernel refused
    /// the files.
    fn load(&self, kernel: &Path, initrd: &Path, command_line: &str) -> io::Result<()>;

    /// Starts the loaded kernel, whatever loaded it; it returns only when
    /// that failed, with why: `Unsupported` where it starts nothing itself.
    fn start(&self) -> io::Error;
}

/// The real kernel's calls.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemKernelLoader;

impl KernelLoader for SystemKernelLoader {
    #[allow(
        clippy::disallowed_methods,
        reason = "the kernel reads the files itself, from descriptors opened here"
    )]
    fn load(&self, kernel: &Path, initrd: &Path, command_line: &str) -> io::Result<()> {
        let command_line = CString::new(command_line)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        let (kernel, initrd) = (File::open(kernel)?, File::open(initrd)?);
        zbm_sys::kexec::load_file(kernel.as_fd(), initrd.as_fd(), &command_line)
    }

    fn start(&self) -> io::Error {
        zbm_sys::kexec::start_loaded()
    }
}

/// A loader that loads and starts nothing itself, for programs that boot
/// nothing, and for tests, which run `kexec`.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoKernelLoader;

impl KernelLoader for NoKernelLoader {
    fn load(&self, _kernel: &Path, _initrd: &Path, _command_line: &str) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }

    fn start(&self) -> io::Error {
        io::ErrorKind::Unsupported.into()
    }
}

#[cfg(test)]
mod tests;
