//! `zbm-kcl`: review or change the kernel command line of a ZFS boot
//! environment or a ZFSBootMenu EFI image.
//!
//! This is the Rust version of the Bash `zbm-kcl`, with the same options,
//! messages and exit codes. The command line of a boot environment is its
//! `org.zfsbootmenu:commandline` property; that of an EFI image is its
//! `.cmdline` section, which `objcopy` reads and replaces. Each step does what
//! the step of the same name in the Bash script does, including its quirks: a
//! failure to read the command line is reported but does not end the program,
//! and a new `.cmdline` section holds the command line and a NUL byte, without
//! the leading space the script meant to add.

#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        reason = "tests describe the commands they expect"
    )
)]

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::getopts::Opt;
use zbm_config::kcl;
use zbm_config::spec::properties::COMMANDLINE;
use zbm_platform::exec::{Runner, SystemRunner};
use zbm_platform::filetest::{copy_like_cp, create_temp_file, is_file, is_readable};
use zbm_tools::efi as efi_tools;
use zbm_zfs::ZfsCommand;
use zbm_zfs::mounts::{MountTable, RootFilesystem};

mod edit;
pub mod efi;
mod getopts;
mod save;

pub use edit::strip_kcl;

/// What `zbm-kcl` depends on outside its arguments.
pub struct Context<'a> {
    /// Runs `zfs`, `objcopy`, `objdump` and the editor.
    pub runner: &'a dyn Runner,
    /// The name of the program in messages: the script's `$0`.
    pub program: String,
    /// The mount table that tells the root filesystem, normally `/proc/mounts`.
    pub mounts: PathBuf,
    /// Where temporary files go, normally `$TMPDIR` or `/tmp`.
    pub temp_dir: PathBuf,
    /// `$EDITOR`.
    pub editor: Option<String>,
    /// `$PATH`, to find the editor.
    pub path: Option<OsString>,
}

impl Context<'static> {
    /// The context of the running program, invoked as `argv0`.
    pub fn from_environment(argv0: &str) -> Context<'static> {
        static RUNNER: SystemRunner = SystemRunner;
        let path = std::env::var_os("PATH");
        Context {
            runner: &RUNNER,
            program: script_name(argv0, path.as_deref()),
            mounts: PathBuf::from("/proc/mounts"),
            temp_dir: std::env::temp_dir(),
            editor: std::env::var("EDITOR").ok(),
            path,
        }
    }
}

/// The streams of the program.
pub struct Io<'a> {
    /// Standard input: an EFI image when the input is `-`.
    pub stdin: &'a mut dyn Read,
    /// Standard output: the command line, the usage text, or an EFI image when the
    /// output is `-`.
    pub stdout: &'a mut dyn Write,
    /// Standard error: error messages.
    pub stderr: &'a mut dyn Write,
}

/// The name Bash gives a script in `$0`: the path the program was run by, which
/// is the `PATH` entry when the program was run by name.
pub fn script_name(argv0: &str, path: Option<&std::ffi::OsStr>) -> String {
    if argv0.contains('/') {
        return argv0.to_owned();
    }
    find_in_path(argv0, path).map_or_else(
        || argv0.to_owned(),
        |found| found.to_string_lossy().into_owned(),
    )
}

/// Finds an executable file named `name` in `path`, as `command -v` does.
fn find_in_path(name: &str, path: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    let path = path?;
    std::env::split_paths(path)
        .map(|dir| {
            if dir.as_os_str().is_empty() {
                PathBuf::from(name)
            } else {
                dir.join(name)
            }
        })
        .find(|candidate| is_file(candidate) && zbm_platform::filetest::is_executable(candidate))
}

/// Whether `command -v name` finds `name`: an executable file at a path, or one
/// found in `path`.
fn command_exists(name: &str, path: Option<&std::ffi::OsStr>) -> bool {
    if name.contains('/') {
        is_file(name) && zbm_platform::filetest::is_executable(name)
    } else {
        find_in_path(name, path).is_some()
    }
}

