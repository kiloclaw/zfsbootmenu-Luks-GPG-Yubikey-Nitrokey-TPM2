//! The start-up hooks: `zfsbootmenu-parse-commandline.sh` and
//! `zfsbootmenu-preinit.sh`.
//!
//! dracut runs the two scripts in its own shell at different times (the
//! `cmdline` and `pre-mount` hooks), so the shell variables of the first reach
//! the second. The Rust applets are separate processes, so the first also does
//! the part of the second that needs those variables: it appends the parsed
//! options to `/etc/zfsbootmenu.conf`. It also writes `zbm.cmdline` and
//! `cmdline.d`, which the legacy code writes the first time it looks up an
//! argument, so hooks and later applets find them. See "Start-up hooks" in
//! `crates/DESIGN.md`.

use std::fmt::Write as _;
use std::io;
use std::path::{Path, PathBuf};

use zbm_config::args::{ZbmArgs, cmdline_files};
use zbm_config::conf::{self, Conf};
use zbm_config::options::ZbmOptions;
use zbm_platform::console::Size;
use zbm_platform::error::{Error, Result};
use zbm_platform::files::FileKind;
use zbm_platform::{Files, KernelLog, System};
use zbm_types::KernelCommandLine;

use crate::lease;

/// The state directory when the configuration names none.
pub const DEFAULT_BASE: &str = "/zfsbootmenu";

/// Where an embedded command line goes.
const EMBEDDED_KCL: &str = "etc/cmdline.d/zfsbootmenu.conf";

/// The image's name servers.
const RESOLV_CONF: &str = "etc/resolv.conf";

/// Where mkinitcpio's `net` hook leaves its leases, `net-<device>.conf`.
const LEASES: &str = "tmp";

/// The most of a lease that is read, in bytes.
const LEASE_LIMIT: u64 = 64 * 1024;

/// Reads `/etc/zfsbootmenu.conf` below `root`; a line it cannot read is
/// skipped with a warning.
///
/// # Errors
///
/// When the file cannot be read.
pub fn read_conf(system: &System<'_>, root: &Path) -> Result<Conf> {
    let path = root.join(conf::PATH.trim_start_matches('/'));
    let text = system
        .files
        .read_to_string(&path)
        .map_err(|error| Error::io(format!("unable to read {}", path.display()), error))?;
    let conf = Conf::parse(&text);
    for skipped in conf.skipped() {
        zbm_trace::warn!("{skipped}"; %skipped);
    }
    Ok(conf)
}

/// The state directory the configuration names (`BASE`), or the default.
pub fn base(conf: &Conf) -> PathBuf {
    PathBuf::from(
        conf.get("BASE")
            .filter(|base| !base.is_empty())
            .unwrap_or(DEFAULT_BASE),
    )
}

/// The texts the kernel command line is built from, below `root`:
/// `etc/cmdline`, the `etc/cmdline.d/*.conf` files in name order and
/// `proc/cmdline`, skipping those that cannot be read.
pub fn zbm_sources(files: &dyn Files, root: &Path) -> Vec<String> {
    let mut paths = vec![root.join("etc/cmdline")];
    let dir = root.join("etc/cmdline.d");
    for name in files.read_dir(&dir).unwrap_or_default() {
        let bytes = name.as_encoded_bytes();
        if bytes.ends_with(b".conf") && !bytes.starts_with(b".") {
            paths.push(dir.join(name));
        }
    }
    paths.push(root.join("proc/cmdline"));
    paths
        .iter()
        .filter_map(|path| files.read(path).ok())
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .collect()
}

/// ZFSBootMenu's command line: the arguments of each of `sources`, as the
/// kernel splits them, one after the other; and the text of what is not an
/// argument in them.
pub fn zbm_cmdline(sources: &[String]) -> (KernelCommandLine, Vec<String>) {
    let mut cmdline = KernelCommandLine::default();
    let mut refused = Vec::new();
    for source in sources {
        let (args, left_out) = KernelCommandLine::lenient(source);
        cmdline.extend(args.args().iter().cloned());
        refused.extend(left_out);
    }
    (cmdline, refused)
}

