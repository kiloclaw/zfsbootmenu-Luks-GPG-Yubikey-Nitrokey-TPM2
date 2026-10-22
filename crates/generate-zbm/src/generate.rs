//! The program: the Rust counterpart of the main part of
//! `legacy/bin/generate-zbm`.
//!
//! The steps, messages and exit statuses follow the Perl program. Like it, the
//! generator runs every external program through `/bin/sh -c '<command> 2>&1'`:
//! the initramfs flags in the configuration are shell words, so the commands
//! must keep shell semantics, and paths are quoted. Perl's `exit` and `die`
//! become `Stop`; whichever way the program ends, the boot partition is
//! unmounted if the generator mounted it, as the Perl `END` block does.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use zbm_core::fs::{is_dir, is_executable, is_file, is_readable, is_writable, strerror};
use zbm_core::shell::quote;
use zbm_platform::exec::Runner;
use zbm_tools::programs;

use crate::cli::{self, Options, Outcome};
use crate::config::{Fatal, Value};
use crate::glob::glob;
use crate::kernel::{self, Decision};
use crate::version::versioncmp;
use crate::{dump, efi, images, perl};

/// The ZFSBootMenu version, `$VERSION` in the Perl program.
pub const VERSION: &str = zbm_core::version::ZBM_VERSION;

/// Where the generator finds things on the system; tests replace the paths.
pub struct Context<'a> {
    /// Runs the external programs.
    pub runner: &'a dyn Runner,
    /// The program name for messages, Perl's `$0`.
    pub program: String,
    /// Whether `POSIXLY_CORRECT` is set, which changes option parsing.
    pub posixly_correct: bool,
    /// `$TMPDIR`.
    pub tmpdir: Option<PathBuf>,
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
    /// Reports a signal that should end the generator.
    pub signal: &'a dyn Fn() -> Option<zbm_sys::Signal>,
}