/// The usage text, as the script prints it.
pub fn usage(program: &str) -> String {
    format!(
        "USAGE: {program} [-a <arg>] [-r <arg>] [-o <out>] [-e] [-d] [bootenv|zbm-efi]

Review or update kernel command line (KCL) associated with the ZFS boot
environment or <bootenv> or the ZFSBootMenu EFI executable <zbm-efi>.

When the boot environment or EFI executable is unspecified, the current
root filesystem will be used by default. If \"-\" is passed, stdin will
be read as an EFI executable.

ARGUMENTS
-a <arg>: Append an argument
-r <arg>: Remove an argument
-o <out>: Write output to <out> filesystem or executable
-e: Open the KCL for editing in $EDITOR
-d: Remove entire command line
"
    )
}

/// Why a step failed: the `ERROR:` messages to print, in order.
pub(crate) struct Failure(Vec<String>);

impl Failure {
    /// The failure with the one message `message`.
    fn of(message: String) -> Failure {
        Failure(vec![message])
    }
}

/// The temporary files of a run, removed when it ends.
#[derive(Default)]
struct Workspace {
    efi: Option<PathBuf>,
    kcl: Option<PathBuf>,
    preserve_efi: bool,
}

impl Workspace {
    fn cleanup(&mut self, stdout: &mut dyn Write) {
        if let Some(efi) = self.efi.take()
            && is_file(&efi)
        {
            if self.preserve_efi {
                let _ = writeln!(
                    stdout,
                    "NOTICE: saving temporary EFI output {}",
                    efi.display()
                );
            } else {
                let _ = std::fs::remove_file(&efi);
            }
        }
        if let Some(kcl) = self.kcl.take()
            && is_file(&kcl)
        {
            let _ = std::fs::remove_file(&kcl);
        }
    }
}

/// Removes a temporary file when it goes out of scope, as the script's `trap
/// ... RETURN` does.
struct TempFile(PathBuf);

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Where the command line comes from and goes to.
enum Target {
    /// A boot environment's property.
    Zfs(String),
    /// An EFI image in the workspace.
    Efi(PathBuf),
}

/// What the options ask for.
#[derive(Default)]
struct Options {
    /// The arguments to append (`-a`).
    appends: Vec<String>,
    /// The arguments to remove (`-r`).
    removes: Vec<String>,
    /// Where the command line goes (`-o`); the input when empty.
    output: String,
    /// Open the command line in the editor (`-e`).
    edit: bool,
    /// Remove the whole command line (`-d`).
    delete: bool,
    /// The boot environment or EFI image, or `-` for standard input.
    input: String,
}

impl Options {
    /// Whether the command line is to be changed rather than printed.
    fn wants_change(&self) -> bool {
        !self.removes.is_empty() || !self.appends.is_empty() || self.edit || self.delete
    }
}

/// Runs `zbm-kcl` with `args` (without the program name) and returns its exit
/// code.
pub fn run(context: &Context, args: &[String], io: &mut Io) -> i32 {
    let mut workspace = Workspace::default();
    let code = run_in(context, args, io, &mut workspace).map_or_else(|code| code, |()| 0);
    workspace.cleanup(io.stdout);
    code
}

fn error(io: &mut Io, message: &str) {
    let _ = writeln!(io.stderr, "ERROR: {message}");
}

/// Prints the messages of `failure`, then `then` when given; status 1.
fn report(io: &mut Io, failure: Failure, then: Option<&str>) -> i32 {
    for message in failure.0.iter().map(String::as_str).chain(then) {
        error(io, message);
    }
    1
}

/// Reads the options; `Err` with the exit status when the run ends here (the
/// usage text, asked for or after a bad option).
fn parse(context: &Context, args: &[String], io: &mut Io) -> Result<Options, i32> {
    let parsed = getopts::parse("ha:r:o:ed", args);
    let mut options = Options::default();
    for option in &parsed.options {
        match option {
            Opt::Value('a', value) => options.appends.push(value.clone()),
            Opt::Value('r', value) => options.removes.push(value.clone()),
            Opt::Value(_, value) => options.output.clone_from(value),
            Opt::Flag('e') => options.edit = true,
            Opt::Flag('d') => options.delete = true,
            Opt::Flag(_) => {
                let _ = io.stdout.write_all(usage(&context.program).as_bytes());
                return Err(0);
            }
        }
    }
    if let Some(problem) = parsed.error {
        let _ = writeln!(io.stderr, "{}", problem.message(&context.program));
        let _ = io.stdout.write_all(usage(&context.program).as_bytes());
        return Err(1);
    }
    options.input = parsed.operands.first().cloned().unwrap_or_default();
    Ok(options)
}