/// `zfsbootmenu-parse-commandline`: parses ZFSBootMenu's command line below
/// `root` (normally `/`), logs what the legacy script logs, and writes the
/// state the later stages read: the embedded command line, `zbm.cmdline`,
/// `cmdline.d`, the command line override and the options in
/// `/etc/zfsbootmenu.conf`.
///
/// # Errors
///
/// When the configuration cannot be read or parsed.
pub fn parse_commandline(system: &System<'_>, root: &Path) -> Result<ZbmOptions> {
    let files = system.files;
    let conf = read_conf(system, root)?;
    if let Some(embedded) = conf.get("embedded_kcl").filter(|kcl| !kcl.is_empty()) {
        let path = root.join(EMBEDDED_KCL);
        if let Some(dir) = path.parent() {
            let _ = files.create_dir_all(dir);
        }
        let _ = files.write(&path, format!("{embedded}\n").as_bytes());
    }
    let base = base(&conf);
    let base_in_root = root.join(base.strip_prefix("/").unwrap_or(&base));
    let _ = files.create_dir_all(&base_in_root);

    let (cmdline, refused) = zbm_cmdline(&zbm_sources(files, root));
    let (options, messages) = ZbmOptions::parse(&ZbmArgs::new(&cmdline));
    let _log = KernelLog::new(
        files,
        &options.loglevel.to_string(),
        &base_in_root,
        system.processes.id(),
    )
    .with_device(&root.join("dev/kmsg"))
    .install();
    for arg in refused {
        zbm_trace::warn!(
            arg,
            "ignoring part of the command line: it is not one kernel argument"
        );
    }
    for message in &messages {
        message.emit();
    }

    // One argument per line, as the legacy script tokenized it.
    let _ = files.write(
        &base_in_root.join("zbm.cmdline"),
        cmdline.one_per_line().as_bytes(),
    );
    let dir = base_in_root.join("cmdline.d");
    let _ = files.create_dir_all(&dir);
    for (key, contents) in cmdline_files(&cmdline) {
        let _ = files.write(&dir.join(key), contents.as_bytes());
    }
    if let Some(cmdline) = &options.kcl_override {
        let override_file = base_in_root.join("cmdline");
        let _ = files.write(&override_file, cmdline.one_per_line().as_bytes());
    }

    let conf_path = root.join(conf::PATH.trim_start_matches('/'));
    let additions = options.conf(&base.to_string_lossy());
    files
        .append(&conf_path, additions.as_bytes())
        .map_err(|error| Error::io(format!("unable to write {}", conf_path.display()), error))?;
    Ok(options)
}

/// Whether `zfsbootmenu-preinit.sh` leaves the terminal size alone: the
/// control terminal matches `/dev/tty[0-9]`, a virtual console.
pub fn is_virtual_console(control_term: &str) -> bool {
    control_term
        .match_indices("/dev/tty")
        .any(|(start, found)| {
            control_term[start + found.len()..]
                .bytes()
                .next()
                .is_some_and(|byte| byte.is_ascii_digit())
        })
}

/// What `zfsbootmenu-preinit.sh` does before it starts the start-up program:
/// turns kernel messages on the console off, sets the size of a serial
/// terminal (on standard input, as `stty` did), and names the host. The
/// options come from the command line again, for `zbm.lines` and
/// `zbm.columns`, which the configuration does not hold. It also writes the
/// name servers of mkinitcpio's `net` hook, which the legacy hook did not:
/// see [`lease`].
pub fn prepare(system: &System<'_>, root: &Path, options: &ZbmOptions) {
    let files = system.files;
    let _ = files.write(&root.join("proc/sys/kernel/printk"), b"0\n");
    if !is_virtual_console(&options.control_term) {
        let number = |value: Option<&str>, default| value.unwrap_or(default).parse().ok();
        let rows = number(options.lines.as_deref(), "25");
        let columns = number(options.columns.as_deref(), "80");
        // A console that is no terminal keeps its size, and so does a side
        // that is no number, as when `stty` failed.
        if let Ok(size) = system.console.size() {
            let size = Size {
                rows: rows.unwrap_or(size.rows),
                columns: columns.unwrap_or(size.columns),
            };
            let _ = system.console.resize(size);
        }
    }
    let _ = files.write(&root.join("proc/sys/kernel/hostname"), b"ZFSBootMenu\n");
    write_name_servers(files, root);
}

