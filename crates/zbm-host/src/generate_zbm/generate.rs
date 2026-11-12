//! The program: the Rust counterpart of the main part of the Perl
//! `generate-zbm`.
//!
//! The steps, messages and exit statuses follow the Perl program. Unlike it,
//! the generator runs every program without a shell (ADR 0013), with its
//! arguments as they are (`invocation::Invocation`); the initramfs flags of
//! the configuration are split as shell words, and anything a shell would
//! expand in them is refused. Perl's `exit` and `die`
//! become `Stop`; whichever way the program ends, the boot partition is
//! unmounted if the generator mounted it, as the Perl `END` block does.
//!
//! This module holds the run itself and the output and command helpers; the
//! steps are in submodules: `settings` (the configuration file and the
//! options), `system` (the boot partition, hooks, hostid and work directory),
//! `kernels` (the choice of the kernel), `initramfs`, `bundle` (the EFI bundle)
//! and `install` (placing the images and pruning old ones); `warnings` finds
//! the image plan's warnings in the builder's output.

#![allow(
    clippy::disallowed_methods,
    reason = "a host tool works on the files and environment of the host it runs on; \
              its tests run it in sandboxes (ADR 0013)"
)]

mod bundle;
mod files;
mod graphics;
mod initramfs;
mod install;
mod invocation;
mod kernels;
mod settings;
mod system;
mod warnings;

use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;

use zbm_config::spec::OptionSpec;
use zbm_config::spec::generate::{
    COMPONENTS_ENABLED, EFI_ENABLED, GLOBAL_MANAGE_IMAGES, GLOBAL_POST_HOOKS_DIR,
    GLOBAL_PRE_HOOKS_DIR,
};
use zbm_platform::exec::Runner;
use zbm_tools::programs;

use crate::generate_zbm::cli::{self, Options, Outcome};
use crate::generate_zbm::config::{Fatal, Value};
use crate::generate_zbm::perl;
use invocation::Invocation;
use settings::Settings;
use system::WorkDir;

/// The ZFSBootMenu version, `$VERSION` in the Perl program.
pub const VERSION: &str = zbm_platform::version::ZBM_VERSION;

/// Where the generator finds things on the system; tests replace the paths.
pub struct Context<'a> {
    /// Runs the external programs.
    pub runner: &'a dyn Runner,
    /// The program name for messages, Perl's `$0`.
    pub program: String,
    /// `$TMPDIR`.
    pub tmpdir: Option<PathBuf>,
    /// Whether `SOURCE_DATE_EPOCH` is set: images are then built to be
    /// reproducible (`dracut --reproducible`; `mkinitcpio` reads the
    /// variable itself).
    pub reproducible: bool,
    /// The time stamp of EFI bundles: `SOURCE_DATE_EPOCH`, when it is a
    /// time a PE header can hold; the stub's own otherwise.
    pub timestamp: Option<u32>,
    /// The hostid the SPL module uses.
    pub spl_hostid: PathBuf,
    /// The system hostid.
    pub etc_hostid: PathBuf,
    /// The release information added to EFI bundles.
    pub os_release: PathBuf,
    /// Where device trees named by a relative `EFI.DeviceTree` are, per kernel
    /// version.
    pub dtb_dir: String,
    /// The EFI stubs tried when `EFI.Stub` is not set.
    pub default_stubs: Vec<String>,
    /// The PCI devices, where `Kernel.GraphicsDrivers` finds the GPUs of
    /// the build host.
    pub pci_devices: PathBuf,
    /// Reports a signal that should end the generator.
    pub signal: &'a dyn Fn() -> Option<zbm_sys::Signal>,
}

