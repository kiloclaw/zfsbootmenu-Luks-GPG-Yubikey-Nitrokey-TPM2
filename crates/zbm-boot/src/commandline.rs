//! The kernel command line of a boot environment, and the `kexec` commands.
//!
//! The command line is assembled from the user's override, a cache or the
//! boot environment's own settings, with the host ID when it applies. This
//! module also finds the prefix of the `root=` argument and the device tree,
//! and builds the `kexec` commands that load and run the kernel.

use std::path::{Component, Path, PathBuf};

use zbm_config::kcl;
use zbm_config::parent::{CommandLineSource, read_kcl_contents, read_kcl_prop};
use zbm_platform::exec::{Output, Runner};
use zbm_platform::files::Files;
use zbm_state::State;
use zbm_tools::kexec;
use zbm_trace::Message;

pub use zbm_pool::hostid::{ETC_HOSTID, SPL_HOSTID, apparent_hostid as spl_hostid};

/// The command line of a boot environment that sets none.
const DEFAULT_ARGS: &str = "quiet loglevel=4";

/// What `$( ... )` gives for `text`, followed by the newline of a here-string
/// (`<<< "${var}"`), so text moves between the `kcl` functions as it does
/// between the legacy ones.
fn here(text: &str) -> String {
    format!("{}\n", text.trim_end_matches('\n'))
}

/// Whether the command line cache `path` can be used: it exists and was
/// written after the user declined to resume from hibernation, if they did.
fn cache_is_valid(files: &dyn Files, state: &State, path: &Path) -> bool {
    let Ok(metadata) = files.metadata(path) else {
        return false;
    };
    match state.noresume() {
        None => true,
        Some(noresume) => metadata.modified > noresume,
    }
}

/// The command line of `dataset` booted with `kernel` (may be empty): the
/// user's override, a valid cache, or the boot environment's own command line
/// (a kernel's `.kcl` file, or `org.zfsbootmenu:commandline` with `%{parent}`
/// expanded) without `root=`, with `noresume` and the host ID when they apply.
/// A computed command line is written to the cache.
pub fn be_cmdline(
    files: &dyn Files,
    source: &dyn CommandLineSource,
    state: &State,
    dataset: &str,
    kernel: &str,
    set_hostid: bool,
    log: &mut Vec<Message>,
) -> String {
    if dataset.is_empty() {
        log.push(Message::error("filesystem is undefined"));
        return String::new();
    }
    let (kernel_kcl, cache) = kcl_files(files, state, dataset, kernel, log);
    if let Some(stored) = stored_cmdline(files, state, dataset, cache.as_deref(), log) {
        return stored;
    }
    let args = own_args(files, source, dataset, kernel_kcl.as_deref(), log);
    let args = edited_args(files, state, dataset, set_hostid, &args, log);

    if let Some(cache) = &cache {
        log.push(Message::debug(format!(
            "caching KCL for {dataset} at {}",
            cache.display()
        )));
        // printf "%s\n" "${args}" > "${cache}"; a failure is not reported
        let _ = files.write(cache, format!("{args}\n").as_bytes());
    }
    let assembled = kcl::assemble(&here(&args));
    log.push(Message::debug(format!(
        "assembled commandline: '{assembled}'"
    )));
    assembled
}

/// A command line typed for one boot in place of the property of its boot
/// environment `dataset`, as if set there: `%{parent}` in it stands for
/// the command line of the parent dataset.
struct Typed<'a> {
    source: &'a dyn CommandLineSource,
    dataset: &'a str,
    line: &'a str,
}

impl CommandLineSource for Typed<'_> {
    fn commandline(&self, filesystem: &str) -> Option<String> {
        if filesystem == self.dataset {
            return Some(self.line.to_owned());
        }
        self.source.commandline(filesystem)
    }

    fn inherited_source(&self, filesystem: &str) -> Option<String> {
        if filesystem == self.dataset {
            return Some(String::new());
        }
        self.source.inherited_source(filesystem)
    }
}

