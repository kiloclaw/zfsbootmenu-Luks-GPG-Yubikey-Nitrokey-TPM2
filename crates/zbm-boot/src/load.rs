//! What the boot loads with `kexec`: the kernel, the initramfs, the device
//! tree and the command line of a boot environment, or the copies measured
//! boot made of them.

use std::path::Path;

use zbm_bootenv::commandline::SEPARATOR;
use zbm_platform::exec::{Output, Runner};
use zbm_platform::kernel::KernelLoader;
use zbm_tools::kexec;
use zbm_types::{KernelArg, KernelCommandLine};

use crate::boot::measured::Copies;

/// What the boot passes to `kexec -l`: the kernel, initramfs, device tree
/// and command line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KexecLoad<'a> {
    /// Where the boot environment is mounted.
    pub mnt: &'a str,
    /// The kernel, relative to the boot environment's root.
    pub kernel: &'a str,
    /// The initramfs, relative to the boot environment's root.
    pub initramfs: &'a str,
    /// The device tree, a full path, if any.
    pub dtb: Option<&'a Path>,
    /// The root argument, such as `root=zfs:zroot/ROOT/void`.
    pub root: &'a KernelArg,
    /// The rest of the command line.
    pub args: &'a KernelCommandLine,
    /// The measured copies to load in place of the kernel and initramfs
    /// of the boot environment, after measured boot (ROADMAP.md SE10).
    pub copies: Option<&'a Copies>,
}

impl KexecLoad<'_> {
    /// The command line the kernel gets.
    pub fn command_line(&self) -> String {
        format!("{}{SEPARATOR}{}", self.root, self.args)
    }

    /// Calls `with` with what `kexec -l` loads.
    fn with_image<T>(&self, with: impl FnOnce(&kexec::Image<'_>) -> T) -> T {
        let (kernel, initrd) = match self.copies {
            Some(copies) => (
                copies.kernel.to_string_lossy().into_owned(),
                copies.initramfs.to_string_lossy().into_owned(),
            ),
            None => (
                format!("{}{}", self.mnt, self.kernel),
                format!("{}{}", self.mnt, self.initramfs),
            ),
        };
        let command_line = self.command_line();
        with(&kexec::Image {
            kernel: &kernel,
            initrd: &initrd,
            dtb: self.dtb,
            command_line: &command_line,
        })
    }

    /// The `kexec -a -l ...` command line, for logs and tests.
    pub fn line(&self) -> String {
        self.with_image(kexec::load_line)
    }

    /// Loads the kernel: with `kexec_file_load` through `loader`, or with
    /// `kexec -a -l ...` on `runner` for a device tree or what the call does
    /// not load, its two output streams merged as `2>&1` merges them; with
    /// the call alone when `checked` names what requires the kernel to check
    /// what it boots (`zbm_tools::kexec::checked_boot`).
    ///
    /// # Errors
    ///
    /// The error that kept `kexec` from starting.
    pub fn run(
        &self,
        loader: &dyn KernelLoader,
        runner: &dyn Runner,
        checked: Option<kexec::CheckedBoot>,
    ) -> std::io::Result<Output> {
        self.with_image(|image| kexec::load(loader, runner, image, checked))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn kexec_commands() {
        let dtb = PathBuf::from("/mnt/boot/dtbs/x.dtb");
        let root = KernelArg::new("root", Some("zfs:tank/ROOT/void")).unwrap();
        let load = KexecLoad {
            mnt: "/zfsbootmenu/environments/tank/ROOT/void/mnt",
            kernel: "/boot/vmlinuz-6.12.9_1",
            initramfs: "/boot/initramfs-6.12.9_1.img",
            dtb: Some(&dtb),
            root: &root,
            args: &"ro quiet".parse().unwrap(),
            copies: None,
        };
        let runner = zbm_testkit::FakeRunner::new();
        load.run(&zbm_platform::NoKernelLoader, &runner, None)
            .unwrap();
        let command = &runner.calls()[0];
        assert_eq!(command.program(), "kexec");
        let args: Vec<String> = command
            .get_args()
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            [
                "-a",
                "-l",
                "/zfsbootmenu/environments/tank/ROOT/void/mnt/boot/vmlinuz-6.12.9_1",
                "--initrd=/zfsbootmenu/environments/tank/ROOT/void/mnt/boot/initramfs-6.12.9_1.img",
                "--dtb=/mnt/boot/dtbs/x.dtb",
                "--command-line=root=zfs:tank/ROOT/void           ro quiet",
            ]
        );
        assert!(command.merges_stderr());
        assert_eq!(load.line(), command.display());
    }
}