/// Without an input, the root filesystem's dataset; without an output, the
/// input.
fn default_input(context: &Context, io: &mut Io, options: &mut Options) -> Result<(), i32> {
    if options.input.is_empty() {
        let root = MountTable::read(&context.mounts)
            .map_or(RootFilesystem::NotZfs, |table| table.root_filesystem());
        match root {
            RootFilesystem::Zfs(dataset) => options.input = dataset,
            RootFilesystem::NotZfs => {
                error(io, "root does not appear to be ZFS");
                return Err(1);
            }
            // The script's loop ends without output and with status 0, so the
            // input stays empty.
            RootFilesystem::Missing => {}
        }
    }
    if options.output.is_empty() {
        options.output.clone_from(&options.input);
    }
    Ok(())
}

/// Copies the EFI image named by the input, or read from standard input, into
/// the workspace.
fn efi_target(
    context: &Context,
    io: &mut Io,
    workspace: &mut Workspace,
    options: &Options,
) -> Result<Target, i32> {
    let input = &options.input;
    let Ok(temp_efi) = create_temp_file(&context.temp_dir, "tmp.XXXXXXXXXX") else {
        error(
            io,
            "unable to create temporary file for EFI executable edits",
        );
        return Err(1);
    };
    workspace.efi = Some(temp_efi.clone());
    if input == "-" {
        if options.edit {
            error(io, "cannot edit KCL in streaming mode");
            return Err(1);
        }
        let mut image = Vec::new();
        if io.stdin.read_to_end(&mut image).is_err() || std::fs::write(&temp_efi, &image).is_err() {
            error(io, "failed to save EFI executable from stdin");
            return Err(1);
        }
    } else if let Err(failure) = copy_like_cp(Path::new(input), &temp_efi) {
        let _ = writeln!(io.stderr, "{}", failure.cp_message());
        error(
            io,
            &format!("failed to copy EFI executable '{input}' to workspace"),
        );
        return Err(1);
    }
    Ok(Target::Efi(temp_efi))
}

/// Where the command line comes from: an EFI image, or a dataset that exists
/// with an output dataset that exists.
fn target(
    context: &Context,
    io: &mut Io,
    workspace: &mut Workspace,
    options: &Options,
    zfs: &ZfsCommand,
) -> Result<Target, i32> {
    let (input, output) = (&options.input, &options.output);
    if input == "-" || is_file(input) {
        efi_target(context, io, workspace, options)
    } else if !zfs.exists(input) {
        error(io, "KCL source is not a ZFS filesystem or EFI executable");
        Err(1)
    } else if output != input && !zfs.exists(output) {
        error(io, "KCL destination is not a ZFS filesystem");
        Err(1)
    } else {
        Ok(Target::Zfs(input.clone()))
    }
}

/// Reads, tokenizes and modifies the command line into `temp_kcl`. As in the
/// script's pipeline, a failure to read is reported by the reader and the
/// command line is then empty.
fn read_tokens(
    context: &Context,
    io: &mut Io,
    options: &Options,
    target: &Target,
    zfs: &ZfsCommand,
    temp_kcl: &Path,
) -> Result<String, i32> {
    let raw = if options.delete {
        String::new()
    } else {
        match target {
            Target::Zfs(dataset) => raw_kcl_zfs(zfs, dataset),
            Target::Efi(image) => raw_kcl_efi(context, io, image),
        }
    };
    let remove: Vec<&str> = options.removes.iter().map(String::as_str).collect();
    let mut tokens = kcl::suppress(&kcl::tokenize(&raw), &remove);
    for arg in options.appends.iter().filter(|arg| !arg.is_empty()) {
        tokens.push_str(&kcl::tokenize(&format!("{arg}\n")));
    }
    if std::fs::write(temp_kcl, &tokens).is_err() {
        error(io, "failed to parse input KCL");
        return Err(1);
    }
    Ok(tokens)
}