/// Writes `/etc/resolv.conf` below `root` from the leases that mkinitcpio's
/// `net` hook leaves, `/tmp/net-*.conf` in name order, when the image has
/// none or an empty one: a `nameserver` line for each server
/// ([`lease::name_servers`]), replaced whole, mode 0644. A file or link that
/// an earlier hook made is kept, and nothing is written without a server.
fn write_name_servers(files: &dyn Files, root: &Path) {
    let path = root.join(RESOLV_CONF);
    if !needs_name_servers(files, &path) {
        return;
    }
    let servers = lease::name_servers(&leases(files, root));
    if servers.is_empty() {
        return;
    }
    let text = servers.iter().fold(String::new(), |mut text, server| {
        let _ = writeln!(text, "nameserver {server}");
        text
    });
    if let Err(error) = files
        .replace(&path, text.as_bytes())
        .and_then(|()| files.set_mode(&path, 0o644))
    {
        zbm_trace::warn!(
            "unable to write the name servers of the DHCP lease to {path}: {error}";
            path = %path.display(), %error
        );
    }
}

/// Whether the image's `resolv.conf` at `path` is missing, or an empty file
/// that is no link.
fn needs_name_servers(files: &dyn Files, path: &Path) -> bool {
    if files.read_link(path).is_ok() {
        return false;
    }
    match files.metadata(path) {
        Ok(metadata) => metadata.kind == FileKind::File && metadata.len == 0,
        Err(error) => error.kind() == io::ErrorKind::NotFound,
    }
}

/// The text of the leases below `root`, `tmp/net-*.conf` in name order, one
/// after the other; a lease that is no regular file, or is larger than
/// [`LEASE_LIMIT`], is skipped.
fn leases(files: &dyn Files, root: &Path) -> String {
    let dir = root.join(LEASES);
    let mut text = String::new();
    for name in files.read_dir(&dir).unwrap_or_default() {
        let bytes = name.as_encoded_bytes();
        if !bytes.starts_with(b"net-") || !bytes.ends_with(b".conf") {
            continue;
        }
        if let Ok(lease) = files.read_regular(&dir.join(&name), LEASE_LIMIT) {
            text.push_str(&String::from_utf8_lossy(&lease));
            text.push('\n');
        }
    }
    text
}

/// The command line that [`parse_commandline`] saved in `zbm.cmdline`, in
/// the state directory of `conf` below `root`; empty when there is none.
pub fn saved_commandline(system: &System<'_>, root: &Path, conf: &Conf) -> String {
    let base = base(conf);
    let path = root
        .join(base.strip_prefix("/").unwrap_or(&base))
        .join("zbm.cmdline");
    system.files.read_to_string(&path).unwrap_or_default()
}

/// The options for [`prepare`]: the command line that
/// [`parse_commandline`] saved in `zbm.cmdline`, parsed again without
/// logging, with the control terminal of the configuration.
pub fn saved_options(system: &System<'_>, root: &Path, conf: &Conf) -> ZbmOptions {
    let kcl = saved_commandline(system, root, conf);
    let (mut options, _) = ZbmOptions::parse(&ZbmArgs::from_cmdline(&kcl));
    if let Some(term) = conf.get("control_term").filter(|term| !term.is_empty()) {
        term.clone_into(&mut options.control_term);
    }
    options
}

