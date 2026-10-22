//! The kernel command line of a boot environment, and the `kexec` commands.
//!
//! The Rust counterparts of `load_be_cmdline` (with `validate_cmdline_cache`
//! and `get_spl_hostid`), `find_root_prefix`, the device tree lookup of
//! `kexec_kernel`, and the `kexec` commands it runs.

use std::path::{Path, PathBuf};

use zbm_cmdline::kcl;
use zbm_cmdline::parent::{CommandLineSource, read_kcl_contents};
use zbm_core::exec::Command;
use zbm_core::shell::{command_substitution, read_first_line, test_integer};
use zbm_log::Message;
use zbm_state::State;
use zbm_system::files::Files;

/// The module parameter that holds the host ID SPL uses.
pub const SPL_HOSTID: &str = "/sys/module/spl/parameters/spl_hostid";

/// The host ID file.
pub const ETC_HOSTID: &str = "/etc/hostid";

/// The command line of a boot environment that sets none.
const DEFAULT_ARGS: &str = "quiet loglevel=4";

/// What `$( ... )` gives for `text`, followed by the newline of a here-string
/// (`<<< "${var}"`), so text moves between the `kcl` functions as it does
/// between the legacy ones.
fn here(text: &str) -> String {
    format!("{}\n", command_substitution(text))
}

/// The host ID as `get_spl_hostid` reports it (`0x` and eight or more hex
/// digits): from the SPL module parameter if it is not 0, from `/etc/hostid`,
/// or from the host ID that pool import matched.
pub fn spl_hostid(files: &dyn Files, state: &State, log: &mut Vec<Message>) -> Option<String> {
    if let Ok(text) = files.read_to_string(Path::new(SPL_HOSTID)) {
        let value = read_first_line(&text);
        if let Some(number) = test_integer(&value).filter(|number| *number != 0) {
            log.push(Message::debug(format!(
                "hostid from spl.spl_hostid: {value}"
            )));
            // printf "0x%08x" of the decimal value
            return Some(format!("0x{:08x}", number as u64));
        }
    }
    if let Ok(bytes) = files.read(Path::new(ETC_HOSTID))
        && !bytes.is_empty()
    {
        // od -tx4 -N4 reads the first four bytes as a native (little-endian)
        // word, padding a short file with zeros
        let mut word = [0_u8; 4];
        for (target, byte) in word.iter_mut().zip(&bytes) {
            *target = *byte;
        }
        let hostid = format!("{:08x}", u32::from_le_bytes(word));
        log.push(Message::debug(format!("hostid from /etc/hostid: {hostid}")));
        return Some(format!("0x{hostid}"));
    }
    let matched = state.spl_hostid()?;
    log.push(Message::debug(format!(
        "hostid from {}/spl_hostid: {matched}",
        state.base().display()
    )));
    Some(format!("0x{matched}"))
}

/// Whether the command line cache `path` can be used, as
/// `validate_cmdline_cache` decides: it exists and was written after the user
/// declined to resume from hibernation, if they did.
fn cache_is_valid(files: &dyn Files, state: &State, path: &Path) -> bool {
    let Ok(metadata) = files.metadata(path) else {
        return false;
    };
    match state.noresume() {
        None => true,
        Some(noresume) => metadata.modified > noresume,
    }
}

/// The command line of `dataset` booted with `kernel` (may be empty), as
/// `load_be_cmdline` assembles it: the user's override, a valid cache, or the
/// boot environment's own command line (a kernel's `.kcl` file, or
/// `org.zfsbootmenu:commandline` with `%{parent}` expanded) without `root=`,
/// with `noresume` and the host ID when they apply. A computed command line is
/// cached, as the legacy function caches it.
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

    if let Some(text) = state.cmdline_override() {
        log.push(Message::debug(format!(
            "using {}/cmdline as command line for {dataset}",
            state.base().display()
        )));
        return kcl::assemble(&text);
    }
    if let Some(cache) = cache
        .as_deref()
        .filter(|cache| cache_is_valid(files, state, cache))
    {
        log.push(Message::debug(format!(
            "using cached KCL from {} as command line for {dataset}",
            cache.display()
        )));
        let text = files.read_to_string(cache).unwrap_or_default();
        return kcl::assemble(&text);
    }

    let mut removals = vec!["root".to_owned()];
    let mut additions = Vec::new();

    // read_kcl_value with the kernel's .kcl file, which the legacy function
    // passes even when it is empty, so the property is read
    let kcl_path = kernel_kcl.clone().unwrap_or_default();
    let contents = kernel_kcl.as_deref().and_then(|path| files.read(path).ok());
    let value =
        read_kcl_contents(source, dataset, &kcl_path, contents.as_deref(), log).unwrap_or_default();
    let mut args = command_substitution(&kcl::tokenize(&here(&value))).to_owned();
    if args.is_empty() {
        args = DEFAULT_ARGS.to_owned();
    }

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
    let suppressed = kcl::suppress(&here(&args), &removals);
    let args = command_substitution(&kcl::append(&suppressed, &additions)).to_owned();

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
/// `mnt`, as `find_root_prefix` chooses it: `org.zfsbootmenu:rootprefix`
/// (`property`, `None` when it could not be read), or the prefix of the
/// distribution named in `ID` or `ID_LIKE` of its `os-release`, or `root=zfs:`.
pub fn root_prefix(files: &dyn Files, property: Option<&str>, mnt: &Path) -> String {
    if let Some(prefix) = property.filter(|prefix| *prefix != "-") {
        return prefix.to_owned();
    }
    os_release_prefix(files, mnt).unwrap_or_else(|| "root=zfs:".to_owned())
}

