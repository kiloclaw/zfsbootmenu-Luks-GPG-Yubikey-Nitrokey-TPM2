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

use std::path::{Path, PathBuf};

use zbm_config::args::{ZbmArgs, cmdline_files};
use zbm_config::conf::{self, Conf};
use zbm_config::options::ZbmOptions;
use zbm_platform::console::Size;
use zbm_platform::error::{Error, Result};
use zbm_platform::{KernelLog, System};
use zbm_types::KernelCommandLine;

/// The state directory when the configuration names none.
pub const DEFAULT_BASE: &str = "/zfsbootmenu";

/// Where an embedded command line goes.
const EMBEDDED_KCL: &str = "etc/cmdline.d/zfsbootmenu.conf";

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
pub fn zbm_sources(system: &System<'_>, root: &Path) -> Vec<String> {
    let mut paths = vec![root.join("etc/cmdline")];
    let dir = root.join("etc/cmdline.d");
    for name in system.files.read_dir(&dir).unwrap_or_default() {
        let bytes = name.as_encoded_bytes();
        if bytes.ends_with(b".conf") && !bytes.starts_with(b".") {
            paths.push(dir.join(name));
        }
    }
    paths.push(root.join("proc/cmdline"));
    paths
        .iter()
        .filter_map(|path| system.files.read(path).ok())
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

    let (cmdline, refused) = zbm_cmdline(&zbm_sources(system, root));
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
/// `zbm.columns`, which the configuration does not hold.
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
    use zbm_testkit::fakes::FakeSystem;

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
}
