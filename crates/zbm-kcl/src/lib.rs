//! `zbm-kcl`: review or change the kernel command line of a ZFS boot
//! environment or a ZFSBootMenu EFI image.
//!
//! This is the Rust version of `legacy/bin/zbm-kcl`, with the same options,
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

use zbm_cmdline::kcl;
use zbm_cmdline::parent::COMMANDLINE_PROPERTY;
use zbm_core::fs::{copy_like_cp, create_temp_file, is_file, is_readable};
use zbm_core::getopts::{self, Opt};
use zbm_core::shell::{command_substitution, echo, read_lines};
use zbm_platform::exec::{Output, Runner, SystemRunner};
use zbm_tools::efi as efi_tools;
use zbm_tools::programs;
use zbm_zfs::ZfsCommand;
use zbm_zfs::mounts::{MountTable, RootFilesystem};

pub mod efi;

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
        .find(|candidate| is_file(candidate) && zbm_core::fs::is_executable(candidate))
}

/// Whether `command -v name` finds `name`: an executable file at a path, or one
/// found in `path`.
fn command_exists(name: &str, path: Option<&std::ffi::OsStr>) -> bool {
    if name.contains('/') {
        is_file(name) && zbm_core::fs::is_executable(name)
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

/// The text added below the command line when it is opened for editing.
const EDIT_HELP: &str = "\n\n# KCL processing ends with the first line that contains no text.
# Anything starting with # is considered a comment and will be ignored.
# Multiple lines will be concatenated with spaces to form a single line.
";

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

/// Runs `zbm-kcl` with `args` (without the program name) and returns its exit
/// code.
pub fn run(context: &Context, args: &[String], io: &mut Io) -> i32 {
    let mut workspace = Workspace::default();
    let code = run_in(context, args, io, &mut workspace);
    workspace.cleanup(io.stdout);
    code
}

fn error(io: &mut Io, message: &str) {
    let _ = writeln!(io.stderr, "ERROR: {message}");
}

fn run_in(context: &Context, args: &[String], io: &mut Io, workspace: &mut Workspace) -> i32 {
    let parsed = getopts::parse("ha:r:o:ed", args);
    let (mut appends, mut removes) = (Vec::new(), Vec::new());
    let (mut output, mut edit, mut delete) = (String::new(), false, false);
    for option in &parsed.options {
        match option {
            Opt::Value('a', value) => appends.push(value.clone()),
            Opt::Value('r', value) => removes.push(value.clone()),
            Opt::Value(_, value) => output.clone_from(value),
            Opt::Flag('e') => edit = true,
            Opt::Flag('d') => delete = true,
            Opt::Flag(_) => {
                let _ = io.stdout.write_all(usage(&context.program).as_bytes());
                return 0;
            }
        }
    }
    if let Some(problem) = parsed.error {
        let _ = writeln!(io.stderr, "{}", problem.message(&context.program));
        let _ = io.stdout.write_all(usage(&context.program).as_bytes());
        return 1;
    }

    let mut input = parsed.operands.first().cloned().unwrap_or_default();
    if input.is_empty() {
        let root = MountTable::read(&context.mounts)
            .map_or(RootFilesystem::NotZfs, |table| table.root_filesystem());
        match root {
            RootFilesystem::Zfs(dataset) => input = dataset,
            RootFilesystem::NotZfs => {
                error(io, "root does not appear to be ZFS");
                return 1;
            }
            // The script's loop ends without output and with status 0, so the input stays
            // empty.
            RootFilesystem::Missing => {}
        }
    }
    if output.is_empty() {
        output.clone_from(&input);
    }
    let modify = !removes.is_empty() || !appends.is_empty() || edit || delete;

    let zfs = ZfsCommand::new(context.runner);
    let efi_mode = input == "-" || is_file(&input);
    let target = if efi_mode {
        let Ok(temp_efi) = create_temp_file(&context.temp_dir, "tmp.XXXXXXXXXX") else {
            error(
                io,
                "unable to create temporary file for EFI executable edits",
            );
            return 1;
        };
        workspace.efi = Some(temp_efi.clone());
        if input == "-" {
            if edit {
                error(io, "cannot edit KCL in streaming mode");
                return 1;
            }
            let mut image = Vec::new();
            if io.stdin.read_to_end(&mut image).is_err()
                || std::fs::write(&temp_efi, &image).is_err()
            {
                error(io, "failed to save EFI executable from stdin");
                return 1;
            }
        } else if let Err(failure) = copy_like_cp(Path::new(&input), &temp_efi) {
            let _ = writeln!(io.stderr, "{}", failure.cp_message());
            error(
                io,
                &format!("failed to copy EFI executable '{input}' to workspace"),
            );
            return 1;
        }
        Target::Efi(temp_efi)
    } else if !zfs.exists(&input) {
        error(io, "KCL source is not a ZFS filesystem or EFI executable");
        return 1;
    } else if output != input && !zfs.exists(&output) {
        error(io, "KCL destination is not a ZFS filesystem");
        return 1;
    } else {
        Target::Zfs(input.clone())
    };

    let Ok(temp_kcl) = create_temp_file(&context.temp_dir, "tmp.XXXXXXXXXX") else {
        error(io, "failed to create temporary KCL file");
        return 1;
    };
    workspace.kcl = Some(temp_kcl.clone());

    // Read, tokenize and modify the input KCL. As in the script's pipeline, a
    // failure to read is reported by the reader and the command line is then empty.
    let raw = if delete {
        String::new()
    } else {
        match &target {
            Target::Zfs(dataset) => raw_kcl_zfs(&zfs, dataset),
            Target::Efi(image) => raw_kcl_efi(context, io, image),
        }
    };
    let remove: Vec<&str> = removes.iter().map(String::as_str).collect();
    let mut tokens = kcl::suppress(&kcl::tokenize(&raw), &remove);
    for arg in appends.iter().filter(|arg| !arg.is_empty()) {
        tokens.push_str(&kcl::tokenize(&format!("{arg}\n")));
    }
    if std::fs::write(&temp_kcl, &tokens).is_err() {
        error(io, "failed to parse input KCL");
        return 1;
    }

    if !modify {
        let _ = writeln!(io.stdout, "{}", kcl::assemble(&tokens));
        return 0;
    }

    if edit && !edit_kcl(context, io, &temp_kcl) {
        return 1;
    }

    let assembled = std::fs::read(&temp_kcl)
        .map(|text| kcl::assemble(&String::from_utf8_lossy(&text)))
        .unwrap_or_default();
    let saved = match &target {
        Target::Zfs(_) => save_kcl_zfs(&zfs, io, &assembled, &output),
        Target::Efi(image) => save_kcl_efi(context, io, &assembled, image),
    };
    if !saved {
        error(io, "failed to save KCL changes");
        return 1;
    }

    if let Target::Efi(image) = &target {
        if output == "-" {
            let written = std::fs::read(image).map(|bytes| io.stdout.write_all(&bytes));
            if !matches!(written, Ok(Ok(()))) {
                return 1;
            }
        } else if let Err(failure) = copy_like_cp(image, Path::new(&output)) {
            let _ = writeln!(io.stderr, "{}", failure.cp_message());
            error(
                io,
                &format!(
                    "failed to write EFI executable '{output}'; saved as '{}'",
                    image.display()
                ),
            );
            workspace.preserve_efi = true;
            return 1;
        }
    }
    0
}

/// `raw_kcl_zfs`: the property's value, empty when it is not set or cannot be
/// read.
fn raw_kcl_zfs(zfs: &ZfsCommand, dataset: &str) -> String {
    match zfs.get_value(COMMANDLINE_PROPERTY, dataset) {
        Some(value) if value != "-" => echo(&value),
        _ => String::new(),
    }
}

/// `raw_kcl_efi`: the contents of the `.cmdline` section, empty when it cannot
/// be read.
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
            let message = command_substitution(&output.stdout_text()).to_owned();
            error(io, &format!("failed to dump EFI cmdline: {message}"));
            String::new()
        }
        Err(_) => {
            error(io, "failed to dump EFI cmdline: ");
            String::new()
        }
    }
}

