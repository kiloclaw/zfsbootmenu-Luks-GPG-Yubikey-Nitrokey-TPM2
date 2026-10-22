//! The `.cmdline` section of an EFI bundle: `objcopy` and `objdump`.

#![allow(
    clippy::disallowed_methods,
    reason = "an adapter: these command lines are built here and nowhere else"
)]

use std::io;
use std::path::Path;

use zbm_platform::{Command, Input, Output, Runner};

/// Runs `command` with the console as its standard input and its error
/// messages in its output.
fn merged(runner: &dyn Runner, command: Command) -> io::Result<Output> {
    runner.run(&command.stdin(Input::Inherit).merge_stderr())
}

/// Writes the `.cmdline` section of `image` to `to`
/// (`objcopy --dump-section .cmdline=TO IMAGE`).
///
/// # Errors
///
/// The error that kept `objcopy` from starting.
pub fn dump_cmdline(runner: &dyn Runner, image: &Path, to: &Path) -> io::Result<Output> {
    let command = Command::new("objcopy")
        .arg("--dump-section")
        .arg(format!(".cmdline={}", to.display()))
        .arg(image);
    merged(runner, command)
}

/// The section headers of `image`, one per line (`objdump -h -w IMAGE`).
///
/// # Errors
///
/// The error that kept `objdump` from starting.
pub fn section_headers(runner: &dyn Runner, image: &Path) -> io::Result<Output> {
    let command = Command::new("objdump")
        .args(["-h", "-w"])
        .arg(image)
        .stdin(Input::Inherit);
    runner.run(&command)
}

/// Removes the `.cmdline` section of `image`
/// (`objcopy --remove-section .cmdline IMAGE`).
///
/// # Errors
///
/// The error that kept `objcopy` from starting.
pub fn remove_cmdline(runner: &dyn Runner, image: &Path) -> io::Result<Output> {
    let command = Command::new("objcopy")
        .args(["--remove-section", ".cmdline"])
        .arg(image);
    merged(runner, command)
}

/// Adds the contents of `from` to `image` as its `.cmdline` section at the
/// address `0x<offset>`.
///
/// # Errors
///
/// The error that kept `objcopy` from starting.
pub fn add_cmdline(
    runner: &dyn Runner,
    image: &Path,
    from: &Path,
    offset: &str,
) -> io::Result<Output> {
    let command = Command::new("objcopy")
        .arg("--add-section")
        .arg(format!(".cmdline={}", from.display()))
        .arg("--change-section-vma")
        .arg(format!(".cmdline=0x{offset}"))
        .arg(image);
    merged(runner, command)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use zbm_testkit::FakeRunner;

    #[test]
    fn the_section_is_dumped_measured_and_replaced() {
        let runner = FakeRunner::new();
        let image = Path::new("/boot/efi/zbm.EFI");
        let section = Path::new("/tmp/tmp.1");

        dump_cmdline(&runner, image, section).unwrap();
        section_headers(&runner, image).unwrap();
        remove_cmdline(&runner, image).unwrap();
        add_cmdline(&runner, image, section, "30000").unwrap();

        assert_eq!(
            runner.lines(),
            [
                "objcopy --dump-section .cmdline=/tmp/tmp.1 /boot/efi/zbm.EFI",
                "objdump -h -w /boot/efi/zbm.EFI",
                "objcopy --remove-section .cmdline /boot/efi/zbm.EFI",
                "objcopy --add-section .cmdline=/tmp/tmp.1 --change-section-vma \
                 .cmdline=0x30000 /boot/efi/zbm.EFI",
            ]
        );
        let calls = runner.calls();
        assert!(calls[0].merges_stderr() && !calls[1].merges_stderr());
        assert!(calls.iter().all(|call| *call.get_stdin() == Input::Inherit));
    }
}
