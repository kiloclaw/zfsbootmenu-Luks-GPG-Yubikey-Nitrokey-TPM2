//! zreport: the system report, in the sections and colours of the legacy
//! function: the release, the command line, the hooks and the built-in
//! steps, the ZFS modules, and what `zfs` and `zpool` say of the pools and
//! the datasets (ADR 0018, point 17). Names, paths, the command line and
//! what the programs print are shown through [`Printable`], line by line
//! (P13); the legacy function printed them raw. The import stage logs the
//! same report at debug level.

use std::ffi::OsStr;
use std::fmt::Write as _;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use zbm_app::steps::StepState;
use zbm_bootenv::commandline::os_release;
use zbm_bootenv::hooks::HOOKS;
use zbm_config::spec::properties::KEYSOURCE;
use zbm_platform::color::colorize;
use zbm_platform::files::FileKind;
use zbm_platform::{Context, Files, System};
use zbm_state::State;
use zbm_tools::modules::Modules;
use zbm_types::Printable;
use zbm_zfs::ZfsText;

use super::cmdline;

/// The properties the report lists of every dataset.
const COLUMNS: [&str; 7] = [
    "name",
    "mountpoint",
    "canmount",
    "encroot",
    "keystatus",
    "keylocation",
    KEYSOURCE.name,
];

/// The system report of this machine, whose state directory is `state`'s:
/// what zreport prints.
pub fn report(context: &Context<'_>, state: &State<'_>) -> String {
    let mut out = Vec::new();
    // A vector takes every write.
    let _ = written(context, state, &mut out);
    String::from_utf8_lossy(&out).into_owned()
}

/// Writes the system report on `out`, each section before the programs it
/// runs, whose messages then follow its title as the legacy function's
/// did; the exit status of the last, `zfs list`, as the shell reports it.
fn written(context: &Context<'_>, state: &State<'_>, out: &mut dyn Write) -> io::Result<i32> {
    let files = context.files();
    let mut text = colorize("white", "System Report\n\n");
    text.push_str(&release(files));
    section(&mut text, "ZFSBootMenu commandline");
    let line = cmdline::assembled(files, state.base());
    let _ = writeln!(text, "{}", Printable(&line));
    hooks(files, &mut text);
    steps(files, state, &mut text);
    section(&mut text, "ZFS/SPL module information");
    sent(out, &mut text)?;
    let modules = Modules::new(context.runner(), files, context.modules());
    for module in ["spl", "zfs"] {
        let file = modules.file(module).unwrap_or_default();
        let version = modules.version(module).unwrap_or_default();
        let _ = writeln!(text, "{}: {}", Printable(&file), Printable(&version));
        sent(out, &mut text)?;
    }
    let zfs = ZfsText::new(context.runner());
    section(&mut text, "ZFS version");
    sent(out, &mut text)?;
    lines(&mut text, &zfs.version_text());
    section(&mut text, "Imported zpools");
    sent(out, &mut text)?;
    lines(&mut text, &zfs.pool_list_text());
    section(&mut text, "ZFS datasets");
    sent(out, &mut text)?;
    let (status, datasets) = zfs.dataset_columns(&COLUMNS);
    lines(&mut text, &datasets);
    sent(out, &mut text)?;
    Ok(status)
}

/// Writes `text` on `out` and flushes it, so that it comes before what a
/// program prints next on the terminal; `text` is then empty.
fn sent(out: &mut dyn Write, text: &mut String) -> io::Result<()> {
    out.write_all(text.as_bytes())?;
    text.clear();
    out.flush()
}

/// zreport: prints the system report on `out`; the exit status of its
/// last command, `zfs list`, as the legacy function's was, or what
/// [`printed`](super::printed) makes of a report that could not be
/// printed.
pub(super) fn zreport(
    system: &System<'_>,
    state: &State<'_>,
    _args: &[String],
    out: &mut dyn Write,
) -> i32 {
    match written(&Context::new(*system), state, out) {
        Ok(status) => status,
        Err(error) => super::printed(Err(error)),
    }
}

/// Adds the title of a section to `text`.
fn section(text: &mut String, title: &str) {
    text.push_str(&colorize("orange", &format!("\n>> {title}\n")));
}

/// Adds each line of `output` to `text`, escaped, with a newline.
fn lines(text: &mut String, output: &str) {
    for line in output.split_terminator('\n') {
        let _ = writeln!(text, "{}", Printable(line));
    }
}

/// Whether `path` is a regular file, as the shell's `-f` finds it.
fn is_file(files: &dyn Files, path: &Path) -> bool {
    files
        .metadata(path)
        .is_ok_and(|metadata| metadata.kind == FileKind::File)
}

/// The value the release file `path` gives `name` when sourced, empty
/// without one.
fn sourced(files: &dyn Files, path: &str, name: &str) -> String {
    let text = files.read_to_string(Path::new(path)).unwrap_or_default();
    os_release(&text)
        .into_iter()
        .rev()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value)
        .unwrap_or_default()
}

/// The first line of `path`, without its surrounding blanks, as `read -r`
/// reads it; empty when it cannot be read.
fn first_line(files: &dyn Files, path: &str) -> String {
    let text = files.read_to_string(Path::new(path)).unwrap_or_default();
    let line = text.lines().next().unwrap_or_default();
    line.trim_matches([' ', '\t']).to_owned()
}

