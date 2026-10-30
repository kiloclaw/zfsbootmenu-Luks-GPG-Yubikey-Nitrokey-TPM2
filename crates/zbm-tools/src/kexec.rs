//! `kexec`: loading a boot environment's kernel and starting it. The
//! kernel's own calls come first (ROADMAP.md, SB4): `kexec_file_load`,
//! which checks the files itself, as lockdown and Secure Boot require, and
//! `reboot(LINUX_REBOOT_CMD_KEXEC)`, through the [`KernelLoader`] port. The
//! `kexec` program loads what the call does not take, a device tree, which
//! `kexec_file_load` has no argument for, and what it refuses, as on a
//! kernel without the call (`ENOSYS`) or with a compressed arm64 kernel
//! (`ENOEXEC`); and starts the kernel when the call cannot.

#![allow(
    clippy::disallowed_methods,
    reason = "an adapter: these command lines are built here and nowhere else"
)]

use std::io;
use std::path::Path;

use zbm_platform::{Command, KernelLoader, Output, Runner, Status};

/// What `kexec -l` loads.
#[derive(Clone, Copy, Debug)]
pub struct Image<'i> {
    /// The kernel.
    pub kernel: &'i str,
    /// The initramfs.
    pub initrd: &'i str,
    /// The device tree, if any.
    pub dtb: Option<&'i Path>,
    /// The kernel's command line.
    pub command_line: &'i str,
}

/// The `kexec -a -l` command for `image`, with standard error merged into
/// its output as `2>&1` does.
fn load_command(image: &Image<'_>) -> Command {
    let mut command = Command::new("kexec").args([
        "-a".to_owned(),
        "-l".to_owned(),
        image.kernel.to_owned(),
        format!("--initrd={}", image.initrd),
    ]);
    if let Some(dtb) = image.dtb {
        command = command.arg(format!("--dtb={}", dtb.display()));
    }
    command
        .arg(format!("--command-line={}", image.command_line))
        .merge_stderr()
}

/// The command line `kexec -a -l` gets for `image`, for logs and tests.
pub fn load_line(image: &Image<'_>) -> String {
    load_command(image).display()
}

/// Logs why the kernel's call `call` did not do it, unless the port makes
/// no calls: the program does it now.
fn log_fallback(call: &str, error: &io::Error) {
    if error.kind() != io::ErrorKind::Unsupported {
        zbm_trace::info!("{call} failed, so kexec runs: {error}"; call, %error);
    }
}

/// Loads `image` with `loader`'s `kexec_file_load`, or with `kexec -a -l`
/// when it has a device tree or the call did not load it; the program's
/// error messages in the output.
///
/// # Errors
///
/// The error that kept `kexec` from starting.
pub fn load(
    loader: &dyn KernelLoader,
    runner: &dyn Runner,
    image: &Image<'_>,
) -> io::Result<Output> {
    if image.dtb.is_none() {
        let kernel = (Path::new(image.kernel), Path::new(image.initrd));
        match loader.load(kernel.0, kernel.1, image.command_line) {
            Ok(()) => {
                zbm_trace::debug!("kexec_file_load loaded {kernel}"; kernel = image.kernel);
                return Ok(Output::new(Status::exited(0), Vec::new()));
            }
            Err(error) => log_fallback("kexec_file_load", &error),
        }
    }
    runner.run(&load_command(image))
}

/// Starts the loaded kernel, whatever loaded it: `loader`'s
/// `reboot(LINUX_REBOOT_CMD_KEXEC)`, then `kexec -e -i` when that fails. It
/// returns only when both did, with the program's error messages.
///
/// # Errors
///
/// The error that kept `kexec` from starting.
pub fn execute(loader: &dyn KernelLoader, runner: &dyn Runner) -> io::Result<Output> {
    log_fallback("reboot", &loader.start());
    runner.run(&Command::new("kexec").args(["-e", "-i"]).merge_stderr())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use zbm_platform::NoKernelLoader;
    use zbm_testkit::FakeRunner;
    use zbm_testkit::fakes::{FakeKernelLoader, Loaded, Loads};

    #[test]
    fn without_the_calls_kexec_loads_and_starts_kernels() {
        let runner = FakeRunner::new();
        let image = Image {
            kernel: "/mnt/boot/vmlinuz",
            initrd: "/mnt/boot/initramfs.img",
            dtb: Some(Path::new("/mnt/boot/board.dtb")),
            command_line: "root=zfs:tank/ROOT/void quiet",
        };

        load(&NoKernelLoader, &runner, &image).unwrap();
        load(&NoKernelLoader, &runner, &Image { dtb: None, ..image }).unwrap();
        execute(&NoKernelLoader, &runner).unwrap();

        let line = "kexec -a -l /mnt/boot/vmlinuz --initrd=/mnt/boot/initramfs.img";
        assert_eq!(
            runner.lines(),
            [
                format!(
                    "{line} --dtb=/mnt/boot/board.dtb \
                     --command-line=root=zfs:tank/ROOT/void quiet"
                ),
                format!("{line} --command-line=root=zfs:tank/ROOT/void quiet"),
                "kexec -e -i".to_owned(),
            ]
        );
        assert_eq!(load_line(&image), runner.lines()[0]);
        assert!(runner.calls().iter().all(Command::merges_stderr));
    }

    #[test]
    fn the_kernel_loads_what_it_takes_and_kexec_the_rest() {
        let runner = FakeRunner::new();
        let loader = FakeKernelLoader::default();
        loader.answer(Loads::Accepted);
        let image = Image {
            kernel: "/mnt/boot/vmlinuz",
            initrd: "/mnt/boot/initramfs.img",
            dtb: None,
            command_line: "root=zfs:tank/ROOT/void quiet",
        };

        let outcome = load(&loader, &runner, &image).unwrap();
        assert!(outcome.status.is_success());
        assert_eq!(
            loader.loaded(),
            [Loaded {
                kernel: "/mnt/boot/vmlinuz".into(),
                initrd: "/mnt/boot/initramfs.img".into(),
                command_line: "root=zfs:tank/ROOT/void quiet".to_owned(),
            }]
        );
        assert!(runner.lines().is_empty());

        // A device tree: the program, which takes it.
        let dtb = Path::new("/mnt/boot/board.dtb");
        load(
            &loader,
            &runner,
            &Image {
                dtb: Some(dtb),
                ..image
            },
        )
        .unwrap();
        // A kernel the call refuses: the program too.
        loader.answer(Loads::Refused(8));
        load(&loader, &runner, &image).unwrap();
        assert_eq!(loader.loaded().len(), 1);
        assert_eq!(runner.lines().len(), 2);
        assert!(runner.lines()[0].contains("--dtb="));

        // The start returns only when it failed: the program tries.
        execute(&loader, &runner).unwrap();
        assert_eq!(loader.starts(), 1);
        assert_eq!(runner.lines()[2], "kexec -e -i");
    }
}