/// The command line of `dataset` booted with `line`, which the user typed
/// for this boot in place of its `org.zfsbootmenu:commandline`: `%{parent}`
/// expanded, without `root=`, with `noresume` and the host ID when they
/// apply, as its own would be. Nothing is written down, so the next boot
/// uses the boot environment's own command line again; neither a kernel's
/// `.kcl` file nor the cache takes the typed line's place.
pub fn edited_cmdline(
    files: &dyn Files,
    source: &dyn CommandLineSource,
    state: &State,
    (dataset, line): (&str, &str),
    set_hostid: bool,
    log: &mut Vec<Message>,
) -> String {
    log.push(Message::debug(format!(
        "using the command line typed for this boot of {dataset}"
    )));
    let typed = Typed {
        source,
        dataset,
        line,
    };
    let value = read_kcl_prop(&typed, dataset, log).unwrap_or_default();
    let args = kcl::tokenize(&here(&value))
        .trim_end_matches('\n')
        .to_owned();
    let args = if args.is_empty() {
        DEFAULT_ARGS.to_owned()
    } else {
        args
    };
    let args = edited_args(files, state, dataset, set_hostid, &args, log);
    let assembled = kcl::assemble(&here(&args));
    log.push(Message::debug(format!(
        "assembled commandline: '{assembled}'"
    )));
    assembled
}

/// The command line files of `dataset` booted with `kernel`: the kernel's own
/// file in the command line cache if it exists, and the cache of the default
/// command line, which is `None` when the kernel has its own file.
fn kcl_files(
    files: &dyn Files,
    state: &State,
    dataset: &str,
    kernel: &str,
    log: &mut Vec<Message>,
) -> (Option<PathBuf>, Option<PathBuf>) {
    // Each kernel_kcl_cache call logs this in be_location.
    let location = || Message::debug(format!("fs set to {dataset}"));
    log.push(location());
    if !kernel.is_empty() {
        log.push(location());
    }
    let kernel_kcl = state
        .kcl_cache(dataset, kernel)
        .filter(|path| files.exists(path));
    let cache = match &kernel_kcl {
        Some(path) => {
            log.push(Message::debug(format!(
                "found kernel KCL {} on {dataset}, ignoring KCL cache",
                path.display()
            )));
            None
        }
        None => {
            log.push(location());
            state.kcl_cache(dataset, "default")
        }
    };
    (kernel_kcl, cache)
}

/// The command line of `dataset` that is already written down, assembled: the
/// user's override, or else the default command line `cache` if it is valid.
fn stored_cmdline(
    files: &dyn Files,
    state: &State,
    dataset: &str,
    cache: Option<&Path>,
    log: &mut Vec<Message>,
) -> Option<String> {
    if let Some(text) = state.cmdline_override() {
        log.push(Message::debug(format!(
            "using {}/cmdline as command line for {dataset}",
            state.base().display()
        )));
        return Some(kcl::assemble(&text));
    }
    let cache = cache.filter(|cache| cache_is_valid(files, state, cache))?;
    log.push(Message::debug(format!(
        "using cached KCL from {} as command line for {dataset}",
        cache.display()
    )));
    let text = files.read_to_string(cache).unwrap_or_default();
    Some(kcl::assemble(&text))
}

/// The boot environment's own command line, one argument per line: from the
/// kernel's command line file `kernel_kcl`, or from
/// `org.zfsbootmenu:commandline` with `%{parent}` expanded, or the default
/// arguments when that gives none.
fn own_args(
    files: &dyn Files,
    source: &dyn CommandLineSource,
    dataset: &str,
    kernel_kcl: Option<&Path>,
    log: &mut Vec<Message>,
) -> String {
    // read_kcl_value with the kernel's .kcl file, which the legacy function
    // passes even when it is empty, so the property is read
    let kcl_path = kernel_kcl.map(Path::to_path_buf).unwrap_or_default();
    let contents = kernel_kcl.and_then(|path| files.read(path).ok());
    let value =
        read_kcl_contents(source, dataset, &kcl_path, contents.as_deref(), log).unwrap_or_default();
    let args = kcl::tokenize(&here(&value))
        .trim_end_matches('\n')
        .to_owned();
    if args.is_empty() {
        DEFAULT_ARGS.to_owned()
    } else {
        args
    }
}

