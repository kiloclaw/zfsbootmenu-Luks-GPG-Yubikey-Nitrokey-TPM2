//! Kernels loaded and started, recorded.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use zbm_platform::kernel::KernelLoader;

use crate::sync::lock;

/// What the fake loader does with a load.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum Loads {
    /// Nothing itself (`Unsupported`): the boot runs `kexec`, as on a
    /// kernel without `kexec_file_load`.
    #[default]
    None,
    /// Records it and succeeds.
    Accepted,
    /// Refuses it with the OS error `errno`, as `ENOEXEC` (8) or
    /// `EKEYREJECTED` (129).
    Refused(i32),
}

/// One kernel loaded.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Loaded {
    /// The kernel.
    pub kernel: PathBuf,
    /// The initramfs.
    pub initrd: PathBuf,
    /// The command line.
    pub command_line: String,
}

/// A [`KernelLoader`] that records loads and starts instead of making them.
#[derive(Debug, Default)]
pub struct FakeKernelLoader {
    loads: Mutex<Loads>,
    loaded: Mutex<Vec<Loaded>>,
    starts: Mutex<usize>,
}

impl FakeKernelLoader {
    /// Answers loads as `loads` says from now on.
    pub fn answer(&self, loads: Loads) {
        *lock(&self.loads) = loads;
    }

    /// The kernels it loaded.
    pub fn loaded(&self) -> Vec<Loaded> {
        lock(&self.loaded).clone()
    }

    /// How many times a kernel was started; a start fails as `EINVAL` (22)
    /// when it accepts loads, and leaves it to `kexec` otherwise.
    pub fn starts(&self) -> usize {
        *lock(&self.starts)
    }
}

impl KernelLoader for FakeKernelLoader {
    fn load(&self, kernel: &Path, initrd: &Path, command_line: &str) -> io::Result<()> {
        match lock(&self.loads).clone() {
            Loads::None => Err(io::ErrorKind::Unsupported.into()),
            Loads::Refused(errno) => Err(io::Error::from_raw_os_error(errno)),
            Loads::Accepted => {
                lock(&self.loaded).push(Loaded {
                    kernel: kernel.to_owned(),
                    initrd: initrd.to_owned(),
                    command_line: command_line.to_owned(),
                });
                Ok(())
            }
        }
    }

    fn start(&self) -> io::Error {
        *lock(&self.starts) += 1;
        match *lock(&self.loads) {
            Loads::None => io::ErrorKind::Unsupported.into(),
            Loads::Accepted | Loads::Refused(_) => io::Error::from_raw_os_error(22),
        }
    }
}