/// Writes the changed EFI image to the output, or standard output for `-`;
/// keeps the image in the workspace when the output cannot be written.
fn write_image(
    io: &mut Io,
    workspace: &mut Workspace,
    image: &Path,
    output: &str,
) -> Result<(), i32> {
    if output == "-" {
        let written = std::fs::read(image).map(|bytes| io.stdout.write_all(&bytes));
        if !matches!(written, Ok(Ok(()))) {
            return Err(1);
        }
    } else if let Err(failure) = copy_like_cp(image, Path::new(output)) {
        let _ = writeln!(io.stderr, "{}", failure.cp_message());
        error(
            io,
            &format!(
                "failed to write EFI executable '{output}'; saved as '{}'",
                image.display()
            ),
        );
        workspace.preserve_efi = true;
        return Err(1);
    }
    Ok(())
}

/// The run: `Ok` for status 0, `Err` with the status otherwise.
fn run_in(
    context: &Context,
    args: &[String],
    io: &mut Io,
    workspace: &mut Workspace,
) -> Result<(), i32> {
    let mut options = parse(context, args, io)?;
    default_input(context, io, &mut options)?;
    let zfs = ZfsCommand::new(context.runner);
    let target = target(context, io, workspace, &options, &zfs)?;
    let Ok(temp_kcl) = create_temp_file(&context.temp_dir, "tmp.XXXXXXXXXX") else {
        error(io, "failed to create temporary KCL file");
        return Err(1);
    };
    workspace.kcl = Some(temp_kcl.clone());
    let tokens = read_tokens(context, io, &options, &target, &zfs, &temp_kcl)?;
    if !options.wants_change() {
        let _ = writeln!(io.stdout, "{}", kcl::assemble(&tokens));
        return Ok(());
    }
    if options.edit {
        edit::in_editor(context, &temp_kcl).map_err(|failure| report(io, failure, None))?;
    }
    let assembled = std::fs::read(&temp_kcl)
        .map(|text| kcl::assemble(&String::from_utf8_lossy(&text)))
        .unwrap_or_default();
    let saved = match &target {
        Target::Zfs(_) => save::to_dataset(&zfs, &assembled, &options.output),
        Target::Efi(image) => save::to_image(context, &assembled, image),
    };
    saved.map_err(|failure| report(io, failure, Some("failed to save KCL changes")))?;
    if let Target::Efi(image) = &target {
        write_image(io, workspace, image, &options.output)?;
    }
    Ok(())
}

/// The property's value, empty when it is not set or cannot be read.
fn raw_kcl_zfs(zfs: &ZfsCommand, dataset: &str) -> String {
    match zfs.get_value(COMMANDLINE.name, dataset) {
        Some(value) if value != "-" => format!("{value}\n"),
        _ => String::new(),
    }
}

/// The contents of the image's `.cmdline` section, empty when it cannot be
/// read.
fn raw_kcl_efi(context: &Context, io: &mut Io, image: &Path) -> String {
    if !is_readable(image) {
        error(io, "the EFI file cannot be read");
        return String::new();
    }
    let Ok(section) = create_temp_file(&context.temp_dir, "tmp.XXXXXXXXXX") else {
        error(io, "failed to create temporary file");
        return String::new();
    };
    let section = TempFile(section);
    match efi_tools::dump_cmdline(context.runner, image, &section.0) {
        Ok(output) if output.status.is_success() => std::fs::read(&section.0)
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default(),
        Ok(output) => {
            let message = output.stdout_text().trim_end_matches('\n').to_owned();
            error(io, &format!("failed to dump EFI cmdline: {message}"));
            String::new()
        }
        Err(_) => {
            error(io, "failed to dump EFI cmdline: ");
            String::new()
        }
    }
}

/// The arguments `while read -r` would read from a tokenized command line; for
/// tests.
pub fn read_arguments(tokens: &str) -> Vec<String> {
    tokens.lines().map(|line| line.trim().to_owned()).collect()
}

#[cfg(test)]
mod tests;