/// `args` without `root=`, and with `noresume` and the host ID in place of the
/// arguments they replace, when they apply.
fn edited_args(
    files: &dyn Files,
    state: &State,
    dataset: &str,
    set_hostid: bool,
    args: &str,
    log: &mut Vec<Message>,
) -> String {
    let mut removals = vec!["root".to_owned()];
    let mut additions = Vec::new();

    if state.noresume().is_some() {
        log.push(Message::debug(format!(
            "{}/noresume set, expunging from {dataset}",
            state.base().display()
        )));
        removals.push("resume".to_owned());
        additions.push("noresume".to_owned());
    }
    if set_hostid && let Some(hostid) = spl_hostid(files, state, log) {
        log.push(Message::debug(format!(
            "overriding spl_hostid and spl.spl_hostid for {dataset}"
        )));
        removals.push("spl_hostid".to_owned());
        removals.push("spl.spl_hostid".to_owned());
        additions.push(if hostid == "0x00000000" {
            "spl_hostid=00000000".to_owned()
        } else {
            format!("spl.spl_hostid={hostid}")
        });
    }

    let removals: Vec<&str> = removals.iter().map(String::as_str).collect();
    let additions: Vec<&str> = additions.iter().map(String::as_str).collect();
    let suppressed = kcl::suppress(&here(args), &removals);
    kcl::append(&suppressed, &additions)
        .trim_end_matches('\n')
        .to_owned()
}

/// Reads the assignments of an `os-release` file, as sourcing it sets them:
/// `NAME=value` lines, with double-quoted, single-quoted or unquoted values.
/// Lines that are not plain assignments are ignored.
pub fn os_release(text: &str) -> Vec<(String, String)> {
    let mut vars = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        let Some((name, raw)) = line.split_once('=') else {
            continue;
        };
        let name_ok = name
            .chars()
            .next()
            .is_some_and(|first| first == '_' || first.is_ascii_alphabetic())
            && name.chars().all(|c| c == '_' || c.is_ascii_alphanumeric());
        if !name_ok {
            continue;
        }
        if let Some(value) = unquote(raw) {
            vars.push((name.to_owned(), value));
        }
    }
    vars
}

/// The value of one shell word: `'...'`, `"..."` with backslash escapes of
/// `"`, `\`, `$` and `` ` ``, or unquoted text with backslash escapes, in any
/// sequence. `None` if a quote is not closed or a blank follows the word.
fn unquote(raw: &str) -> Option<String> {
    let mut value = String::new();
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' => loop {
                match chars.next()? {
                    '\'' => break,
                    c => value.push(c),
                }
            },
            '"' => loop {
                match chars.next()? {
                    '"' => break,
                    '\\' => {
                        let next = chars.next()?;
                        if !matches!(next, '"' | '\\' | '$' | '`') {
                            value.push('\\');
                        }
                        value.push(next);
                    }
                    c => value.push(c),
                }
            },
            '\\' => value.push(chars.next()?),
            ' ' | '\t' => return None,
            c => value.push(c),
        }
    }
    Some(value)
}

/// The prefix of the `root=` argument for the boot environment mounted at
/// `mnt`: `org.zfsbootmenu:rootprefix` (`property`, `None` when it could not
/// be read), or the prefix of the distribution named in `ID` or `ID_LIKE` of
/// its `os-release`, or `root=zfs:`.
pub fn root_prefix(files: &dyn Files, property: Option<&str>, mnt: &Path) -> String {
    if let Some(prefix) = property.filter(|prefix| *prefix != "-") {
        return prefix.to_owned();
    }
    os_release_prefix(files, mnt).unwrap_or_else(|| "root=zfs:".to_owned())
}

/// The prefix of the `root=` argument for the distribution named in `ID` or
/// `ID_LIKE` of the `os-release` of the boot environment mounted at `mnt`, if
/// it is one with a known prefix: Void, Ubuntu, Debian, Devuan, Chimera, Arch,
/// Artix, CachyOS, Gentoo or Alpine.
pub fn os_release_prefix(files: &dyn Files, mnt: &Path) -> Option<String> {
    let mut id = String::new();
    let mut id_like = String::new();
    for file in ["usr/lib/os-release", "etc/os-release"] {
        let Ok(text) = files.read_to_string(&mnt.join(file)) else {
            continue;
        };
        for (name, value) in os_release(&text) {
            match name.as_str() {
                "ID" => id = value,
                "ID_LIKE" => id_like = value,
                _ => {}
            }
        }
    }
    // for ostype in $ID $ID_LIKE
    let types = id.split_whitespace().chain(id_like.split_whitespace());
    for ostype in types {
        let prefix = match ostype {
            "void" | "ubuntu" | "debian" | "devuan" | "chimera" => "root=zfs:",
            "arch" | "artix" | "cachyos" => "zfs=",
            "gentoo" | "alpine" => "root=ZFS=",
            _ => continue,
        };
        return Some(prefix.to_owned());
    }
    None
}

