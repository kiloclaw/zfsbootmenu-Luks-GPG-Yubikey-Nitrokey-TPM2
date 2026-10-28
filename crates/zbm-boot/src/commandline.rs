//! The kernel command line of a boot environment, and the `kexec` commands.
//!
//! The command line is assembled from the user's override, a cache or the
//! boot environment's own settings, with the host ID when it applies. This
//! module also finds the prefix of the `root=` argument and the device tree,
//! and builds the `kexec` commands that load and run the kernel.

use std::path::{Component, Path, PathBuf};

use zbm_config::parent::{CommandLineSource, read_kcl_contents, read_kcl_prop};
use zbm_platform::exec::{Output, Runner};
use zbm_platform::files::Files;
use zbm_state::State;
use zbm_tools::kexec;
use zbm_trace::Message;
use zbm_types::{KernelArg, KernelCommandLine};

pub use zbm_pool::hostid::{ETC_HOSTID, SPL_HOSTID, apparent_hostid as spl_hostid};

/// The command line of a boot environment that sets none.
const DEFAULT_ARGS: &str = "quiet loglevel=4";

/// The arguments of `text`, a command line from `what`; those that are not
/// kernel arguments are left out, with a warning.
fn arguments(text: &str, what: &str, log: &mut Vec<Message>) -> KernelCommandLine {
    let (cmdline, refused) = KernelCommandLine::lenient(text);
    for arg in refused {
        log.push(Message::warning(format!(
            "ignoring {arg} in {what}: it is not one kernel argument"
        )));
    }
    cmdline
}

/// `cmdline`, or the default arguments when it has none.
fn or_default(cmdline: KernelCommandLine) -> KernelCommandLine {
    if cmdline.is_empty() {
        DEFAULT_ARGS.parse().unwrap_or_default()
    } else {
        cmdline
    }
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
) -> KernelCommandLine {
    if dataset.is_empty() {
        log.push(Message::error("filesystem is undefined"));
        return KernelCommandLine::default();
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
        // One argument per line; a failure is not reported.
        let _ = files.write(cache, args.one_per_line().as_bytes());
    }
    log.push(Message::debug(format!("assembled commandline: '{args}'")));
    args
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
) -> KernelCommandLine {
    log.push(Message::debug(format!(
        "using the command line typed for this boot of {dataset}"
    )));
    let typed = Typed {
        source,
        dataset,
        line,
    };
    let value = read_kcl_prop(&typed, dataset, log).unwrap_or_default();
    let args = or_default(arguments(&value, "the command line typed", log));
    let args = edited_args(files, state, dataset, set_hostid, &args, log);
    log.push(Message::debug(format!("assembled commandline: '{args}'")));
    args
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
    let cache = if let Some(path) = &kernel_kcl {
        log.push(Message::debug(format!(
            "found kernel KCL {} on {dataset}, ignoring KCL cache",
            path.display()
        )));
        None
    } else {
        log.push(location());
        state.kcl_cache(dataset, "default")
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
) -> Option<KernelCommandLine> {
    if let Some(text) = state.cmdline_override() {
        log.push(Message::debug(format!(
            "using {}/cmdline as command line for {dataset}",
            state.base().display()
        )));
        return Some(arguments(&text, "zbm.kcl_override", log));
    }
    let cache = cache.filter(|cache| cache_is_valid(files, state, cache))?;
    log.push(Message::debug(format!(
        "using cached KCL from {} as command line for {dataset}",
        cache.display()
    )));
    let text = files.read_to_string(cache).unwrap_or_default();
    Some(arguments(&text, &cache.display().to_string(), log))
}

/// The boot environment's own command line: from the
/// kernel's command line file `kernel_kcl`, or from
/// `org.zfsbootmenu:commandline` with `%{parent}` expanded, or the default
/// arguments when that gives none.
fn own_args(
    files: &dyn Files,
    source: &dyn CommandLineSource,
    dataset: &str,
    kernel_kcl: Option<&Path>,
    log: &mut Vec<Message>,
) -> KernelCommandLine {
    // read_kcl_value with the kernel's .kcl file, which the legacy function
    // passes even when it is empty, so the property is read
    let kcl_path = kernel_kcl.map(Path::to_path_buf).unwrap_or_default();
    let contents = kernel_kcl.and_then(|path| files.read(path).ok());
    let value =
        read_kcl_contents(source, dataset, &kcl_path, contents.as_deref(), log).unwrap_or_default();
    let what = format!("the command line of {dataset}");
    or_default(arguments(&value, &what, log))
}

/// `args` without `root=`, and with `noresume` and the host ID in place of the
/// arguments they replace, when they apply.
fn edited_args(
    files: &dyn Files,
    state: &State,
    dataset: &str,
    set_hostid: bool,
    args: &KernelCommandLine,
    log: &mut Vec<Message>,
) -> KernelCommandLine {
    let mut removals = vec!["root"];
    let mut additions = Vec::new();

    if state.noresume().is_some() {
        log.push(Message::debug(format!(
            "{}/noresume set, expunging from {dataset}",
            state.base().display()
        )));
        removals.push("resume");
        additions.push(KernelArg::new("noresume", None));
    }
    if set_hostid && let Some(hostid) = spl_hostid(files, state, log) {
        log.push(Message::debug(format!(
            "overriding spl_hostid and spl.spl_hostid for {dataset}"
        )));
        removals.extend(["spl_hostid", "spl.spl_hostid"]);
        additions.push(if hostid == "0x00000000" {
            KernelArg::new("spl_hostid", Some("00000000"))
        } else {
            KernelArg::new("spl.spl_hostid", Some(&hostid))
        });
    }

    let mut edited = args.without(&removals);
    edited.extend(additions.into_iter().filter_map(Result::ok));
    edited
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
        if is_root_prefix(prefix) {
            zbm_trace::debug!("using org.zfsbootmenu:rootprefix: {prefix}"; %prefix);
            return prefix.to_owned();
        }
        zbm_trace::warn!(
            prefix,
            "ignoring org.zfsbootmenu:rootprefix: it does not start one kernel argument"
        );
    }
    if let Some(prefix) = os_release_prefix(files, mnt) {
        zbm_trace::debug!("using os-release: {prefix}"; %prefix);
        return prefix;
    }
    zbm_trace::debug!("using default");
    "root=zfs:".to_owned()
}