/// `save_kcl_zfs`: sets the property, or removes it when the command line is
/// empty.
fn save_kcl_zfs(zfs: &ZfsCommand, io: &mut Io, assembled: &str, dataset: &str) -> bool {
    // `kcl="$(cat)"` drops trailing newlines and NUL bytes.
    let kcl = command_substitution(assembled).replace('\0', "");
    if kcl.is_empty() {
        if !zfs.inherit(COMMANDLINE_PROPERTY, dataset).unwrap_or(false) {
            error(io, &format!("failed to clear KCL property on {dataset}"));
            return false;
        }
    } else if !zfs
        .set(COMMANDLINE_PROPERTY, &kcl, dataset)
        .unwrap_or(false)
    {
        error(io, &format!("failed to set KCL property on {dataset}"));
        return false;
    }
    true
}

/// `save_kcl_efi`: replaces the `.cmdline` section of the image with the
/// command line and a NUL byte, at the same address, if it fits in the space
/// before the next section.
fn save_kcl_efi(context: &Context, io: &mut Io, assembled: &str, image: &Path) -> bool {
    let image_name = image.display().to_string();
    let gap = if !is_readable(image) {
        Err(vec![format!("unable to read object file '{image_name}'")])
    } else {
        match efi_tools::section_headers(context.runner, image) {
            Ok(output) if output.status.is_success() => {
                efi::cmdline_gap(&output.stdout_text(), &image_name)
            }
            _ => Err(vec![
                "failed to parse object file; is objdump available?".to_owned(),
            ]),
        }
    };
    let (offset, gap) = match gap {
        Ok(found) => found,
        Err(messages) => {
            for message in messages {
                error(io, &message);
            }
            error(io, "failed to determine offset data for KCL");
            return false;
        }
    };

    let Ok(section) = create_temp_file(&context.temp_dir, "tmp.XXXXXXXXXX") else {
        error(io, "failed to save temporary KCL");
        return false;
    };
    let section = TempFile(section);
    let mut contents = assembled.as_bytes().to_vec();
    contents.push(0);
    if std::fs::write(&section.0, &contents).is_err() {
        error(io, "failed to save temporary KCL");
        return false;
    }
    if !efi::fits(contents.len(), gap) {
        error(
            io,
            &format!("new KCL size exceeds space available in EFI file '{image_name}'; aborting"),
        );
        return false;
    }

    let succeeded =
        |run: std::io::Result<Output>| run.is_ok_and(|output| output.status.is_success());
    if !succeeded(efi_tools::remove_cmdline(context.runner, image)) {
        error(io, "failed to clear existing KCL from EFI executable");
        return false;
    }
    if !succeeded(efi_tools::add_cmdline(
        context.runner,
        image,
        &section.0,
        &offset,
    )) {
        error(io, "failed to write new KCL to EFI executable");
        return false;
    }
    true
}