impl<'a> Context<'a> {
    /// The context of a real run, which `signal` stops.
    pub fn from_environment(
        runner: &'a dyn Runner,
        program: String,
        signal: &'a dyn Fn() -> Option<zbm_sys::Signal>,
    ) -> Context<'a> {
        let default_stubs = if cfg!(target_arch = "x86_64") {
            vec!["/usr/lib/systemd/boot/efi/linuxx64.efi.stub".to_owned()]
        } else if cfg!(target_arch = "aarch64") {
            vec!["/usr/lib/systemd/boot/efi/linuxaa64.efi.stub".to_owned()]
        } else {
            Vec::new()
        };
        Context {
            runner,
            program,
            tmpdir: std::env::var_os("TMPDIR").map(PathBuf::from),
            reproducible: std::env::var_os("SOURCE_DATE_EPOCH")
                .is_some_and(|epoch| !epoch.is_empty()),
            timestamp: std::env::var("SOURCE_DATE_EPOCH")
                .ok()
                .and_then(|epoch| epoch.trim().parse().ok()),
            spl_hostid: PathBuf::from("/sys/module/spl/parameters/spl_hostid"),
            etc_hostid: PathBuf::from("/etc/hostid"),
            os_release: PathBuf::from("/etc/os-release"),
            dtb_dir: "/boot/dtbs".to_owned(),
            default_stubs,
            pci_devices: PathBuf::from("/sys/bus/pci/devices"),
            signal,
        }
    }
}

/// The program's output streams.
pub struct Io<'a> {
    /// Standard output.
    pub stdout: &'a mut dyn Write,
    /// Standard error.
    pub stderr: &'a mut dyn Write,
}

/// Why the program stops early: Perl's `exit` with a status, or `die` with a
/// message and a status.
///
/// A Perl program that dies exits with `errno` when it is set, so a `die` after
/// a failed system call exits with that call's error number; [`Stop::fatal`]
/// keeps it. Other dies, such as a configuration section that is not a mapping,
/// exit with 255 here; Perl exits with whatever error number an earlier call
/// left behind.
#[derive(Debug)]
enum Stop {
    Exit(i32),
    Fatal(String, i32),
}

impl Stop {
    /// A `die` after a failed system call.
    fn fatal(message: String, error: &std::io::Error) -> Stop {
        Stop::Fatal(
            message,
            error.raw_os_error().filter(|&code| code > 0).unwrap_or(255),
        )
    }
}

impl From<Fatal> for Stop {
    fn from(fatal: Fatal) -> Stop {
        Stop::Fatal(fatal.0, 255)
    }
}

type Step<T> = Result<T, Stop>;

/// Runs the generator with the given arguments and returns its exit status.
pub fn run(context: &Context, args: &[String], io: &mut Io) -> i32 {
    let mut generator = Generator {
        context,
        io,
        debug: false,
        dry_run: false,
        config: Value::map(Vec::new()),
        mounted: None,
    };
    let code = match generator.main(args) {
        Ok(()) => 0,
        Err(Stop::Exit(code)) => code,
        Err(Stop::Fatal(message, code)) => {
            let _ = writeln!(generator.io.stderr, "{message}");
            code
        }
    };
    // Perl's END block.
    generator.cleanup_mount();
    code
}

struct Generator<'a, 'io, 'w> {
    context: &'a Context<'a>,
    io: &'io mut Io<'w>,
    debug: bool,
    /// `--dry-run`: changes are said, not made.
    dry_run: bool,
    config: Value,
    /// The boot partition, when the generator mounted it.
    mounted: Option<String>,
}

/// Whether an optional text has a value: defined and not empty.
fn has_value(text: Option<&String>) -> bool {
    text.is_some_and(|text| !text.is_empty())
}

/// What a shell prints when the signal `signal` ends a program, as `dash`
/// and Bash print it: nothing for an interrupt or a closed pipe.
fn signal_description(signal: i32) -> Option<&'static str> {
    Some(match signal {
        1 => "Hangup",
        3 => "Quit",
        4 => "Illegal instruction",
        5 => "Trace/breakpoint trap",
        6 => "Aborted",
        7 => "Bus error",
        8 => "Floating point exception",
        9 => "Killed",
        10 => "User defined signal 1",
        11 => "Segmentation fault",
        12 => "User defined signal 2",
        14 => "Alarm clock",
        15 => "Terminated",
        _ => return None,
    })
}

