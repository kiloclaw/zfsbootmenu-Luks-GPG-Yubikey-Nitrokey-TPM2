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

use zbm_cmdline::args::{ZbmArgs, cmdline_files};
use zbm_cmdline::conf::{self, Conf};
use zbm_cmdline::kcl;
use zbm_cmdline::options::ZbmOptions;
use zbm_platform::error::{Error, Result};
use zbm_platform::{KernelLog, System};
use zbm_tools::terminal;

/// The state directory when the configuration names none.
pub const DEFAULT_BASE: &str = "/zfsbootmenu";

/// Where an embedded command line goes.
const EMBEDDED_KCL: &str = "etc/cmdline.d/zfsbootmenu.conf";

/// Reads `/etc/zfsbootmenu.conf` below `root`.
pub fn read_conf(system: &System<'_>, root: &Path) -> Result<Conf> {
    let path = root.join(conf::PATH.trim_start_matches('/'));
    let text = system
        .files
        .read_to_string(&path)
        .map_err(|error| Error::io(format!("unable to read {}", path.display()), error))?;
    Conf::parse(&text)
}

/// The state directory the configuration names (`BASE`), or the default.
pub fn base(conf: &Conf) -> PathBuf {
    PathBuf::from(
        conf.get("BASE")
            .filter(|base| !base.is_empty())
            .unwrap_or(DEFAULT_BASE),
    )
}

/// The texts `_build_zbm_kcl` reads below `root`: `etc/cmdline`, the
/// `etc/cmdline.d/*.conf` files in name order and `proc/cmdline`, skipping
/// those that cannot be read.
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

/// `zfsbootmenu-parse-commandline`: parses ZFSBootMenu's command line below
/// `root` (normally `/`), logs what the legacy script logs, and writes the
/// state the later stages read: the embedded command line, `zbm.cmdline`,
/// `cmdline.d`, the command line override and the options in
/// `/etc/zfsbootmenu.conf`.
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

    let kcl = kcl::build_zbm_kcl(&zbm_sources(system, root));
    let (options, messages) = ZbmOptions::parse(&ZbmArgs::from_kcl(&kcl));
    let _log = KernelLog::new(
        files,
        &options.loglevel,
        &base_in_root,
        system.processes.id(),
    )
    .with_device(&root.join("dev/kmsg"))
    .install();
    for message in &messages {
        message.emit();
    }

    // get_zbm_kcl and make_cmdline_dir
    let zbm_cmdline = format!("{}\n", kcl.trim_end_matches('\n'));
    let _ = files.write(&base_in_root.join("zbm.cmdline"), zbm_cmdline.as_bytes());
    let dir = base_in_root.join("cmdline.d");
    let _ = files.create_dir_all(&dir);
    for (key, contents) in cmdline_files(&kcl) {
        let _ = files.write(&dir.join(key), contents.as_bytes());
    }
    if let Some(cmdline) = &options.kcl_override {
        let _ = files.write(&base_in_root.join("cmdline"), cmdline.as_bytes());
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
/// terminal (on standard input, as `stty` does), and names the host. The
/// options come from the command line again, for `zbm.lines` and
/// `zbm.columns`, which the configuration does not hold.
pub fn prepare(system: &System<'_>, root: &Path, options: &ZbmOptions) {
    let files = system.files;
    let _ = files.write(&root.join("proc/sys/kernel/printk"), b"0\n");
    if !is_virtual_console(&options.control_term) {
        let lines = options.lines.as_deref().unwrap_or("25");
        let columns = options.columns.as_deref().unwrap_or("80");
        for (setting, value) in [("rows", lines), ("cols", columns)] {
            terminal::stty(system.runner, &[setting, value]);
        }
    }
    let _ = files.write(&root.join("proc/sys/kernel/hostname"), b"ZFSBootMenu\n");
}

/// The options for [`prepare`]: the command line that
/// [`parse_commandline`] saved in `zbm.cmdline`, parsed again without
/// logging, with the control terminal of the configuration.
pub fn saved_options(system: &System<'_>, root: &Path, conf: &Conf) -> ZbmOptions {
    let base = base(conf);
    let path = root
        .join(base.strip_prefix("/").unwrap_or(&base))
        .join("zbm.cmdline");
    let kcl = system.files.read_to_string(&path).unwrap_or_default();
    let (mut options, _) = ZbmOptions::parse(&ZbmArgs::from_kcl(&kcl));
    if let Some(term) = conf.get("control_term").filter(|term| !term.is_empty()) {
        term.clone_into(&mut options.control_term);
    }
    options
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
             zbm.kcl_override=\"quiet 'a b'\" zbm.lines=40",
        );
        let options = parse_commandline(&fake.system(), Path::new("/")).unwrap();
        assert_eq!(options.control_term, "/dev/ttyS0");
        assert_eq!(options.menu_timeout, "3");
        assert_eq!(
            fake.files
                .text("/etc/cmdline.d/zfsbootmenu.conf")
                .as_deref(),
            Some("rd.hostonly=0\n")
        );
        let zbm_cmdline = fake.files.text("/zfsbootmenu/zbm.cmdline").unwrap();
        assert!(zbm_cmdline.starts_with("zbm.timeout=3\nrd.hostonly=0\nloglevel=7\n"));
        assert_eq!(
            fake.files
                .text("/zfsbootmenu/cmdline.d/zbm.prefer")
                .as_deref(),
            Some("tank!\n")
        );
        assert_eq!(
            fake.files.text("/zfsbootmenu/cmdline").as_deref(),
            options.kcl_override.as_deref()
        );
        let conf = Conf::parse(&fake.files.text("/etc/zfsbootmenu.conf").unwrap()).unwrap();
        assert_eq!(conf.get("zbm_prefer_pool"), Some("tank"));
        assert_eq!(conf.get("zbm_require_pool"), Some("yes"));
        assert_eq!(conf.get("control_term"), Some("/dev/ttyS0"));
        assert_eq!(conf.get("BASE"), Some("/zfsbootmenu"));
        assert!(fake.kmsg().contains("<6>ZFSBootMenu: requiring pool tank"));

        // The pre-mount hook finds what it needs again.
        let options = saved_options(&fake.system(), Path::new("/"), &conf);
        assert_eq!(options.lines.as_deref(), Some("40"));
        prepare(&fake.system(), Path::new("/"), &options);
        assert_eq!(fake.runner.lines(), ["stty rows 40", "stty cols 80"]);
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
}