/// The release line: ZFSBootMenu's name and version, the initramfs
/// builder's, and the kernel's name, release and architecture, as `uname
/// -srm` prints them.
fn release(files: &dyn Files) -> String {
    let (version, pretty) = if is_file(files, Path::new("/etc/zbm-release")) {
        (
            sourced(files, "/etc/zbm-release", "VERSION"),
            sourced(files, "/etc/zbm-release", "PRETTY_NAME"),
        )
    } else {
        (String::new(), String::new())
    };
    let color = if version.ends_with("dev") {
        "red"
    } else {
        "green"
    };
    let version = colorize(color, &Printable(&version).to_string());
    let name = if pretty == "ZFSBootMenu" {
        colorize("orange", "ZFS") + &colorize("lightgray", "BootMenu")
    } else {
        "ZFSBootMenu".to_owned()
    };
    format!(
        "{name} {version} with {} and {}\n",
        Printable(&initramfs(files)),
        Printable(&uname(files))
    )
}

/// The builder of the initramfs and its version: mkinitcpio's `/VERSION`,
/// else dracut's release file, the legacy one first.
fn initramfs(files: &dyn Files) -> String {
    if is_file(files, Path::new("/VERSION")) {
        return format!("mkinitcpio {}", first_line(files, "/VERSION"));
    }
    let dracut = if is_file(files, Path::new("/etc/initrd-release")) {
        sourced(files, "/etc/initrd-release", "DRACUT_VERSION")
    } else if is_file(files, Path::new("/usr/lib/initrd-release")) {
        sourced(files, "/usr/lib/initrd-release", "VERSION_ID")
    } else {
        String::new()
    };
    if dracut.is_empty() {
        "unknown initramfs".to_owned()
    } else {
        format!("Dracut {dracut}")
    }
}

/// The kernel's name, release and architecture, as `uname -srm` prints
/// them, from `/proc/sys/kernel`; the architecture this program was built
/// for where the kernel does not say.
fn uname(files: &dyn Files) -> String {
    let mut arch = first_line(files, "/proc/sys/kernel/arch");
    if arch.is_empty() {
        std::env::consts::ARCH.clone_into(&mut arch);
    }
    format!(
        "{} {} {arch}",
        first_line(files, "/proc/sys/kernel/ostype"),
        first_line(files, "/proc/sys/kernel/osrelease"),
    )
}

/// The hooks of every stage, as the shell's `/libexec/hooks/*.d/*` gives
/// them: by path, names that start with a dot left out.
fn hook_paths(files: &dyn Files) -> Vec<PathBuf> {
    let visible = |name: &OsStr| !name.as_encoded_bytes().starts_with(b".");
    let root = Path::new(HOOKS);
    let mut paths: Vec<PathBuf> = files
        .read_dir(root)
        .unwrap_or_default()
        .into_iter()
        .filter(|name| visible(name) && name.as_encoded_bytes().ends_with(b".d"))
        .flat_map(|stage| {
            let dir = root.join(stage);
            let names = files.read_dir(&dir).unwrap_or_default();
            names
                .into_iter()
                .filter(|name| visible(name))
                .map(move |name| dir.join(name))
        })
        .collect();
    paths.sort();
    paths
}

/// Adds the enabled hooks, those a stage runs or ran once, then the
/// disabled ones, files without an execute bit.
fn hooks(files: &dyn Files, text: &mut String) {
    const COMPLETED: &str = ".completed";
    let paths = hook_paths(files);
    let runs = |path: &Path| {
        files
            .metadata(path)
            .is_ok_and(|metadata| metadata.kind == FileKind::Directory || metadata.is_executable())
    };
    section(text, "Enabled hooks");
    for path in &paths {
        let shown = path.to_string_lossy();
        if shown.ends_with(COMPLETED) {
            let shown = shown.replace(COMPLETED, " (run once, completed)");
            listed(text, "green", &shown);
        } else if runs(path) {
            listed(text, "green", &shown);
        }
    }
    section(text, "Disabled hooks");
    for path in &paths {
        let shown = path.to_string_lossy();
        if is_file(files, path) && !runs(path) && !shown.ends_with(COMPLETED) {
            listed(text, "red", &shown);
        }
    }
}

/// Adds an entry of a list: `item`, escaped, in `color`.
fn listed(text: &mut String, color: &str, item: &str) {
    let _ = writeln!(text, "* {}", colorize(color, &Printable(item).to_string()));
}

/// Adds the built-in steps (ADR 0017), by stage and name, each with its
/// state: skipped by `zbm.skip_hooks`, replaced by a hook of its name,
/// run once at start-up, or to run.
fn steps(files: &dyn Files, state: &State<'_>, text: &mut String) {
    section(text, "Built-in steps");
    for (stage, name, how) in StepState::of_each(files, Path::new(HOOKS), state) {
        let (color, how) = match how {
            StepState::Skipped => ("red", " (skipped by zbm.skip_hooks)"),
            StepState::Replaced => ("red", " (replaced by a hook)"),
            StepState::Ran => ("green", " (run once, completed)"),
            StepState::Pending => ("green", ""),
        };
        listed(text, color, &format!("{stage}/{name}{how}"));
    }
}

#[cfg(test)]
mod tests;