/// The prefix of the `root=` argument for the distribution named in `ID` or
/// `ID_LIKE` of the `os-release` of the boot environment mounted at `mnt`, if
/// `find_root_prefix` knows it.
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

/// The device tree of `kernel` in the boot environment mounted at `mnt`, as
/// `kexec_kernel` finds it from `org.zfsbootmenu:devicetree` (`property`): an
/// absolute path, in which `%{kernel}` is the kernel's version, or a file in
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

/// The file `kexec_kernel` tries as the device tree of `kernel` for the value
/// of `org.zfsbootmenu:devicetree`, or `None` when the property is unset.
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
    let mnt = mnt.to_string_lossy();
    let candidate = if property.starts_with('/') {
        format!("{mnt}{}", property.replace("%{kernel}", kver))
    } else {
        format!("{mnt}/boot/dtbs/dtbs-{kver}/{property}")
    };
    Some(PathBuf::from(candidate))
}

/// The spaces between the root argument and the rest of the command line: the
/// legacy script continues the quoted argument on an indented line.
const SEPARATOR: &str = "           ";

/// What `kexec_kernel` passes to `kexec -l`.
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

    /// `kexec -a -l ...`, with its two output streams merged as `2>&1` merges
    /// them.
    pub fn command(&self) -> Command {
        let mut command = Command::new("kexec").args([
            "-a".to_owned(),
            "-l".to_owned(),
            format!("{}{}", self.mnt, self.kernel),
            format!("--initrd={}{}", self.mnt, self.initramfs),
        ]);
        if let Some(dtb) = self.dtb {
            command = command.arg(format!("--dtb={}", dtb.display()));
        }
        command
            .arg(format!("--command-line={}", self.command_line()))
            .merge_stderr()
    }
}

