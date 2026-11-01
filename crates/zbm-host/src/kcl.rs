//! `zbm-kcl`: review or change the kernel command line of a ZFS boot
//! environment or a ZFSBootMenu EFI image.
//!
//! It takes the options of the Bash `zbm-kcl`, with its messages and exit
//! codes. The command line of a boot environment is its
//! `org.zfsbootmenu:commandline` property; that of an EFI image is its
//! `.cmdline` section, which [`crate::pe`] reads and replaces (ADR 0014): a
//! line longer than the room before the next section moves the sections
//! after it, an image without one gets one, and `-d` removes it. The
//! section holds the command line and a NUL byte. A failure to read the
//! command line is reported but does not end the program, as in the script.

#![allow(
    clippy::disallowed_methods,
    reason = "a host tool works on the files and environment of the host it runs on; \
              its tests run it in sandboxes (ADR 0013)"
)]

use std::ffi::OsString;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use crate::options::{Order, Problem, Spec};
use crate::pe::Image;
use zbm_config::spec::properties::COMMANDLINE;
use zbm_platform::exec::{Runner, SystemRunner};
use zbm_platform::filetest::{copy_like_cp, create_temp_file, is_dir, is_file, strerror};
use zbm_types::{DatasetName, KernelCommandLine};
use zbm_zfs::mounts::{MountTable, RootFilesystem};
use zbm_zfs::{Stderr, ZfsText};

mod edit;

mod save;

pub use edit::strip_kcl;

/// What `zbm-kcl` depends on outside its arguments.
pub struct Context<'a> {
    /// Runs `zfs` and the editor.
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
    zbm_platform::filetest::find_program(argv0, path).map_or_else(
        || argv0.to_owned(),
        |found| found.to_string_lossy().into_owned(),
    )
}

/// Whether `command -v name` finds `name`: an executable file at a path, or one
/// found in `path`.
fn command_exists(name: &str, path: Option<&std::ffi::OsStr>) -> bool {
    zbm_platform::filetest::find_program(name, path).is_some()
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
    /// A boot environment's property, and the dataset the changes go to.
    Zfs(DatasetName, DatasetName),
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
fn report(io: &mut Io, failure: &Failure, then: Option<&str>) -> i32 {
    for message in failure.0.iter().map(String::as_str).chain(then) {
        error(io, message);
    }
    1
}

/// Reads the options; `Err` with the exit status when the run ends here (the
/// usage text, asked for or after a bad option).
fn parse(context: &Context, args: &[String], io: &mut Io) -> Result<Options, i32> {
    let parsed = crate::options::parse(&context.program, SPECS, Order::StopAtOperand, args);
    let mut options = Options::default();
    // In order, as getopts reads them: the reading stopped at the first problem
    for given in &parsed.given {
        let value = given.value.clone().unwrap_or_default();
        match SPECS.get(given.index).and_then(|spec| spec.short) {
            Some('a') => options.appends.push(value),
            Some('r') => options.removes.push(value),
            Some('o') => options.output = value,
            Some('e') => options.edit = true,
            Some('d') => options.delete = true,
            _ => {
                let _ = io.stdout.write_all(usage(&context.program).as_bytes());
                return Err(0);
            }
        }
    }
    if let Some(problem) = parsed.problems.first() {
        let (what, name) = match problem {
            Problem::MissingValue(name) => ("option requires an argument", name),
            Problem::Unknown(name) | Problem::Ambiguous(name, _) | Problem::UnwantedValue(name) => {
                ("illegal option", name)
            }
        };
        let name = name.trim_start_matches('-');
        let _ = writeln!(io.stderr, "{}: {what} -- {name}", context.program);
        let _ = io.stdout.write_all(usage(&context.program).as_bytes());
        return Err(1);
    }
    options.input = parsed.operands.first().cloned().unwrap_or_default();
    Ok(options)
}

/// The options of `zbm-kcl` (`zbm-kcl(8)`).
const SPECS: &[Spec] = &[
    Spec::flag(None, Some('h')),
    Spec::value(None, Some('a')),
    Spec::value(None, Some('r')),
    Spec::value(None, Some('o')),
    Spec::flag(None, Some('e')),
    Spec::flag(None, Some('d')),
];

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
    zfs: &ZfsText,
) -> Result<Target, i32> {
    let (input, output) = (&options.input, &options.output);
    if input == "-" || is_file(input) {
        return efi_target(context, io, workspace, options);
    }
    // A name that is not a dataset's is no ZFS filesystem either.
    let existing = |name: &str| {
        name.parse::<DatasetName>()
            .ok()
            .filter(|dataset| zfs.exists(dataset))
    };
    let Some(source) = existing(input) else {
        error(io, "KCL source is not a ZFS filesystem or EFI executable");
        return Err(1);
    };
    let destination = if output == input {
        Some(source.clone())
    } else {
        existing(output)
    };
    let Some(destination) = destination else {
        error(io, "KCL destination is not a ZFS filesystem");
        return Err(1);
    };
    Ok(Target::Zfs(source, destination))
}