/// Chooses the shell of the runtime's programs ([`zbm_platform::shell`]):
/// brush when the image below `root` has it and the saved command line of
/// `conf` does not ask for Bash (`zbm.shell=bash`); Bash otherwise. The
/// choice is also written to `shell` in the state directory, for the SSH
/// logins, which do not descend from the supervisor.
pub fn shell(system: &System<'_>, root: &Path, conf: &Conf) -> &'static str {
    let kcl = saved_commandline(system, root, conf);
    let wanted = zbm_config::args::Args::lookup(
        &ZbmArgs::from_cmdline(&kcl),
        &zbm_config::spec::cmdline::SHELL,
        &mut Vec::new(),
    );
    let brush = root.join(zbm_platform::shell::BRUSH.trim_start_matches('/'));
    let shell = zbm_platform::shell::choose(wanted.as_deref(), system.files.is_file(&brush));
    let base = base(conf);
    let path = root
        .join(base.strip_prefix("/").unwrap_or(&base))
        .join("shell");
    let _ = system.files.write(&path, format!("{shell}\n").as_bytes());
    shell
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::QEMU_LEASE;
    use zbm_testkit::fakes::{FakeSystem, FileOperation};

    const IMAGE_CONF: &str = "\
[ -n \"${_ETC_ZFSBOOTMENU_CONF}\" ] && return
readonly _ETC_ZFSBOOTMENU_CONF=1
export ZBM_RELEASE_BUILD=\"\"
export embedded_kcl=\"rd.hostonly=0\"
";

    fn setup(cmdline: &str) -> FakeSystem {
        let fake = FakeSystem::new();
        fake.files
            .file("/etc/zfsbootmenu.conf", IMAGE_CONF)
            .file("/proc/cmdline", cmdline)
            .file("/etc/cmdline", "zbm.timeout=3\n")
            .dir("/proc/sys/kernel");
        fake
    }

    #[test]
    fn parsing_writes_the_state_and_the_configuration() {
        let fake = setup(
            "loglevel=7 console=ttyS0,115200 zbm.prefer=tank! \
             zbm.kcl_override=\"quiet x='a b'\" zbm.lines=40",
        );
        let options = parse_commandline(&fake.system(), Path::new("/")).unwrap();
        assert_eq!(options.control_term, "/dev/ttyS0");
        assert_eq!(options.menu_timeout, zbm_types::Timeout::Seconds(3));
        assert_eq!(
            fake.files
                .text("/etc/cmdline.d/zfsbootmenu.conf")
                .as_deref(),
            Some("rd.hostonly=0\n")
        );
        let zbm_config = fake.files.text("/zfsbootmenu/zbm.cmdline").unwrap();
        assert!(zbm_config.starts_with("zbm.timeout=3\nrd.hostonly=0\nloglevel=7\n"));
        assert_eq!(
            fake.files
                .text("/zfsbootmenu/cmdline.d/zbm.prefer")
                .as_deref(),
            Some("tank!\n")
        );
        assert_eq!(
            fake.files.text("/zfsbootmenu/cmdline").as_deref(),
            Some("quiet\nx=\"a b\"\n")
        );
        let conf = Conf::parse(&fake.files.text("/etc/zfsbootmenu.conf").unwrap());
        assert_eq!(conf.get("zbm_prefer_pool"), Some("tank"));
        assert_eq!(conf.get("zbm_require_pool"), Some("yes"));
        assert_eq!(conf.get("control_term"), Some("/dev/ttyS0"));
        assert_eq!(conf.get("BASE"), Some("/zfsbootmenu"));
        assert!(fake.kmsg().contains("<6>ZFSBootMenu: requiring pool tank"));

        // The pre-mount hook finds what it needs again.
        let options = saved_options(&fake.system(), Path::new("/"), &conf);
        assert_eq!(options.lines.as_deref(), Some("40"));
        prepare(&fake.system(), Path::new("/"), &options);
        assert!(fake.runner.lines().is_empty());
        assert_eq!(
            zbm_platform::console::Console::size(&fake.console).unwrap(),
            zbm_platform::console::Size {
                rows: 40,
                columns: 80
            }
        );
        assert_eq!(
            fake.files.text("/proc/sys/kernel/printk").as_deref(),
            Some("0\n")
        );
        assert_eq!(
            fake.files.text("/proc/sys/kernel/hostname").as_deref(),
            Some("ZFSBootMenu\n")
        );
    }

    #[test]
    fn virtual_consoles_keep_their_size() {
        assert!(is_virtual_console("/dev/tty1"));
        assert!(is_virtual_console("/dev/tty12"));
        assert!(!is_virtual_console("/dev/ttyS0"));
        assert!(!is_virtual_console("/dev/hvc0"));
        assert!(!is_virtual_console("/dev/tty"));
        let fake = setup("quiet");
        let options = parse_commandline(&fake.system(), Path::new("/")).unwrap();
        assert_eq!(options.control_term, "/dev/tty1");
        prepare(&fake.system(), Path::new("/"), &options);
        assert!(fake.runner.lines().is_empty());
    }

    #[test]
    fn a_missing_configuration_is_an_error() {
        let fake = FakeSystem::new();
        assert!(parse_commandline(&fake.system(), Path::new("/")).is_err());
    }

    #[test]
    fn brush_is_the_shell_where_the_image_has_it_and_bash_is_not_asked_for() {
        let without = setup("quiet");
        let system = without.system();
        let conf = read_conf(&system, Path::new("/")).unwrap();
        parse_commandline(&system, Path::new("/")).unwrap();
        assert_eq!(shell(&system, Path::new("/"), &conf), "/bin/bash");

        without.files.file("/usr/bin/brush", "");
        assert_eq!(shell(&system, Path::new("/"), &conf), "/usr/bin/brush");
        let saved = system.files.read_to_string(Path::new("/zfsbootmenu/shell"));
        assert_eq!(saved.unwrap(), "/usr/bin/brush\n");

        let asked = setup("quiet zbm.shell=bash");
        asked.files.file("/usr/bin/brush", "");
        let system = asked.system();
        let conf = read_conf(&system, Path::new("/")).unwrap();
        parse_commandline(&system, Path::new("/")).unwrap();
        assert_eq!(shell(&system, Path::new("/"), &conf), "/bin/bash");
    }

    /// An image after mkinitcpio's `net` hook: no `/etc/resolv.conf`, and
    /// `leases` in `/tmp` by name.
    fn after_net(leases: &[(&str, &str)]) -> FakeSystem {
        let fake = setup("quiet");
        fake.files.dir("/etc").dir("/tmp");
        for (name, lease) in leases {
            fake.files.file(Path::new("/tmp").join(name), lease);
        }
        fake
    }

    /// The image's `/etc/resolv.conf` after [`prepare`].
    fn prepared(fake: &FakeSystem) -> Option<String> {
        let system = fake.system();
        prepare(
            &system,
            Path::new("/"),
            &saved_options(&system, Path::new("/"), &Conf::default()),
        );
        fake.files.text("/etc/resolv.conf")
    }

    #[test]
    fn the_name_servers_of_net_s_lease_become_resolv_conf() {
        let fake = after_net(&[("net-eth0.conf", QEMU_LEASE)]);

        assert_eq!(prepared(&fake).as_deref(), Some("nameserver 10.0.2.3\n"));
        let in_etc: Vec<FileOperation> = fake
            .files
            .operations()
            .into_iter()
            .filter(|operation| match operation {
                FileOperation::Write(path, _) | FileOperation::SetMode(path, _) => {
                    !path.starts_with("/proc")
                }
                _ => true,
            })
            .collect();
        let temporary = format!("/etc/.resolv.conf.{}.tmp", std::process::id());
        assert_eq!(
            in_etc,
            [
                FileOperation::Write(temporary.clone().into(), b"nameserver 10.0.2.3\n".to_vec()),
                FileOperation::Rename(temporary.into(), "/etc/resolv.conf".into()),
                FileOperation::SetMode("/etc/resolv.conf".into(), 0o644),
            ]
        );
    }

    #[test]
    fn unquoted_name_servers_from_an_older_ipconfig_become_resolv_conf() {
        let fake = after_net(&[("net-eth0.conf", "IPV4DNS0=10.0.2.3\nIPV4DNS1=0.0.0.0\n")]);

        assert_eq!(prepared(&fake).as_deref(), Some("nameserver 10.0.2.3\n"));
    }

    #[test]
    fn a_resolv_conf_that_is_not_empty_is_kept() {
        let empty = after_net(&[("net-eth0.conf", QEMU_LEASE)]);
        empty.files.file("/etc/resolv.conf", "");
        assert_eq!(prepared(&empty).as_deref(), Some("nameserver 10.0.2.3\n"));

        let fake = after_net(&[("net-eth0.conf", QEMU_LEASE)]);
        fake.files.file("/etc/resolv.conf", "nameserver 1.1.1.1\n");
        assert_eq!(prepared(&fake).as_deref(), Some("nameserver 1.1.1.1\n"));

        let linked = after_net(&[("net-eth0.conf", QEMU_LEASE)]);
        linked.files.symlink("/etc/resolv.conf", "/run/resolv.conf");
        assert_eq!(prepared(&linked), None);
        assert_eq!(
            linked.files.read_link(Path::new("/etc/resolv.conf")).ok(),
            Some(PathBuf::from("/run/resolv.conf"))
        );

        let directory = after_net(&[("net-eth0.conf", QEMU_LEASE)]);
        directory.files.dir("/etc/resolv.conf");
        assert_eq!(prepared(&directory), None);
        assert!(directory.files.is_dir(Path::new("/etc/resolv.conf")));
    }

    #[test]
    fn no_lease_writes_no_resolv_conf() {
        let without = after_net(&[]);
        assert_eq!(prepared(&without), None);

        let other_files = after_net(&[
            ("dhcp.conf", "IPV4DNS0='192.0.2.1'\n"),
            ("net-eth0.leases", "IPV4DNS0='192.0.2.1'\n"),
        ]);
        assert_eq!(prepared(&other_files), None);

        let unset = after_net(&[(
            "net-eth0.conf",
            "IPV4DNS0='0.0.0.0'\nHOSTNAME='$(reboot)'\n",
        )]);
        assert_eq!(prepared(&unset), None);

        let linked = after_net(&[]);
        linked.files.file("/etc/lease", "IPV4DNS0='192.0.2.1'\n");
        linked.files.symlink("/tmp/net-eth0.conf", "/etc/lease");
        linked.files.fifo("/tmp/net-eth1.conf");
        linked.files.sized("/tmp/net-eth2.conf", LEASE_LIMIT + 1);
        assert_eq!(prepared(&linked), None);

        // The lease that follows them is read.
        linked.files.file("/tmp/net-eth3.conf", QEMU_LEASE);
        assert_eq!(prepared(&linked).as_deref(), Some("nameserver 10.0.2.3\n"));
    }

    #[test]
    fn at_most_three_servers_in_file_order() {
        let fake = after_net(&[
            (
                "net-eth1.conf",
                "IPV4DNS0='192.0.2.2'\nIPV4DNS1='192.0.2.3'\n",
            ),
            (
                "net-eth0.conf",
                "IPV4DNS0='192.0.2.1'\nIPV4DNS1='192.0.2.2'",
            ),
            ("net-eth2.conf", "IPV4DNS0='192.0.2.4'\n"),
        ]);

        assert_eq!(
            prepared(&fake).as_deref(),
            Some("nameserver 192.0.2.1\nnameserver 192.0.2.2\nnameserver 192.0.2.3\n")
        );
    }

    #[test]
    fn a_resolv_conf_that_cannot_be_written_is_left_alone() {
        let fake = after_net(&[("net-eth0.conf", QEMU_LEASE)]);
        let temporary = format!("/etc/.resolv.conf.{}.tmp", std::process::id());
        fake.files
            .fail(&temporary, io::ErrorKind::ReadOnlyFilesystem);

        assert_eq!(prepared(&fake), None);
    }
}
