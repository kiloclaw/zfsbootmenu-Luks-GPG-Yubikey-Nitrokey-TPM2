//! `kexec`: loading a boot environment's kernel and starting it.

#![allow(
    clippy::disallowed_methods,
    reason = "an adapter: these command lines are built here and nowhere else"
)]

use std::io;
use std::path::Path;

use zbm_platform::{Command, Output, Runner};

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

/// Loads `image` (`kexec -a -l`), with its error messages in the output.
///
/// # Errors
///
/// The error that kept `kexec` from starting.
pub fn load(runner: &dyn Runner, image: &Image<'_>) -> io::Result<Output> {
    runner.run(&load_command(image))
}

/// Starts the loaded kernel (`kexec -e -i`); it returns only when that
/// fails, with the error messages in the output.
///
/// # Errors
///
/// The error that kept `kexec` from starting.
pub fn execute(runner: &dyn Runner) -> io::Result<Output> {
    runner.run(&Command::new("kexec").args(["-e", "-i"]).merge_stderr())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use zbm_testkit::FakeRunner;

    #[test]
    fn kernels_are_loaded_and_started() {
        let runner = FakeRunner::new();
        let image = Image {
            kernel: "/mnt/boot/vmlinuz",
            initrd: "/mnt/boot/initramfs.img",
            dtb: Some(Path::new("/mnt/boot/board.dtb")),
            command_line: "root=zfs:tank/ROOT/void quiet",
        };

        load(&runner, &image).unwrap();
        load(&runner, &Image { dtb: None, ..image }).unwrap();
        execute(&runner).unwrap();

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
}