/// Whether `prefix` can start the `root=` argument: a name, `=` and the
/// start of a value, with no space, quote or control character, so that the
/// name of any boot environment after it makes one argument. Otherwise
/// `org.zfsbootmenu:rootprefix` could add arguments to the command line
/// (ROADMAP.md, bug 27).
pub(crate) fn is_root_prefix(prefix: &str) -> bool {
    !prefix.contains(|c: char| c.is_whitespace() || c.is_control() || c == '"')
        && root_argument(prefix, "pool").is_some()
}

/// The `root=` argument that boots `fs`, after `prefix`: `root=zfs:` and
/// `zroot/ROOT/void` make `root=zfs:zroot/ROOT/void`, and a name with a
/// space is quoted, so the argument stays one. `None` when the two do not
/// make one argument with a value: the prefix has no `=`, no name before
/// it, or a quote.
pub(crate) fn root_argument(prefix: &str, fs: &str) -> Option<KernelArg> {
    let (name, value) = prefix.split_once('=')?;
    KernelArg::new(name, Some(&format!("{value}{fs}"))).ok()
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
        zbm_trace::warn!(
            "ignoring the device tree {inside}: it leaves the boot environment";
            %inside
        );
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
    /// The kernel, relative to the boot environment's root.
    pub kernel: &'a str,
    /// The initramfs, relative to the boot environment's root.
    pub initramfs: &'a str,
    /// The device tree, a full path, if any.
    pub dtb: Option<&'a Path>,
    /// The root argument, such as `root=zfs:zroot/ROOT/void`.
    pub root: &'a KernelArg,
    /// The rest of the command line.
    pub args: &'a KernelCommandLine,
}

impl KexecLoad<'_> {
    /// The command line the kernel gets.
    pub fn command_line(&self) -> String {
        format!("{}{SEPARATOR}{}", self.root, self.args)
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
mod root_tests;
#[cfg(test)]
mod tests;