/// `kexec -e -i`, which boots the loaded kernel.
pub fn kexec_execute() -> Command {
    Command::new("kexec").args(["-e", "-i"]).merge_stderr()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};
    use zbm_core::exec::SystemRunner;
    use zbm_system::files::RealFiles;
    use zbm_testkit::fake_zfs::FakeZfs;
    use zbm_testkit::fakes::FakeFiles;
    use zbm_testkit::legacy::{Bash, LOG_STUBS, log_lines};
    use zbm_testkit::{Rng, TempDir, parity};
    use zbm_zfs::Zfs;

    #[test]
    fn host_ids_come_from_spl_then_etc_then_the_match() {
        let files = FakeFiles::new();
        files.dir("/zfsbootmenu");
        let state = State::new(&files, "/zfsbootmenu");
        let mut log = Vec::new();
        assert_eq!(spl_hostid(&files, &state, &mut log), None);
        state.set_spl_hostid("deadbeef").unwrap();
        assert_eq!(
            spl_hostid(&files, &state, &mut log).as_deref(),
            Some("0xdeadbeef")
        );
        files.file(ETC_HOSTID, [0x0c_u8, 0xb1, 0xba, 0x00]);
        assert_eq!(
            spl_hostid(&files, &state, &mut log).as_deref(),
            Some("0x00bab10c")
        );
        files.file(ETC_HOSTID, [0x01_u8]);
        assert_eq!(
            spl_hostid(&files, &state, &mut log).as_deref(),
            Some("0x00000001")
        );
        files.file(SPL_HOSTID, "0\n");
        assert_eq!(
            spl_hostid(&files, &state, &mut log).as_deref(),
            Some("0x00000001")
        );
        files.file(SPL_HOSTID, "3735928559\n");
        assert_eq!(
            spl_hostid(&files, &state, &mut log).as_deref(),
            Some("0xdeadbeef")
        );
        files.file(SPL_HOSTID, "x\n");
        assert_eq!(
            spl_hostid(&files, &state, &mut log).as_deref(),
            Some("0x00000001")
        );
    }

    struct Properties(Vec<(&'static str, &'static str)>);

    impl CommandLineSource for Properties {
        fn commandline(&self, filesystem: &str) -> Option<String> {
            let value = self.0.iter().find(|(fs, _)| *fs == filesystem);
            Some(value.map_or("-", |(_, value)| *value).to_owned())
        }

        fn inherited_source(&self, _: &str) -> Option<String> {
            Some(String::new())
        }
    }

    fn setup() -> (FakeFiles, Properties) {
        let files = FakeFiles::new();
        files.dir("/zfsbootmenu/environments/tank/ROOT/void/kcl");
        let properties = Properties(vec![("tank/ROOT/void", "ro quiet root=bad resume=/dev/x")]);
        (files, properties)
    }

    #[test]
    fn command_lines_come_from_the_property_and_are_cached() {
        let (files, properties) = setup();
        let state = State::new(&files, "/zfsbootmenu");
        let mut log = Vec::new();
        let cmdline = |log: &mut Vec<Message>| {
            be_cmdline(
                &files,
                &properties,
                &state,
                "tank/ROOT/void",
                "/boot/vmlinuz",
                false,
                log,
            )
        };
        assert_eq!(cmdline(&mut log), "ro quiet resume=/dev/x");
        let cache = "/zfsbootmenu/environments/tank/ROOT/void/kcl/default.kcl";
        assert_eq!(
            files.text(cache).as_deref(),
            Some("ro\nquiet\nresume=/dev/x\n")
        );

        // The cache wins until the user declines to resume.
        files.file(cache, "cached\n");
        assert_eq!(cmdline(&mut log), "cached");
        state.set_noresume().unwrap();
        files.set_modified(cache, SystemTime::UNIX_EPOCH);
        assert_eq!(cmdline(&mut log), "ro quiet noresume");

        // The user's override wins over everything.
        state.set_cmdline_override("edited\nby user\n").unwrap();
        assert_eq!(cmdline(&mut log), "edited by user");
        assert!(
            log.iter()
                .any(|message| message.text.contains("/zfsbootmenu/cmdline"))
        );
        assert_eq!(
            be_cmdline(&files, &properties, &state, "", "", false, &mut log),
            ""
        );
    }

    #[test]
    fn kernel_command_lines_and_defaults() {
        let (files, properties) = setup();
        let state = State::new(&files, "/zfsbootmenu");
        let mut log = Vec::new();
        files.file(
            "/zfsbootmenu/environments/tank/ROOT/void/kcl/boot:vmlinuz-6.1.kcl",
            "%{parent} single\n",
        );
        assert_eq!(
            be_cmdline(
                &files,
                &properties,
                &state,
                "tank/ROOT/void",
                "/boot/vmlinuz-6.1",
                false,
                &mut log
            ),
            "ro quiet resume=/dev/x single"
        );
        // No cache is written for a kernel with its own command line.
        assert!(!files.exists(Path::new(
            "/zfsbootmenu/environments/tank/ROOT/void/kcl/default.kcl"
        )));
        let empty = Properties(vec![]);
        assert_eq!(
            be_cmdline(
                &files,
                &empty,
                &state,
                "tank/ROOT/other",
                "",
                false,
                &mut log
            ),
            "quiet loglevel=4"
        );
    }

    #[test]
    fn host_ids_replace_those_of_the_command_line() {
        let (files, properties) = setup();
        let state = State::new(&files, "/zfsbootmenu");
        let mut log = Vec::new();
        files.file(SPL_HOSTID, "3735928559\n");
        let line = be_cmdline(
            &files,
            &properties,
            &state,
            "tank/ROOT/void",
            "",
            true,
            &mut log,
        );
        assert_eq!(line, "ro quiet resume=/dev/x spl.spl_hostid=0xdeadbeef");
        files.remove_file(Path::new(SPL_HOSTID)).unwrap();
        files.file(ETC_HOSTID, [0_u8; 4]);
        files
            .remove_file(Path::new(
                "/zfsbootmenu/environments/tank/ROOT/void/kcl/default.kcl",
            ))
            .unwrap();
        let line = be_cmdline(
            &files,
            &properties,
            &state,
            "tank/ROOT/void",
            "",
            true,
            &mut log,
        );
        assert_eq!(line, "ro quiet resume=/dev/x spl_hostid=00000000");
    }

    #[test]
    fn root_prefixes() {
        let files = FakeFiles::new();
        let mnt = Path::new("/mnt");
        assert_eq!(
            root_prefix(&files, Some("root=custom:"), mnt),
            "root=custom:"
        );
        assert_eq!(root_prefix(&files, Some("-"), mnt), "root=zfs:");
        assert_eq!(root_prefix(&files, None, mnt), "root=zfs:");
        files.file("/mnt/usr/lib/os-release", "NAME=\"Arch Linux\"\nID=arch\n");
        assert_eq!(root_prefix(&files, None, mnt), "zfs=");
        files.file("/mnt/etc/os-release", "ID='gentoo'\n");
        assert_eq!(root_prefix(&files, None, mnt), "root=ZFS=");
        files.file(
            "/mnt/etc/os-release",
            "ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n",
        );
        assert_eq!(root_prefix(&files, None, mnt), "root=zfs:");
        files.file("/mnt/etc/os-release", "ID=other\n");
        files.file("/mnt/usr/lib/os-release", "ID_LIKE=alpine\n");
        assert_eq!(root_prefix(&files, None, mnt), "root=ZFS=");
        assert_eq!(
            os_release("A=\"x \\\"y\\\" \\n\"\n# c\nB='z'\nC=a\\ b\nbad line\nD=\"open\n"),
            [
                ("A".to_owned(), "x \"y\" \\n".to_owned()),
                ("B".to_owned(), "z".to_owned()),
                ("C".to_owned(), "a b".to_owned()),
            ]
        );
    }

    #[test]
    fn device_trees() {
        let files = FakeFiles::new();
        let mnt = Path::new("/mnt");
        files.file("/mnt/boot/dtbs/dtbs-6.1.0/qcom/x13s.dtb", "");
        files.file("/mnt/usr/lib/linux-image-6.1.0/x13s.dtb", "");
        let dtb = |property: &str, kernel: &str| {
            devicetree(&files, Some(property), kernel, mnt).map(|path| path.display().to_string())
        };
        assert_eq!(
            dtb("qcom/x13s.dtb", "/boot/vmlinuz-6.1.0"),
            Some("/mnt/boot/dtbs/dtbs-6.1.0/qcom/x13s.dtb".to_owned())
        );
        assert_eq!(
            dtb(
                "/usr/lib/linux-image-%{kernel}/x13s.dtb",
                "/boot/vmlinuz-6.1.0"
            ),
            Some("/mnt/usr/lib/linux-image-6.1.0/x13s.dtb".to_owned())
        );
        assert_eq!(dtb("qcom/missing.dtb", "/boot/vmlinuz-6.1.0"), None);
        assert_eq!(
            devicetree(&files, Some("-"), "/boot/vmlinuz-6.1.0", mnt),
            None
        );
        assert_eq!(devicetree(&files, None, "/boot/vmlinuz-6.1.0", mnt), None);
        // Without a hyphen the "version" is the whole path, as in the legacy code.
        assert_eq!(dtb("x.dtb", "/boot/vmlinuz"), None);
    }

    #[test]
    fn kexec_commands() {
        let dtb = PathBuf::from("/mnt/boot/dtbs/x.dtb");
        let load = KexecLoad {
            mnt: "/zfsbootmenu/environments/tank/ROOT/void/mnt",
            dataset: "tank/ROOT/void",
            kernel: "/boot/vmlinuz-6.12.9_1",
            initramfs: "/boot/initramfs-6.12.9_1.img",
            dtb: Some(&dtb),
            root_prefix: "root=zfs:",
            args: "ro quiet",
        };
        let command = load.command();
        assert_eq!(command.program(), "kexec");
        let args: Vec<String> = command
            .get_args()
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            [
                "-a",
                "-l",
                "/zfsbootmenu/environments/tank/ROOT/void/mnt/boot/vmlinuz-6.12.9_1",
                "--initrd=/zfsbootmenu/environments/tank/ROOT/void/mnt/boot/initramfs-6.12.9_1.img",
                "--dtb=/mnt/boot/dtbs/x.dtb",
                "--command-line=root=zfs:tank/ROOT/void           ro quiet",
            ]
        );
        assert!(command.merges_stderr());
        assert_eq!(kexec_execute().display(), "kexec -e -i");
    }

    #[test]
    fn the_separator_is_the_legacy_one() {
        // The legacy argument continues after "\" and a newline on a line
        // indented by ten spaces.
        let mut parity = parity!("the_separator_is_the_legacy_one");
        let output: String = parity.legacy("root=zfs:tank ro", || {
            Bash::new()
                .code(&format!(
                    "{}\nshow() {{ root_prefix=root=zfs: fs=tank cli_args=ro; printf '%s' {}; }}",
                    "set -u", "\"${root_prefix}${fs} \\\n          ${cli_args}\""
                ))
                .call("show", &[], b"")
                .stdout_text()
        });
        assert_eq!(output, format!("root=zfs:tank{SEPARATOR}ro"));
    }

    /// The legacy library, with the shim that finds `/lib/zfsbootmenu-kcl.sh`.
    fn legacy_core() -> Bash {
        Bash::new()
            .code(LOG_STUBS)
            .code(
                r#"source() {
                    case "$1" in
                      /lib/*) builtin source "$LEGACY/zfsbootmenu/lib/${1#/lib/}" ;;
                      *) builtin source "$@" ;;
                    esac
                  }"#,
            )
            .source("zfsbootmenu/lib/zfsbootmenu-core.sh")
    }

    /// A starting point of `load_be_cmdline`: properties, files under the
    /// base directory, and the host ID setting.
    #[derive(Debug)]
    struct Scenario {
        /// `org.zfsbootmenu:commandline` of `tank/ROOT`.
        parent: Option<String>,
        /// `org.zfsbootmenu:commandline` of `tank/ROOT/be`.
        own: Option<String>,
        /// The command line cached for the kernel.
        kernel_kcl: Option<String>,
        /// Whether the user edited the command line.
        edited: bool,
        /// Whether `noresume` exists.
        noresume: bool,
        /// Whether a stale `default.kcl` exists.
        stale_default: bool,
        /// `zbm_set_hostid`.
        set_hostid: bool,
    }

    /// The kernel of every scenario.
    const KERNEL: &str = "/boot/vmlinuz-6.1";

    /// The cache of the default command line.
    const DEFAULT_KCL: &str = "environments/tank/ROOT/be/kcl/default.kcl";

    impl Scenario {
        fn generate(rng: &mut Rng) -> Scenario {
            let words = [
                "ro",
                "quiet",
                "root=x",
                "resume=/dev/y",
                "a=\"b c\"",
                "%{parent}",
                "spl_hostid=1",
            ];
            Scenario {
                parent: (rng.below(3) > 0).then(|| rng.join(&words[..5], 3)),
                own: (rng.below(3) > 0).then(|| rng.join(&words, 4)),
                kernel_kcl: (rng.below(3) == 0).then(|| format!("{}\n", rng.join(&words, 3))),
                edited: rng.below(4) == 0,
                noresume: rng.below(3) == 0,
                stale_default: rng.below(3) == 0,
                set_hostid: rng.below(2) == 0,
            }
        }

        /// The pool, with the properties.
        fn zfs(&self) -> FakeZfs {
            let zfs = FakeZfs::new(&["tank", "tank/ROOT", "tank/ROOT/be"]);
            let property = "org.zfsbootmenu:commandline";
            if let Some(value) = &self.parent {
                zfs.set("tank/ROOT", property, value);
            }
            if let Some(value) = &self.own {
                zfs.set("tank/ROOT/be", property, value);
            }
            zfs
        }

        /// A base directory with the files.
        fn base(&self) -> TempDir {
            let dir = TempDir::new("zbm-cmdline");
            std::fs::create_dir_all(dir.join("environments/tank/ROOT/be/kcl")).unwrap();
            if let Some(text) = &self.kernel_kcl {
                dir.write("environments/tank/ROOT/be/kcl/boot:vmlinuz-6.1.kcl", text);
            }
            if self.edited {
                dir.write("cmdline", "edited\nline\n");
            }
            if self.noresume {
                dir.write("noresume", "");
            }
            if self.stale_default {
                dir.write(DEFAULT_KCL, "stale\n");
                let old = SystemTime::now() - Duration::from_secs(3600);
                let file = std::fs::File::options()
                    .write(true)
                    .open(dir.join(DEFAULT_KCL))
                    .unwrap();
                file.set_modified(old).unwrap();
            }
            dir
        }
    }

    /// `text` with the base directory written as `$BASE`, so that it does not
    /// depend on where the test ran.
    fn without_base(text: &str, dir: &TempDir) -> String {
        text.replace(&*dir.path().to_string_lossy(), "$BASE")
    }

    #[test]
    fn command_lines_match_the_legacy_function() {
        let mut parity = parity!("command_lines_match_the_legacy_function");
        let mut rng = Rng::from_env(31);
        for case in 0..30 {
            let scenario = Scenario::generate(&mut rng);
            let zfs = scenario.zfs();
            let (stdout, logged, cache): (String, String, Option<String>) =
                parity.legacy(format!("{scenario:?}"), || {
                    let dir = scenario.base();
                    let legacy = legacy_core()
                        .env("BASE", &dir.path().to_string_lossy())
                        .env("PATH", &zfs.path_env())
                        .env(
                            "zbm_set_hostid",
                            if scenario.set_hostid { "1" } else { "0" },
                        )
                        .call("load_be_cmdline", &["tank/ROOT/be", KERNEL], b"");
                    (
                        without_base(&legacy.stdout_text(), &dir),
                        without_base(&log_lines(&legacy.stderr_text()), &dir),
                        std::fs::read_to_string(dir.join(DEFAULT_KCL)).ok(),
                    )
                });

            let dir = scenario.base();
            let base = dir.path().to_string_lossy().into_owned();
            let state = State::new(&RealFiles, base.as_str());
            let runner = SystemRunner;
            let source = Zfs::with_program(&runner, zfs.program());
            let mut log = Vec::new();
            let rust = be_cmdline(
                &RealFiles,
                &source,
                &state,
                "tank/ROOT/be",
                KERNEL,
                scenario.set_hostid,
                &mut log,
            );
            // kexec_kernel reads it with $( ... ), which drops final newlines
            assert_eq!(
                without_base(&rust, &dir),
                command_substitution(&stdout),
                "case {case}: {logged}"
            );
            let rust_cache = std::fs::read_to_string(dir.join(DEFAULT_KCL)).ok();
            assert_eq!(rust_cache, cache, "case {case}: cache");
            let messages: String = log
                .iter()
                .map(|message| format!("{}\n", message.stderr_line()))
                .collect();
            assert_eq!(without_base(&messages, &dir), logged, "case {case}: log");
        }
    }

    /// The os-release files under `dir`, and the property, for the
    /// description of a case.
    fn os_releases(dir: &TempDir) -> Vec<Option<String>> {
        ["usr/lib/os-release", "etc/os-release"]
            .iter()
            .map(|file| std::fs::read_to_string(dir.join(file)).ok())
            .collect()
    }

    #[test]
    fn root_prefixes_match_the_legacy_function() {
        let mut parity = parity!("root_prefixes_match_the_legacy_function");
        let mut rng = Rng::from_env(32);
        let ids = [
            "void",
            "arch",
            "gentoo",
            "alpine",
            "fedora",
            "\"ubuntu debian\"",
            "'artix'",
            "chimera",
            "",
        ];
        for case in 0..30 {
            let dir = TempDir::new("zbm-prefix");
            for file in ["usr/lib/os-release", "etc/os-release"] {
                if rng.below(3) > 0 {
                    let mut text = String::from("NAME=\"Some Linux\"\n");
                    if rng.below(2) == 0 {
                        text.push_str(&format!("ID={}\n", rng.choose(&ids)));
                    }
                    if rng.below(2) == 0 {
                        text.push_str(&format!("ID_LIKE={}\n", rng.choose(&ids)));
                    }
                    dir.write(file, text);
                }
            }
            let zfs = FakeZfs::new(&["tank", "tank/be"]);
            let property = rng.below(4) == 0;
            if property {
                zfs.set("tank/be", "org.zfsbootmenu:rootprefix", "root=custom:");
            }
            let mnt = dir.path().to_string_lossy().into_owned();
            let input = format!("os-release {:?}, property {property}", os_releases(&dir));
            let legacy: String = parity.legacy(input, || {
                legacy_core()
                    .env("PATH", &zfs.path_env())
                    .call("find_root_prefix", &["tank/be", &mnt], b"")
                    .stdout_text()
            });
            let rust = root_prefix(
                &RealFiles,
                Some(if property { "root=custom:" } else { "-" }),
                dir.path(),
            );
            assert_eq!(format!("{rust}\n"), legacy, "case {case}");
        }
    }
}