/// `edit`: opens the command line in `$EDITOR` (default `vi`) and saves what
/// the editor leaves, tokenized again.
fn edit_kcl(context: &Context, io: &mut Io, temp_kcl: &Path) -> bool {
    let editor = context
        .editor
        .clone()
        .filter(|editor| !editor.is_empty())
        .unwrap_or_else(|| "vi".to_owned());
    if !command_exists(&editor, context.path.as_deref()) {
        error(io, "define $EDITOR to edit");
        return false;
    }
    let Some(tokens) = is_readable(temp_kcl)
        .then(|| std::fs::read(temp_kcl).ok())
        .flatten()
    else {
        error(io, "unable to read KCL");
        return false;
    };
    let Ok(file) = create_temp_file(&context.temp_dir, "tmp.XXXXXXXXXX") else {
        error(io, "failed to create temporary file");
        return false;
    };
    let file = TempFile(file);
    let mut text = kcl::assemble(&String::from_utf8_lossy(&tokens));
    text.push_str(EDIT_HELP);
    if std::fs::write(&file.0, text).is_err() {
        error(io, "failed to create temporary file");
        return false;
    }

    let edited = programs::edit(context.runner, &editor, &file.0)
        .is_ok_and(|output| output.status.is_success());
    if !edited {
        error(io, "failed to edit KCL");
        return false;
    }

    let edited = std::fs::read(&file.0)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();
    if std::fs::write(temp_kcl, kcl::tokenize(&strip_kcl(&edited))).is_err() {
        error(io, "failed to save edited KCL");
        return false;
    }
    true
}

/// `strip_kcl`: joins the lines before the first empty line with spaces,
/// without comments (from the first `#` and the blanks before it) and without
/// lines left empty.
pub fn strip_kcl(text: &str) -> String {
    let mut lines = Vec::new();
    for line in kcl::lines(text) {
        if line.is_empty() {
            break;
        }
        let line = match line.find('#') {
            Some(hash) => line[..hash].trim_end_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r']),
            None => line,
        };
        if !line.is_empty() {
            lines.push(line);
        }
    }
    lines.join(" ")
}