/// The command line of `text`, as the kernel splits it; what is not a
/// kernel argument is said on standard error and left out.
fn command_line(io: &mut Io, text: &str) -> KernelCommandLine {
    let (line, refused) = KernelCommandLine::lenient(text);
    for raw in refused {
        error(
            io,
            &format!("left out '{raw}', which is not a kernel argument"),
        );
    }
    line
}

/// Reads the command line, then removes (`-r`: an argument of that name,
/// or that argument itself) and appends (`-a`: its arguments) what the
/// options ask, and writes it into `temp_kcl`, an argument a line.
fn read_command_line(
    io: &mut Io,
    options: &Options,
    target: &Target,
    zfs: &ZfsText,
    temp_kcl: &Path,
) -> Result<KernelCommandLine, i32> {
    let raw = if options.delete {
        String::new()
    } else {
        match target {
            Target::Zfs(dataset, _) => raw_kcl_zfs(zfs, dataset),
            Target::Efi(image) => raw_kcl_efi(io, image),
        }
    };
    let removed = |arg: &zbm_types::KernelArg| {
        options
            .removes
            .iter()
            .any(|removal| arg.name() == removal || arg.to_string() == *removal)
    };
    let mut line: KernelCommandLine = command_line(io, &raw)
        .args()
        .iter()
        .filter(|arg| !removed(arg))
        .cloned()
        .collect();
    for arg in &options.appends {
        line.extend(command_line(io, arg).args().iter().cloned());
    }
    if std::fs::write(temp_kcl, line.one_per_line()).is_err() {
        error(io, "failed to parse input KCL");
        return Err(1);
    }
    Ok(line)
}

/// The output replaced with the changed `image`, whole or not at all
/// ([`crate::files::copy_over`]; fix 24): an image the firmware boots is
/// never left half written. A new output gets the mode `cp` would give it.
fn replace_image(image: &Path, output: &Path) -> Result<(), String> {
    let output = if is_dir(output) {
        output.join(image.file_name().unwrap_or_default())
    } else {
        output.to_owned()
    };
    let mode = std::fs::metadata(image)
        .map_err(|failure| format!("cannot read '{}': {}", image.display(), strerror(&failure)))?
        .permissions()
        .mode();
    crate::files::copy_over(image, &output, crate::files::NewMode::Of(mode)).map_err(|failure| {
        format!(
            "cannot replace '{}': {}",
            output.display(),
            strerror(&failure)
        )
    })
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
    } else if let Err(failure) = replace_image(image, Path::new(output)) {
        error(io, &failure);
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
    let zfs = ZfsText::new(context.runner);
    let target = target(context, io, workspace, &options, &zfs)?;
    let Ok(temp_kcl) = create_temp_file(&context.temp_dir, "tmp.XXXXXXXXXX") else {
        error(io, "failed to create temporary KCL file");
        return Err(1);
    };
    workspace.kcl = Some(temp_kcl.clone());
    let line = read_command_line(io, &options, &target, &zfs, &temp_kcl)?;
    if !options.wants_change() {
        let _ = writeln!(io.stdout, "{line}");
        return Ok(());
    }
    let line = if options.edit {
        edit::in_editor(context, &temp_kcl).map_err(|failure| report(io, &failure, None))?;
        let edited = std::fs::read(&temp_kcl)
            .map(|text| String::from_utf8_lossy(&text).into_owned())
            .unwrap_or_default();
        command_line(io, &edited)
    } else {
        line
    };
    let assembled = line.to_string();
    let saved = match &target {
        Target::Zfs(_, destination) => save::to_dataset(&zfs, &assembled, destination),
        Target::Efi(image) => save::to_image(io, &assembled, image),
    };
    saved.map_err(|failure| report(io, &failure, Some("failed to save KCL changes")))?;
    if let Target::Efi(image) = &target {
        write_image(io, workspace, image, &options.output)?;
    }
    Ok(())
}

/// The property's value, empty when it is not set or cannot be read.
fn raw_kcl_zfs(zfs: &ZfsText, dataset: &DatasetName) -> String {
    match zfs.value(dataset, COMMANDLINE.name, Stderr::Console) {
        Some(value) if value != "-" => format!("{value}\n"),
        _ => String::new(),
    }
}

/// The contents of the image's `.cmdline` section up to its first NUL, as
/// the stub reads it; empty when it has none, or cannot be read.
fn raw_kcl_efi(io: &mut Io, image: &Path) -> String {
    let Ok(bytes) = std::fs::read(image) else {
        error(io, "the EFI file cannot be read");
        return String::new();
    };
    match Image::read(bytes) {
        Ok(read) => read
            .section(".cmdline")
            .map_or_else(String::new, |section| {
                let data = section.data();
                let end = data
                    .iter()
                    .position(|byte| *byte == 0)
                    .unwrap_or(data.len());
                String::from_utf8_lossy(data.get(..end).unwrap_or_default()).into_owned()
            }),
        Err(failure) => {
            error(io, &format!("unable to read the EFI executable: {failure}"));
            String::new()
        }
    }
}

#[cfg(test)]
mod tests;