/// The device tree of `kernel` in the boot environment mounted at `mnt`, from
/// `org.zfsbootmenu:devicetree` (`property`): an absolute path, in which
/// `%{kernel}` is the kernel's version, or a file in
/// `/boot/dtbs/dtbs-<version>/`. `None` when the property is unset or the file
/// does not exist.
pub fn devicetree(
    files: &dyn Files,
    property: Option<&str>,
    kernel: &str,
    mnt: &Path,
) -> Option<PathBuf> {
    let candidate = devicetree_candidate(property?, kernel, mnt)?;
    files.is_file(&candidate).then_some(candidate)
}

/// The file to try as the device tree of `kernel` for the value of
/// `org.zfsbootmenu:devicetree`, or `None` when the property is unset or
/// its path leaves the boot environment through `..`.
pub fn devicetree_candidate(property: &str, kernel: &str, mnt: &Path) -> Option<PathBuf> {
    if property.is_empty() || property == "-" {
        return None;
    }
    // kver="${kernel#"${kernel%%-*}"-}"
    let base = kernel.split('-').next().unwrap_or(kernel);
    let kver = kernel
        .strip_prefix(base)
        .and_then(|rest| rest.strip_prefix('-'))
        .unwrap_or(kernel);
    let inside = if property.starts_with('/') {
        property.replace("%{kernel}", kver)
    } else {
        format!("/boot/dtbs/dtbs-{kver}/{property}")
    };
    if Path::new(&inside)
        .components()
        .any(|part| part == Component::ParentDir)
    {
        zbm_trace::warn!("ignoring the device tree {inside}: it leaves the boot environment");
        return None;
    }
    Some(PathBuf::from(format!("{}{inside}", mnt.to_string_lossy())))
}

/// The spaces between the root argument and the rest of the command line: the
/// legacy script continues the quoted argument on an indented line.
const SEPARATOR: &str = "           ";

/// What the boot passes to `kexec -l`: the kernel, initramfs, device tree
/// and command line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KexecLoad<'a> {
    /// Where the boot environment is mounted.
    pub mnt: &'a str,
    /// The boot environment.
    pub dataset: &'a str,
    /// The kernel, relative to the boot environment's root.
    pub kernel: &'a str,
    /// The initramfs, relative to the boot environment's root.
    pub initramfs: &'a str,
    /// The device tree, a full path, if any.
    pub dtb: Option<&'a Path>,
    /// The prefix of the root argument, such as `root=zfs:`.
    pub root_prefix: &'a str,
    /// The rest of the command line.
    pub args: &'a str,
}

impl KexecLoad<'_> {
    /// The command line the kernel gets.
    pub fn command_line(&self) -> String {
        format!(
            "{}{}{SEPARATOR}{}",
            self.root_prefix, self.dataset, self.args
        )
    }

    /// Calls `with` with what `kexec -l` loads.
    fn with_image<T>(&self, with: impl FnOnce(&kexec::Image<'_>) -> T) -> T {
        let kernel = format!("{}{}", self.mnt, self.kernel);
        let initrd = format!("{}{}", self.mnt, self.initramfs);
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

    /// Runs `kexec -a -l ...`, with its two output streams merged as `2>&1`
    /// merges them.
    ///
    /// # Errors
    ///
    /// The error that kept `kexec` from starting.
    pub fn run(&self, runner: &dyn Runner) -> std::io::Result<Output> {
        self.with_image(|image| kexec::load(runner, image))
    }
}

#[cfg(test)]
mod devicetree_tests;
#[cfg(test)]
mod tests;