/// The arguments `while read -r` would read from a tokenized command line; for
/// tests.
pub fn read_arguments(tokens: &str) -> Vec<String> {
    read_lines(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbm_testkit::{FakeRunner, TempDir};

    struct Run {
        code: i32,
        stdout: String,
        stderr: String,
    }

    fn run_with(runner: &FakeRunner, mounts: &Path, args: &[&str], stdin: &[u8]) -> Run {
        let temp = TempDir::new("zbm-kcl-unit");
        let context = Context {
            runner,
            program: "zbm-kcl".to_owned(),
            mounts: mounts.to_owned(),
            temp_dir: temp.path().to_owned(),
            editor: None,
            path: None,
        };
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let code = run(
            &context,
            &args,
            &mut Io {
                stdin: &mut &stdin[..],
                stdout: &mut stdout,
                stderr: &mut stderr,
            },
        );
        let leftovers = std::fs::read_dir(temp.path()).unwrap().count();
        assert_eq!(leftovers, 0, "temporary files are removed");
        Run {
            code,
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
        }
    }

    #[test]
    fn prints_the_property_of_the_root_filesystem() {
        let dir = TempDir::new("zbm-kcl-mounts");
        let mounts = dir.write("mounts", "rpool/ROOT/void / zfs rw 0 0\n");
        let runner = FakeRunner::new();
        runner.respond(
            "zfs get -H -o value org.zfsbootmenu:commandline rpool/ROOT/void",
            0,
            "quiet  \"a  b\" loglevel=4\n",
        );
        let result = run_with(&runner, &mounts, &[], b"");
        assert_eq!(result.code, 0);
        assert_eq!(result.stdout, "quiet \"a b\" loglevel=4\n");
        assert_eq!(
            runner.lines(),
            [
                "zfs list -o name -H rpool/ROOT/void",
                "zfs get -H -o value org.zfsbootmenu:commandline rpool/ROOT/void",
            ]
        );
    }

    #[test]
    fn appends_and_removes_on_a_dataset() {
        let dir = TempDir::new("zbm-kcl-mounts");
        let mounts = dir.write("mounts", "tmpfs / tmpfs rw 0 0\n");
        let runner = FakeRunner::new();
        runner.respond(
            "zfs get -H -o value org.zfsbootmenu:commandline pool/be",
            0,
            "quiet loglevel=4 rw\n",
        );
        let result = run_with(
            &runner,
            &mounts,
            &["-r", "loglevel", "-a", "loglevel=7 nomodeset", "pool/be"],
            b"",
        );
        assert_eq!(result.code, 0, "{}", result.stderr);
        assert_eq!(
            runner.lines().last().unwrap(),
            "zfs set org.zfsbootmenu:commandline=quiet rw loglevel=7 nomodeset pool/be"
        );

        let runner = FakeRunner::new();
        let result = run_with(&runner, &mounts, &["-d", "pool/be"], b"");
        assert_eq!(result.code, 0);
        assert_eq!(
            runner.lines().last().unwrap(),
            "zfs inherit org.zfsbootmenu:commandline pool/be"
        );
    }

    #[test]
    fn reports_bad_options_and_sources() {
        let dir = TempDir::new("zbm-kcl-mounts");
        let mounts = dir.write("mounts", "tmpfs / tmpfs rw 0 0\n");
        let runner = FakeRunner::new();
        let result = run_with(&runner, &mounts, &["-x"], b"");
        assert_eq!(result.code, 1);
        assert_eq!(result.stderr, "zbm-kcl: illegal option -- x\n");
        assert!(result.stdout.starts_with("USAGE: zbm-kcl [-a <arg>]"));

        assert_eq!(run_with(&runner, &mounts, &["-h", "-x"], b"").code, 0);

        let result = run_with(&runner, &mounts, &[], b"");
        assert_eq!(result.code, 1);
        assert_eq!(result.stderr, "ERROR: root does not appear to be ZFS\n");

        runner.respond("zfs list -o name -H nope", 1, "");
        let result = run_with(&runner, &mounts, &["nope"], b"");
        assert_eq!(result.code, 1);
        assert_eq!(
            result.stderr,
            "ERROR: KCL source is not a ZFS filesystem or EFI executable\n"
        );

        let result = run_with(&runner, &mounts, &["-e", "-"], b"MZ");
        assert_eq!(result.code, 1);
        assert_eq!(result.stderr, "ERROR: cannot edit KCL in streaming mode\n");
    }

    #[test]
    fn editors_are_found_as_command_v_finds_them() {
        let dir = TempDir::new("zbm-kcl-editor");
        let editor = dir.write("ed", "");
        std::fs::set_permissions(&editor, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        assert!(command_exists(&editor.display().to_string(), None));
        assert!(!command_exists(
            "ed",
            Some(std::ffi::OsStr::new(":/nonexistent"))
        ));
        assert!(!command_exists("ed", None));
    }

    #[test]
    fn strips_comments_and_stops_at_an_empty_line() {
        assert_eq!(
            strip_kcl("quiet # comment\n# whole line\nroot=x\t#x\n   \nlast\n\nignored\n"),
            "quiet root=x     last"
        );
        assert_eq!(strip_kcl("a#b"), "a");
        assert_eq!(strip_kcl(""), "");
        assert_eq!(read_arguments("a\nb"), ["a"]);
    }

    #[test]
    fn script_name_follows_bash() {
        let dir = TempDir::new("zbm-kcl-path");
        let program = dir.write("zbm-kcl", "");
        std::fs::set_permissions(
            &program,
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();
        let path = OsString::from(format!("/nonexistent:{}", dir.path().display()));
        assert_eq!(
            script_name("zbm-kcl", Some(&path)),
            program.display().to_string()
        );
        assert_eq!(script_name("./zbm-kcl", Some(&path)), "./zbm-kcl");
        assert_eq!(script_name("other", Some(&path)), "other");
    }
}