/// Splits program output into lines that keep their newline, as `qx` does in
/// list context.
fn output_lines(output: &[u8]) -> Vec<Vec<u8>> {
    output
        .split_inclusive(|byte| *byte == b'\n')
        .map(<[u8]>::to_vec)
        .collect()
}

impl Generator<'_, '_, '_> {
    fn print(&mut self, text: &str) {
        let _ = self.io.stdout.write_all(text.as_bytes());
    }

    fn print_lines(&mut self, lines: &[Vec<u8>]) {
        for line in lines {
            let _ = self.io.stdout.write_all(line);
        }
    }

    /// The one door of every change the generator makes: in a dry run,
    /// says `action` (`Would ACTION`) and returns `true`, for the caller to
    /// leave it undone; otherwise returns `false`.
    fn is_dry_run_of(&mut self, action: &str) -> bool {
        if self.dry_run {
            self.print(&format!("Would {action}\n"));
        }
        self.dry_run
    }

    /// Prints `text`, the outcome of a change, unless this is a dry run.
    fn done(&mut self, text: &str) {
        if !self.dry_run {
            self.print(text);
        }
    }

    /// Prints a debug message on standard error, when debugging.
    fn log(&mut self, entry: &str) {
        if self.debug {
            let entry = entry.strip_suffix('\n').unwrap_or(entry);
            let _ = writeln!(self.io.stderr, "## {entry}");
        }
    }

    fn log_lines(&mut self, lines: &[Vec<u8>]) {
        if self.debug {
            for line in lines {
                let _ = self.io.stderr.write_all(b"## ");
                let _ = self.io.stderr.write_all(perl::chomp(line));
                let _ = self.io.stderr.write_all(b"\n");
            }
        }
    }

    fn log_groups(&mut self, groups: &BTreeMap<String, Vec<String>>) {
        if self.debug {
            let _ = self
                .io
                .stderr
                .write_all(perl::dump_groups(groups).as_bytes());
        }
    }

    /// The configuration key `spec` as text, when it is defined.
    fn setting(&self, spec: &OptionSpec) -> Step<Option<String>> {
        Ok(self.config.option(spec)?.and_then(|value| value.text()))
    }

    /// Whether the `Enabled` key `spec` of a section is set to a true value.
    fn enabled(&self, spec: &OptionSpec) -> Step<bool> {
        Ok(self
            .config
            .option(spec)?
            .is_some_and(|value| value.truthy()))
    }

    /// Runs a program with standard error merged into its output, and
    /// returns the output lines and the exit status (128 plus the signal
    /// number for a program killed by a signal); a program that cannot start
    /// gets the shell's statuses and messages, 127 when it is not found, 126
    /// otherwise.
    fn run_program(&self, invocation: &Invocation) -> (Vec<Vec<u8>>, i32) {
        match programs::merged(
            self.context.runner,
            invocation.environment(),
            invocation.args(),
        ) {
            Ok(output) => {
                let mut lines = output_lines(&output.stdout);
                // What the shell said of a program that a signal ended
                if let Some(said) = output.status.signal().and_then(signal_description) {
                    lines.push(format!("{said}\n").into_bytes());
                }
                (lines, output.status.shell_code())
            }
            Err(error) => {
                let program = invocation.args().first().map_or("", String::as_str);
                let (why, code) = if error.kind() == std::io::ErrorKind::NotFound {
                    ("not found", 127)
                } else {
                    ("cannot run", 126)
                };
                (vec![format!("{program}: {why}\n").into_bytes()], code)
            }
        }
    }

    /// Runs a program, then stops if a signal arrived meanwhile.
    fn execute(&mut self, invocation: &Invocation) -> Step<(Vec<Vec<u8>>, i32)> {
        let result = self.run_program(invocation);
        self.check_signal()?;
        Ok(result)
    }

    /// The `INT` and `TERM` handler: unmount, report the signal and exit with
    /// status 1.
    fn check_signal(&mut self) -> Step<()> {
        if let Some(signal) = (self.context.signal)() {
            self.cleanup_mount();
            let message = format!(
                "{} terminating on signal {}\n",
                self.context.program,
                signal.name()
            );
            self.print(&message);
            return Err(Stop::Exit(1));
        }
        Ok(())
    }

    /// Unmounts the boot partition if the generator mounted it.
    fn cleanup_mount(&mut self) {
        if let Some(mount_point) = self.mounted.take() {
            self.print(&format!("Unmounting {mount_point}\n"));
            self.run_program(&Invocation::new("umount").quoted(&mount_point));
        }
    }

    fn main(&mut self, args: &[String]) -> Step<()> {
        let parsed = cli::parse(args);
        for warning in &parsed.warnings {
            let _ = self.io.stderr.write_all(warning.as_bytes());
        }
        let options = match parsed.outcome {
            Outcome::Help => {
                self.print(cli::HELP);
                return Err(Stop::Exit(1));
            }
            Outcome::Invalid => return Err(Stop::Exit(1)),
            Outcome::Run(options) => *options,
        };
        self.debug = options.debug;
        self.dry_run = options.dry_run;
        self.check_signal()?;

        if options.showver {
            self.print(&format!("{VERSION}\n"));
            return Err(Stop::Exit(0));
        }

        self.load_config(&options.config)?;

        let enable = if options.disable {
            Some(false)
        } else {
            options.enable.then_some(true)
        };
        if let Some(enable) = enable {
            return self.set_manage_images(&options.config, enable);
        }

        if !self
            .config
            .option(&GLOBAL_MANAGE_IMAGES)?
            .is_some_and(|value| value.truthy())
        {
            self.print("ManageImages not enabled, no action taken\n");
            return Err(Stop::Exit(0));
        }

        let mut settings = self.settings(&options)?;
        self.mount_boot()?;
        self.run_hooks(&GLOBAL_PRE_HOOKS_DIR)?;
        let work = self.work_dir()?;
        self.build(&options, &mut settings, &work)?;
        self.run_hooks(&GLOBAL_POST_HOOKS_DIR)?;
        drop(work);
        Ok(())
    }

    /// Chooses the kernel, then builds and installs the images the
    /// configuration enables.
    fn build(&mut self, options: &Options, settings: &mut Settings, work: &WorkDir) -> Step<()> {
        let request = self.apply_defaults(options, settings)?;
        let kernel = self.choose_kernel(&options.bootdir, request)?;
        self.choose_graphics(&kernel.version, settings)?;

        self.print(&format!(
            "Creating ZFSBootMenu {VERSION} from kernel {}\n",
            kernel.path
        ));
        self.check_hostid();

        let work_dir = work.0.display().to_string();
        let mut initramfs = String::new();
        if self.enabled(&EFI_ENABLED)? || self.enabled(&COMPONENTS_ENABLED)? {
            initramfs = self.create_initramfs(&work_dir, &kernel.version, settings)?;
        }
        if self.enabled(&EFI_ENABLED)? {
            let bundle = self.create_bundle(
                &work_dir,
                &kernel.path,
                &initramfs,
                &kernel.version,
                settings,
            )?;
            self.install_bundle(&bundle, &kernel.prefix, &settings.version)?;
        }
        if self.enabled(&COMPONENTS_ENABLED)? {
            self.install_components(&kernel.path, &initramfs, &kernel.prefix, &settings.version)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use system::canonical_path;

    #[test]
    fn signals_are_described_as_the_shell_describes_them() {
        assert_eq!(signal_description(9), Some("Killed"));
        assert_eq!(signal_description(15), Some("Terminated"));
        assert_eq!(signal_description(2), None);
        assert_eq!(signal_description(13), None);
        let all: Vec<_> = (1..=15).filter_map(signal_description).collect();
        assert_eq!(all.len(), 13);
    }

    #[test]
    fn canonical_paths() {
        assert_eq!(
            canonical_path(Path::new("/tmp//x/")),
            PathBuf::from("/tmp/x")
        );
        assert_eq!(canonical_path(Path::new("/")), PathBuf::from("/"));
        assert_eq!(canonical_path(Path::new("./a/./b")), PathBuf::from("a/b"));
        assert_eq!(canonical_path(Path::new(".")), PathBuf::from("."));
    }
}