impl<'a> Context<'a> {
    /// The context of a real run.
    pub fn from_environment(runner: &'a dyn Runner, program: String) -> Context<'a> {
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
            posixly_correct: std::env::var_os("POSIXLY_CORRECT").is_some(),
            tmpdir: std::env::var_os("TMPDIR").map(PathBuf::from),
            spl_hostid: PathBuf::from("/sys/module/spl/parameters/spl_hostid"),
            etc_hostid: PathBuf::from("/etc/hostid"),
            os_release: PathBuf::from("/etc/os-release"),
            dtb_dir: "/boot/dtbs".to_owned(),
            default_stubs,
            signal: &zbm_sys::received_signal,
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

/// A temporary directory removed when the program ends, as `File::Temp->newdir`
/// arranges.
struct WorkDir(PathBuf);

impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Runs the generator with the given arguments and returns its exit status.
pub fn run(context: &Context, args: &[String], io: &mut Io) -> i32 {
    let mut generator = Generator {
        context,
        io,
        debug: false,
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

/// The values the Perl program keeps in `%runConf` once the options and the
/// configuration are combined.
struct Settings {
    usecpio: bool,
    confd: String,
    hookd: Option<Vec<String>>,
    cmdline: Option<String>,
    version: String,
}

struct Generator<'a, 'io, 'w> {
    context: &'a Context<'a>,
    io: &'io mut Io<'w>,
    debug: bool,
    config: Value,
    /// The boot partition, when the generator mounted it.
    mounted: Option<String>,
}

/// Whether an optional text has a value: defined and not empty.
fn has_value(text: Option<&String>) -> bool {
    text.is_some_and(|text| !text.is_empty())
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

    /// `Log`: a debug message on standard error.
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

    /// `$config{section}{key}` as text, when it is defined.
    fn setting(&self, section: &str, key: &str) -> Step<Option<String>> {
        Ok(self
            .config
            .lookup(section, key)?
            .and_then(|value| value.perl_string()))
    }

    /// `enabled $config{section}`.
    fn enabled(&self, section: &str) -> Step<bool> {
        Ok(self
            .config
            .lookup(section, "Enabled")?
            .is_some_and(|value| value.truthy()))
    }

    /// `execute`: runs a shell command with standard error merged into its output,
    /// and returns the output lines and the exit status (128 plus the signal number
    /// for a command killed by a signal).
    fn run_shell(&self, command: &str) -> (Vec<Vec<u8>>, i32) {
        match programs::shell(self.context.runner, command) {
            Ok(output) => (output_lines(&output.stdout), output.status.shell_code()),
            // Perl reports a shell that cannot be started as status -1, which the fixed
            // `execute` turns into 255.
            Err(_) => (Vec::new(), 255),
        }
    }

    /// Runs a shell command, then stops if a signal arrived meanwhile.
    fn execute(&mut self, command: &str) -> Step<(Vec<Vec<u8>>, i32)> {
        let result = self.run_shell(command);
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

    /// `cleanupMount`: unmounts the boot partition if the generator mounted it.
    fn cleanup_mount(&mut self) {
        if let Some(mount_point) = self.mounted.take() {
            self.print(&format!("Unmounting {mount_point}\n"));
            self.run_shell(&format!("umount {}", quote(&mount_point)));
        }
    }

    fn main(&mut self, args: &[String]) -> Step<()> {
        let parsed = cli::parse(args, self.context.posixly_correct);
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
            .lookup("Global", "ManageImages")?
            .is_some_and(|value| value.truthy())
        {
            self.print("ManageImages not enabled, no action taken\n");
            return Err(Stop::Exit(0));
        }

        let mut settings = self.settings(&options)?;
        self.mount_boot()?;
        self.run_hooks("PreHooksDir")?;
        let work = self.work_dir()?;
        self.build(&options, &mut settings, &work)?;
        self.run_hooks("PostHooksDir")?;
        drop(work);
        Ok(())
    }

    fn load_config(&mut self, path: &str) -> Step<()> {
        if !is_readable(path) {
            self.print(&format!(
                "Configuration {path} does not exist or is unreadable\n"
            ));
            return Err(Stop::Exit(1));
        }
        let loaded = std::fs::read(path)
            .map_err(|error| strerror(&error))
            .and_then(|bytes| {
                crate::config::load(&perl::decode_utf8_perlqq(&bytes)).map_err(|error| error.0)
            });
        match loaded {
            Ok(config) => {
                self.config = config;
                Ok(())
            }
            Err(error) => {
                self.print(&format!("Unable to parse configuration {path} as YAML.\n"));
                let _ = writeln!(self.io.stderr, "{error}");
                Err(Stop::Exit(1))
            }
        }
    }

    /// `--enable` and `--disable`: rewrite the configuration with
    /// `Global.ManageImages` set.
    fn set_manage_images(&mut self, path: &str, enable: bool) -> Step<()> {
        self.config
            .set("Global", "ManageImages", Value::Bool(enable))?;
        let text = dump::dump(&self.config);
        let mut file = std::fs::File::create(path).map_err(|error| {
            Stop::fatal(
                format!("Could not open '{path}' for writing: {}", strerror(&error)),
                &error,
            )
        })?;
        file.write_all(text.as_bytes()).map_err(|error| {
            Stop::fatal(
                format!("Unable to write {path}: {}", strerror(&error)),
                &error,
            )
        })?;
        self.print(&format!("ManageImages set to '{enable}' in {path}\n"));
        Err(Stop::Exit(0))
    }

    /// Combines the options with the configuration: the initramfs generator, its
    /// configuration and hook directories.
    fn settings(&mut self, options: &Options) -> Step<Settings> {
        let usecpio = match options.initcpio {
            Some(usecpio) => usecpio,
            None => match self.config.lookup("Global", "InitCPIO")? {
                Some(value) => value.truthy(),
                None => {
                    let (_, status) = self.execute("sh -c \"command -v dracut\"")?;
                    if status == 0 {
                        self.print("No initramfs generator specified; using dracut\n");
                        false
                    } else {
                        self.print("No initramfs generator specified; using mkinitcpio\n");
                        true
                    }
                }
            },
        };

        let confd = match &options.confd {
            Some(confd) => confd.clone(),
            None => {
                let (default, key) = if usecpio {
                    ("/etc/zfsbootmenu/mkinitcpio.conf", "InitCPIOConfig")
                } else {
                    ("/etc/zfsbootmenu/dracut.conf.d", "DracutConfDir")
                };
                self.setting("Global", key)?
                    .unwrap_or_else(|| default.to_owned())
            }
        };

        let mut hookd = options.hookd.clone();
        if usecpio && hookd.is_none() {
            let hooks = match self.config.lookup("Global", "InitCPIOHookDirs")? {
                Some(Value::Seq(items)) => items
                    .borrow()
                    .iter()
                    .map(|item| item.perl_string().unwrap_or_default())
                    .collect(),
                Some(other) => vec![other.perl_string().unwrap_or_default()],
                None => Vec::new(),
            };
            hookd = Some(hooks);
        }

        Ok(Settings {
            usecpio,
            confd,
            hookd,
            cmdline: options.cmdline.clone(),
            version: String::new(),
        })
    }

    /// Mounts `Global.BootMountPoint` unless it is mounted already.
    fn mount_boot(&mut self) -> Step<()> {
        let Some(mount_point) = self
            .setting("Global", "BootMountPoint")?
            .filter(|text| !text.is_empty())
        else {
            return Ok(());
        };
        let (_, status) = self.execute(&format!("mountpoint {}", quote(&mount_point)))?;
        if status != 0 {
            self.print(&format!("Mounting {mount_point}\n"));
            let (output, status) = self.execute(&format!("mount {}", quote(&mount_point)))?;
            if status == 0 {
                self.mounted = Some(mount_point);
            } else {
                self.print_lines(&output);
                self.print(&format!("Unable to mount {mount_point}\n"));
                return Err(Stop::Exit(status));
            }
        }
        Ok(())
    }

    /// Runs the executables in `Global.PreHooksDir` or `Global.PostHooksDir`, in
    /// glob order.
    fn run_hooks(&mut self, key: &str) -> Step<()> {
        let Some(dir) = self.setting("Global", key)?.filter(|text| !text.is_empty()) else {
            return Ok(());
        };
        if !is_dir(&dir) {
            return Ok(());
        }
        for hook in glob(&format!("{dir}/*")) {
            if !is_executable(&hook) {
                continue;
            }
            self.log(&format!("Processing hook: {hook}"));
            // The Perl program logs the whole result of `execute`, whose last element is
            // the exit status.
            let (mut output, status) = self.execute(&quote(&hook))?;
            output.push(status.to_string().into_bytes());
            self.log_lines(&output);
        }
        Ok(())
    }

    /// File::Spec's `tmpdir`: `$TMPDIR` when it is a writable directory, otherwise
    /// `/tmp`, otherwise `.`.
    fn temp_base(&self) -> PathBuf {
        let candidates = [self.context.tmpdir.clone(), Some(PathBuf::from("/tmp"))];
        let base = candidates
            .into_iter()
            .flatten()
            .find(|dir| is_dir(dir) && is_writable(dir))
            .unwrap_or_else(|| PathBuf::from("."));
        let base = canonical_path(&base);
        if base.is_absolute() {
            base
        } else {
            std::env::current_dir().map_or(base.clone(), |cwd| cwd.join(&base))
        }
    }

    fn work_dir(&mut self) -> Step<WorkDir> {
        let base = self.temp_base();
        zbm_core::fs::create_temp_dir(&base, "XXXXXXXXXX")
            .map(WorkDir)
            .map_err(|error| {
                Stop::fatal(
                    format!(
                        "Error in tempdir() using {}/XXXXXXXXXX: Could not create directory: {}",
                        base.display(),
                        strerror(&error)
                    ),
                    &error,
                )
            })
    }

    fn build(&mut self, options: &Options, settings: &mut Settings, work: &WorkDir) -> Step<()> {
        // Configuration values supply defaults for options that are not given.
        let mut kernel = options.kernel.clone();
        let mut prefix = options.kernel_prefix.clone();
        let mut kernel_version = options.kernel_version.clone();
        let mut version = options.version.clone();
        if let Some(path) = self
            .setting("Kernel", "Path")?
            .filter(|text| !text.is_empty())
            && !has_value(kernel.as_ref())
        {
            kernel = Some(path);
        }
        if let Some(value) = self
            .setting("Kernel", "Prefix")?
            .filter(|text| !text.is_empty())
            && !has_value(prefix.as_ref())
        {
            prefix = Some(value);
        }
        if let Some(value) = self
            .setting("Kernel", "Version")?
            .filter(|text| !text.is_empty())
            && !has_value(kernel_version.as_ref())
        {
            kernel_version = Some(perl::mark_current(&value));
        }
        if let Some(value) = self
            .setting("Global", "Version")?
            .filter(|text| !text.is_empty())
            && !has_value(version.as_ref())
        {
            version = Some(value);
        }
        if let Some(value) = self
            .setting("Kernel", "CommandLine")?
            .filter(|text| !text.is_empty())
            && !has_value(settings.cmdline.as_ref())
        {
            settings.cmdline = Some(value);
        }
        settings.version = match version.filter(|text| !text.is_empty()) {
            Some(version) => perl::replace_current(&perl::mark_current(&version), VERSION),
            None => VERSION.to_owned(),
        };

        // `%{current}` in the kernel version is the running kernel.
        if let Some(value) = kernel_version
            .clone()
            .filter(|text| !text.is_empty() && perl::has_current(text))
        {
            let (uname, status) = self.execute("uname -r")?;
            if status != 0 || uname.is_empty() {
                self.print("Cannot determine current kernel version\n");
                return Err(Stop::Exit(if status == 0 { 1 } else { status }));
            }
            let running = String::from_utf8_lossy(perl::chomp(&uname[0])).into_owned();
            kernel_version = Some(perl::replace_current(&value, &running));
        }

        let kernel = match kernel.filter(|text| !text.is_empty()) {
            Some(kernel) => {
                if !is_file(&kernel) {
                    self.print(&format!(
                        "The provided kernel {kernel} was not found, unable to continue\n"
                    ));
                    return Err(Stop::Exit(1));
                }
                kernel
            }
            None => match kernel_version.clone().filter(|text| !text.is_empty()) {
                Some(wanted) => match versioned_kernel(&options.bootdir, &wanted) {
                    Some((kernel, exact)) => {
                        // An inexact match leaves the version to be found from the kernel.
                        if !exact {
                            kernel_version = None;
                        }
                        kernel
                    }
                    None => {
                        self.print(&format!(
                            "Unable to find file for kernel version {wanted}\n"
                        ));
                        return Err(Stop::Exit(1));
                    }
                },
                None => match self.latest_kernel(&options.bootdir)? {
                    Some(kernel) => kernel,
                    None => {
                        self.print(
                            "Unable to find latest kernel; specify version or path manually\n",
                        );
                        return Err(Stop::Exit(1));
                    }
                },
            },
        };

        let kernel_version = match kernel_version.filter(|text| !text.is_empty()) {
            Some(version) => version,
            None => match self.kernel_version(&kernel)? {
                Some(version) => version,
                None => {
                    self.print(&format!(
                        "Unable to determine kernel version from {kernel}\n"
                    ));
                    return Err(Stop::Exit(1));
                }
            },
        };

        let prefix = match prefix.filter(|text| !text.is_empty()) {
            Some(prefix) => prefix,
            None => {
                let prefix = perl::strip_version_suffix(&perl::basename(&kernel), &kernel_version);
                if prefix.is_empty() {
                    self.print(&format!(
                        "Unable to determine kernel prefix from {kernel}\n"
                    ));
                    return Err(Stop::Exit(1));
                }
                prefix
            }
        };

        self.print(&format!(
            "Creating ZFSBootMenu {VERSION} from kernel {kernel}\n"
        ));
        self.check_hostid();

        let work_dir = work.0.display().to_string();
        let mut initramfs = String::new();
        if self.enabled("EFI")? || self.enabled("Components")? {
            initramfs = self.create_initramfs(&work_dir, &kernel_version, settings)?;
        }
        if self.enabled("EFI")? {
            let bundle =
                self.create_bundle(&work_dir, &kernel, &initramfs, &kernel_version, settings)?;
            self.install_bundle(&bundle, &prefix, &settings.version)?;
        }
        if self.enabled("Components")? {
            self.install_components(&kernel, &initramfs, &prefix, &settings.version)?;
        }
        Ok(())
    }

    /// Warns when the SPL module's hostid and `/etc/hostid` differ.
    fn check_hostid(&mut self) {
        let module = if is_file(&self.context.spl_hostid) {
            let text = std::fs::read(&self.context.spl_hostid).unwrap_or_default();
            let first = text.split_inclusive(|byte| *byte == b'\n').next();
            first.map_or_else(
                || "00000000".to_owned(),
                |line| perl::hex_of_numeric_text(&String::from_utf8_lossy(line)),
            )
        } else {
            "00000000".to_owned()
        };
        if module == "00000000" || !is_file(&self.context.etc_hostid) {
            return;
        }
        let bytes = std::fs::read(&self.context.etc_hostid).unwrap_or_default();
        let system = match bytes.get(..4) {
            Some(word) => format!(
                "{:08x}",
                u32::from_le_bytes([word[0], word[1], word[2], word[3]])
            ),
            None => "00000000".to_owned(),
        };
        if module != system {
            self.print(&format!(
                "SPL ({module}) and system ({system}) hostids do not match!\n"
            ));
        }
    }

    /// `latestKernel`: the kernel with the highest version among the first prefix
    /// that has any.
    fn latest_kernel(&mut self, bootdir: &str) -> Step<Option<String>> {
        for prefix in ["vmlinuz*", "vmlinux*", "linux*", "kernel*"] {
            let mut kernels: BTreeMap<String, String> = BTreeMap::new();
            for kernel in glob(&format!("{bootdir}/{prefix}")) {
                let Some(version) = self.kernel_version(&kernel)? else {
                    continue;
                };
                self.log(&format!("Identified version {version} for kernel {kernel}"));
                kernels.insert(version, kernel);
            }
            let mut versions: Vec<&String> = kernels.keys().collect();
            versions.sort_by(|a, b| versioncmp(b, a));
            if let Some(latest) = versions.first() {
                self.log(&format!("Latest kernel: {latest}"));
                return Ok(Some(kernels[*latest].clone()));
            }
        }
        Ok(None)
    }

    /// `kernelVersion`: the version from the file name and from the kernel's
    /// strings.
    fn kernel_version(&mut self, kernel: &str) -> Step<Option<String>> {
        if !is_readable(kernel) {
            self.log(&format!(
                "Unable to read path {kernel}, assuming no version"
            ));
            return Ok(None);
        }
        let from_name = kernel::version_from_name(&perl::basename(kernel));
        let (lines, status) = self.execute(&format!("strings {}", quote(kernel)))?;
        let lines = if status == 0 { lines } else { Vec::new() };
        let decision = kernel::decide(from_name.as_deref(), lines.iter().map(Vec::as_slice));
        Ok(match decision {
            Decision::None => None,
            Decision::FromName(version) => {
                self.log(&format!(
                    "No version found in kernel strings, using {version} from path {kernel}"
                ));
                Some(version)
            }
            Decision::FromStrings(version) => {
                self.log(&format!(
                    "No version found in path {kernel}, using {version} from kernel strings"
                ));
                Some(version)
            }
            Decision::Agreed(version) => Some(version),
            Decision::Inconsistent { name, strings } => {
                self.print(&format!(
                    "WARNING: ignoring inconsistent versions in kernel {kernel}:\n  \
                     Path suggests version {name}.\n  Kernel strings suggest version {strings}.\n  \
                     To use this kernel, explicitly specify the path and version.\n"
                ));
                None
            }
        })
    }

    /// Runs a command built for the initramfs or the bundle: its output is logged
    /// when it succeeds and printed when it fails, and failure ends the program
    /// with the command's status.
    fn run_build_step(&mut self, words: &[String], output_file: &str) -> Step<()> {
        let command = words.join(" ");
        self.log(&format!("Executing: {command}"));
        let (lines, status) = self.execute(&command)?;
        if status == 0 {
            self.log_lines(&lines);
            Ok(())
        } else {
            self.print_lines(&lines);
            self.print(&format!("Failed to create {output_file}\n"));
            Err(Stop::Exit(status))
        }
    }

    /// `createInitramfs`: builds the initramfs with dracut or mkinitcpio.
    fn create_initramfs(
        &mut self,
        dir: &str,
        kernel_version: &str,
        settings: &Settings,
    ) -> Step<String> {
        let output = format!("{dir}/zfsbootmenu.img");
        let mut words: Vec<String> = Vec::new();
        let flags_key = if settings.usecpio {
            words.extend([
                "mkinitcpio".to_owned(),
                "--config".to_owned(),
                quote(&settings.confd),
            ]);
            if self.debug {
                words.push("-v".to_owned());
            }
            for hook_dir in settings.hookd.iter().flatten() {
                words.push("--hookdir".to_owned());
                words.push(quote(hook_dir));
            }
            "InitCPIOFlags"
        } else {
            words.extend([
                "dracut".to_owned(),
                "-f".to_owned(),
                "--confdir".to_owned(),
                quote(&settings.confd),
            ]);
            if !self.debug {
                words.push("-q".to_owned());
            }
            "DracutFlags"
        };
        // The flags are shell words, as written in the configuration.
        match self.config.lookup("Global", flags_key)? {
            Some(Value::Seq(flags)) => words.extend(
                flags
                    .borrow()
                    .iter()
                    .map(|flag| flag.perl_string().unwrap_or_default()),
            ),
            Some(flag) => words.push(flag.perl_string().unwrap_or_default()),
            None => {}
        }
        if settings.usecpio {
            words.extend([
                "-A".to_owned(),
                "zfsbootmenu".to_owned(),
                "--generate".to_owned(),
                quote(&output),
                "--kernel".to_owned(),
                quote(kernel_version),
            ]);
        } else {
            words.extend([quote(&output), quote(kernel_version)]);
        }
        self.run_build_step(&words, &output)?;
        Ok(output)
    }

    /// `increaseBundleOffset`.
    fn next_offset(&mut self, size: u128, offset: u128, alignment: u128) -> u128 {
        let next = efi::next_offset(size, offset, alignment);
        self.log(&format!("New offset is: 0x{next:X}"));
        next
    }

    /// `addBundleSection`.
    fn add_section(
        &mut self,
        words: &mut Vec<String>,
        name: &str,
        file: &str,
        offset: u128,
        alignment: u128,
    ) -> Step<u128> {
        words.push("--add-section".to_owned());
        words.push(format!("{name}={}", quote(file)));
        words.push("--change-section-vma".to_owned());
        words.push(format!("{name}=0x{offset:X}"));
        let size = std::fs::metadata(file)
            .map_err(|error| {
                Stop::fatal(
                    "Can't call method \"size\" on an undefined value".to_owned(),
                    &error,
                )
            })?
            .len();
        Ok(self.next_offset(u128::from(size), offset, alignment))
    }

    /// `createUEFIBundle`: combines the stub, the release information, the command
    /// line, a splash image, a device tree, the initramfs and the kernel with
    /// objcopy.
    fn create_bundle(
        &mut self,
        dir: &str,
        kernel: &str,
        initramfs: &str,
        kernel_version: &str,
        settings: &Settings,
    ) -> Step<String> {
        let output = format!("{dir}/zfsbootmenu.EFI");
        if !(is_file(kernel) && is_file(initramfs)) {
            self.print("Cannot find kernel or initramfs to create UEFI bundle\n");
            return Err(Stop::Exit(1));
        }

        let stub = match self.setting("EFI", "Stub")?.filter(|text| !text.is_empty()) {
            Some(stub) => {
                if !is_file(&stub) {
                    self.print(&format!("UEFI stub loader '{stub}' does not exist\n"));
                    return Err(Stop::Exit(1));
                }
                stub
            }
            None => match self.context.default_stubs.iter().find(|stub| is_file(stub)) {
                Some(stub) => stub.clone(),
                None => {
                    self.print("Unable to find UEFI stub loader at default locations:\n");
                    for stub in &self.context.default_stubs.clone() {
                        self.print(&format!(" {stub}\n"));
                    }
                    return Err(Stop::Exit(1));
                }
            },
        };

        let (lines, status) = self.execute(&format!("objdump -p {}", quote(&stub)))?;
        let mut alignment = None;
        if status == 0 {
            for line in &lines {
                if let Some(found) = efi::section_alignment(line) {
                    self.log(&format!("Alignment is: {found}"));
                    alignment = Some(found);
                }
            }
        }
        let alignment = match alignment {
            Some(alignment) if status == 0 && alignment > 0 => u128::from(alignment),
            _ => {
                self.print("Unable to determine stub alignment!\n");
                return Err(Stop::Exit(1));
            }
        };

        let (lines, status) = self.execute(&format!("objdump -w -h {}", quote(&stub)))?;
        let end = lines.last().and_then(|line| efi::last_section_end(line));
        let Some(end) = end.filter(|_| status == 0) else {
            self.print("Unable to determine initial stub offset!\n");
            return Err(Stop::Exit(1));
        };
        let mut offset = self.next_offset(end, 0, alignment);
        self.log(&format!("Initial offset is: 0x{offset:X}"));

        let mut words = vec!["objcopy".to_owned()];
        let os_release = self.context.os_release.display().to_string();
        if is_file(&os_release) {
            offset = self.add_section(&mut words, ".osrel", &os_release, offset, alignment)?;
        }
        if let Some(cmdline) = settings.cmdline.clone().filter(|text| !text.is_empty()) {
            let file = format!("{dir}/cmdline.txt");
            let _ = std::fs::write(&file, cmdline);
            offset = self.add_section(&mut words, ".cmdline", &file, offset, alignment)?;
        }
        if let Some(splash) = self
            .setting("EFI", "SplashImage")?
            .filter(|text| !text.is_empty())
            && is_file(&splash)
        {
            offset = self.add_section(&mut words, ".splash", &splash, offset, alignment)?;
        }
        if let Some(tree) = self
            .setting("EFI", "DeviceTree")?
            .filter(|text| !text.is_empty())
        {
            let path = if tree.starts_with('/') {
                tree.replacen("%{kernel}", kernel_version, 1)
            } else {
                format!("{}/dtbs-{kernel_version}/{tree}", self.context.dtb_dir)
            };
            if !is_file(&path) {
                self.print(&format!(
                    "EFI.DeviceTree key set to '{tree}', file not found at '{path}'\n"
                ));
                return Err(Stop::Exit(1));
            }
            offset = self.add_section(&mut words, ".dtb", &path, offset, alignment)?;
        }
        offset = self.add_section(&mut words, ".initrd", initramfs, offset, alignment)?;
        // The kernel goes last, so that it can decompress without overwriting other
        // sections.
        self.add_section(&mut words, ".linux", kernel, offset, alignment)?;
        words.push(quote(&stub));
        words.push(quote(&output));
        self.run_build_step(&words, &output)?;
        Ok(output)
    }

    /// `safeCopy` without keeping times, as the Perl program always calls it.
    fn safe_copy(&mut self, source: &str, dest: &str) -> bool {
        self.log(&format!("safeCopy called with: {source}, {dest}, 0"));
        match file_copy(source, dest) {
            Ok(()) => true,
            Err(error) => {
                self.print(&format!(
                    "Unable to copy {source} to {dest}: {}\n",
                    strerror(&error)
                ));
                false
            }
        }
    }

    /// `verboseUnlink`: removes a file if it exists, printing `message` first, or a
    /// confirmation when there is no message.
    fn verbose_unlink(&mut self, file: &str, message: Option<&str>) {
        if !is_file(file) {
            return;
        }
        if let Some(message) = message.filter(|message| !message.is_empty()) {
            self.print(&format!("{message}\n"));
        }
        match std::fs::remove_file(file) {
            Ok(()) => {
                if message.is_none() {
                    self.print(&format!("Removed file {file}\n"));
                }
            }
            Err(error) => {
                self.print(&format!(
                    "ERROR: unable to remove {file}: {}\n",
                    strerror(&error)
                ));
            }
        }
    }

    /// `maxRevision`, with its log message.
    fn max_revision(&mut self, files: Option<&Vec<String>>, suffix: &str) -> u64 {
        let revision = files.map_or(0, |files| images::max_revision(files, suffix));
        self.log(&format!("maxRevision discovered: {revision}"));
        revision
    }

    /// `pruneVersions`, with its log messages.
    fn prune(&mut self, groups: &BTreeMap<String, Vec<String>>, current: &str, keep: i64) {
        self.log(&format!("pruneVersions called with: {current}, {keep}"));
        self.log_groups(groups);
        let keep = u64::try_from(keep).unwrap_or(0);
        for file in images::prune_versions(groups, current, keep) {
            self.verbose_unlink(&file, None);
        }
    }

    fn temp_file(&self, dir: &str, template: &str) -> Step<String> {
        zbm_core::fs::create_temp_file(Path::new(dir), template)
            .map(|path| path.display().to_string())
            .map_err(|error| {
                let template = if dir.is_empty() {
                    template.to_owned()
                } else {
                    format!("{dir}/{template}")
                };
                Stop::fatal(
                    format!(
                        "Error in tempfile() using template {template}: \
                         Could not create temp file: {}",
                        strerror(&error)
                    ),
                    &error,
                )
            })
    }

    /// Renames a file, reporting whether it worked.
    fn rename(from: &str, to: &str) -> bool {
        std::fs::rename(from, to).is_ok()
    }

    /// Places the EFI bundle in `EFI.ImageDir`, as a new revision or replacing the
    /// previous image.
    fn install_bundle(&mut self, bundle: &str, prefix: &str, version: &str) -> Step<()> {
        let image_dir = self.setting("EFI", "ImageDir")?.unwrap_or_default();
        let efi_prefix = format!("{image_dir}/{prefix}");
        self.log(&format!("Setting $efi_prefix: {efi_prefix}"));
        let versions = self
            .config
            .lookup("EFI", "Versions")?
            .map_or(0, |value| value.perl_int());
        make_path(&image_dir)?;

        let target;
        if versions > 0 {
            self.log(&format!("EFI.Versions is {versions}"));
            let existing = glob(&format!("{efi_prefix}-*.EFI"));
            let groups = images::group_versions(&existing, &efi_prefix, ".EFI");
            self.log_groups(&groups);
            let revision = self.max_revision(groups.get(version), ".EFI") + 1;
            target = format!("{efi_prefix}-{version}_{revision}.EFI");
            self.log(&format!("Setting $efi_target: {target}"));
            if !self.safe_copy(bundle, &target) {
                self.verbose_unlink(&target, Some(&format!("Failed to create {target}")));
                return Err(Stop::Exit(1));
            }
            self.prune(&groups, version, versions);
        } else {
            target = format!("{efi_prefix}.EFI");
            let temp = self.temp_file(&image_dir, "zbm.XXXXXX")?;
            if !self.safe_copy(bundle, &temp) {
                self.verbose_unlink(&temp, Some(&format!("Failed to create {target}")));
                return Err(Stop::Exit(1));
            }
            let backup = format!("{efi_prefix}-backup.EFI");
            if is_file(&target) && Self::rename(&target, &backup) {
                self.print(&format!("Created backup {target} -> {backup}\n"));
            }
            if !Self::rename(&temp, &target) {
                self.verbose_unlink(&temp, Some(&format!("Failed to create {target}")));
                return Err(Stop::Exit(1));
            }
        }
        self.print(&format!("Created new UEFI image {target}\n"));
        Ok(())
    }

    /// Places the kernel and the initramfs in `Components.ImageDir`.
    fn install_components(
        &mut self,
        kernel: &str,
        initramfs: &str,
        prefix: &str,
        version: &str,
    ) -> Step<()> {
        let image_dir = self.setting("Components", "ImageDir")?.unwrap_or_default();
        let component_prefix = format!("{image_dir}/{prefix}");
        let versions = self
            .config
            .lookup("Components", "Versions")?
            .map_or(0, |value| value.perl_int());
        make_path(&image_dir)?;

        let (kernel_target, initramfs_target);
        if versions > 0 {
            let existing = glob(&format!("{component_prefix}-*"));
            let mut groups = images::group_versions(&existing, &component_prefix, "");
            let revision = self.max_revision(groups.get(version), "") + 1;
            kernel_target = format!("{component_prefix}-{version}_{revision}");
            initramfs_target = format!("{image_dir}/initramfs-{version}_{revision}.img");
            if !self.safe_copy(initramfs, &initramfs_target) {
                self.verbose_unlink(
                    &initramfs_target,
                    Some(&format!("Failed to create {initramfs_target}")),
                );
                return Err(Stop::Exit(1));
            }
            if !self.safe_copy(kernel, &kernel_target) {
                self.verbose_unlink(
                    &kernel_target,
                    Some(&format!("Failed to create {kernel_target}")),
                );
                self.verbose_unlink(&initramfs_target, Some(""));
                return Err(Stop::Exit(1));
            }
            self.prune(&groups, version, versions);
            // Each kernel has an initramfs of the same version and revision; prune those
            // too.
            let initramfs_prefix = format!("{image_dir}/initramfs");
            for files in groups.values_mut() {
                for file in files.iter_mut() {
                    *file = format!(
                        "{}.img",
                        file.replacen(&component_prefix, &initramfs_prefix, 1)
                    );
                }
            }
            self.prune(&groups, version, versions);
        } else {
            kernel_target = format!("{component_prefix}-bootmenu");
            initramfs_target = format!("{image_dir}/initramfs-bootmenu.img");
            let init_temp = self.temp_file(&image_dir, "init.XXXXXX")?;
            if !self.safe_copy(initramfs, &init_temp) {
                self.verbose_unlink(
                    &init_temp,
                    Some(&format!("Failed to create {initramfs_target}")),
                );
                return Err(Stop::Exit(1));
            }
            let kern_temp = self.temp_file(&image_dir, "kern.XXXXXX")?;
            if !self.safe_copy(kernel, &kern_temp) {
                self.verbose_unlink(
                    &kern_temp,
                    Some(&format!("Failed to create {kernel_target}")),
                );
                self.verbose_unlink(&init_temp, Some(""));
                return Err(Stop::Exit(1));
            }
            let kernel_backup = format!("{kernel_target}-backup");
            if is_file(&kernel_target) && Self::rename(&kernel_target, &kernel_backup) {
                self.print(&format!(
                    "Created backup {kernel_target} -> {kernel_backup}\n"
                ));
            }
            let initramfs_backup = format!("{image_dir}/initramfs-bootmenu-backup.img");
            if is_file(&initramfs_target) && Self::rename(&initramfs_target, &initramfs_backup) {
                self.print(&format!(
                    "Created backup {initramfs_target} -> {initramfs_backup}\n"
                ));
            }
            if !Self::rename(&init_temp, &initramfs_target) {
                self.verbose_unlink(
                    &init_temp,
                    Some(&format!("Failed to create {initramfs_target}")),
                );
                self.verbose_unlink(&kern_temp, Some(""));
                return Err(Stop::Exit(1));
            }
            if !Self::rename(&kern_temp, &kernel_target) {
                self.verbose_unlink(
                    &kern_temp,
                    Some(&format!("Failed to create {kernel_target}")),
                );
                return Err(Stop::Exit(1));
            }
        }
        self.print(&format!("Created initramfs image {initramfs_target}\n"));
        self.print(&format!("Created kernel image {kernel_target}\n"));
        Ok(())
    }
}

/// `versionedKernel`: a kernel in the boot directory for a version, exactly or
/// by glob.
fn versioned_kernel(bootdir: &str, version: &str) -> Option<(String, bool)> {
    for prefix in ["vmlinuz", "linux", "vmlinux", "kernel"] {
        let pattern = format!("{bootdir}/{prefix}-{version}");
        if is_file(&pattern) {
            return Some((pattern, true));
        }
        let mut kernels = glob(&pattern);
        kernels.sort_by(|a, b| versioncmp(a, b));
        if let Some(kernel) = kernels.into_iter().find(|kernel| is_file(kernel)) {
            return Some((kernel, false));
        }
    }
    None
}

/// File::Path's `make_path`: creates a directory and its missing parents;
/// failing to create one is fatal.
fn make_path(path: &str) -> Step<()> {
    if path.is_empty() || is_dir(path) {
        return Ok(());
    }
    let parent = perl::dirname(path);
    if !(is_dir(&parent) || parent == path) {
        make_path(&parent)?;
    }
    match std::fs::create_dir(path) {
        Err(error) if !is_dir(path) => Err(Stop::fatal(
            format!("mkdir {path}: {}", strerror(&error)),
            &error,
        )),
        _ => Ok(()),
    }
}

/// File::Copy's `copy`: copies the contents into `dest` (into a file of the
/// same name when `dest` is a directory), creating it with the default
/// permissions or truncating it.
fn file_copy(source: &str, dest: &str) -> std::io::Result<()> {
    let mut dest = PathBuf::from(dest);
    if dest.is_dir() && !Path::new(source).is_dir() {
        dest = dest.join(perl::basename(source));
    }
    let mut from = std::fs::File::open(source)?;
    let mut to = std::fs::File::create(&dest)?;
    let mut buffer = vec![0_u8; 1 << 20];
    loop {
        let count = std::io::Read::read(&mut from, &mut buffer)?;
        if count == 0 {
            break;
        }
        to.write_all(&buffer[..count])?;
    }
    Ok(())
}

/// File::Spec's `canonpath` on Unix: no repeated `/`, no `/.` components, no
/// leading `./` and no trailing `/`.
fn canonical_path(path: &Path) -> PathBuf {
    let text = path.display().to_string();
    let mut parts: Vec<&str> = Vec::new();
    for (index, part) in text.split('/').enumerate() {
        if part.is_empty() && index > 0 || part == "." && (index > 0 || text.len() > 1) {
            continue;
        }
        parts.push(part);
    }
    let joined = parts.join("/");
    if joined.is_empty() {
        PathBuf::from(if text.starts_with('/') { "/" } else { "." })
    } else {
        PathBuf::from(joined)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn the_version_matches_the_perl_program() {
        let mut parity = zbm_testkit::parity!("the_version_matches_the_perl_program");
        let line: String = parity.legacy("our $VERSION", || {
            let source =
                std::fs::read_to_string(zbm_testkit::legacy::path("bin/generate-zbm")).unwrap();
            let line = source.lines().find(|line| line.starts_with("our $VERSION"));
            line.unwrap().to_owned()
        });
        assert_eq!(line, format!("our $VERSION = '{VERSION}';"));
    }
}
