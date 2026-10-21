//! What the minimal runtime of Phase 5 cannot handle yet.
//!
//! The Phase 5 runtime boots only the default boot environment of an
//! unencrypted pool. Before it does anything the legacy runtime would do
//! differently, it lists what it cannot handle with [`unsupported`] and, if the
//! list is not empty, logs each item and opens the recovery shell (see "The
//! minimal runtime of Phase 5" in `crates/DESIGN.md`). Phase 6 shortens the
//! list as its packages arrive.

use std::fmt;
use std::path::{Path, PathBuf};

use zbm_cmdline::options::ZbmOptions;
use zbm_core::shell::test_integer;
use zbm_system::files::Files;

use crate::hooks;

/// The message of the recovery shell when something is not supported yet.
pub const MESSAGE: &str = "not supported by this build of ZFSBootMenu yet";

/// Where the image keeps its crypttab.
pub const CRYPTTAB: &str = "/etc/zfsbootmenu/crypttab";

/// How the legacy runtime would choose LUKS volumes (`luks_load_config` and
/// `init.d/45-luks-unlock`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LuksMode {
    /// The image has no `cryptsetup`, so LUKS is never tried.
    Unavailable,
    /// `zbm.luks=off` (or `no`, `none`, `0`, `false`, `disable`, `disabled`).
    Disabled,
    /// Every LUKS volume found on the machine: `zbm.luks` empty, `auto`, `on`,
    /// `yes`, `1` or `true`, or neither `zbm.luks` nor a crypttab.
    Auto,
    /// The volumes `zbm.luks` lists.
    CommandLine,
    /// The volumes of the crypttab.
    Crypttab,
}

impl LuksMode {
    /// The mode for the value of `zbm.luks` (if given), whether the crypttab
    /// is readable, and whether the image has `cryptsetup`, in the legacy
    /// order of checks.
    pub fn decide(zbm_luks: Option<&str>, crypttab: bool, cryptsetup: bool) -> LuksMode {
        if !cryptsetup {
            return LuksMode::Unavailable;
        }
        match zbm_luks.map(str::to_lowercase).as_deref() {
            Some("off" | "no" | "none" | "0" | "false" | "disable" | "disabled") => {
                LuksMode::Disabled
            }
            Some("" | "auto" | "on" | "yes" | "1" | "true") => LuksMode::Auto,
            Some(_) => LuksMode::CommandLine,
            None if crypttab => LuksMode::Crypttab,
            None => LuksMode::Auto,
        }
    }
}

/// What the runtime found out about the machine before booting.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Facts {
    /// How LUKS volumes would be chosen; `None` counts as unavailable.
    pub luks: Option<LuksMode>,
    /// The LUKS volumes found on the machine, for [`LuksMode::Auto`].
    pub luks_volumes: Vec<String>,
    /// The executable `load-key.d` hooks.
    pub load_key_hooks: Vec<PathBuf>,
    /// The default boot environment (`bootfs`), if one was found.
    pub bootfs: Option<String>,
    /// The encryption root of the default boot environment, if it is locked.
    pub locked_encryption_root: Option<String>,
    /// Whether a key was pressed during the countdown.
    pub countdown_interrupted: bool,
}

impl Facts {
    /// Fills in what the files tell: the hooks below `hooks_root` and whether
    /// the crypttab is readable, for the given `zbm.luks` and `cryptsetup`.
    pub fn from_files(
        files: &dyn Files,
        hooks_root: &Path,
        zbm_luks: Option<&str>,
        cryptsetup: bool,
    ) -> Facts {
        let crypttab = files.read(Path::new(CRYPTTAB)).is_ok();
        Facts {
            luks: Some(LuksMode::decide(zbm_luks, crypttab, cryptsetup)),
            load_key_hooks: hooks::executable_hooks(files, hooks_root, "load-key.d"),
            ..Facts::default()
        }
    }
}

/// One thing the Phase 5 runtime cannot handle yet.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Unsupported {
    /// LUKS volumes from a crypttab or `zbm.luks`.
    LuksConfiguration(LuksMode),
    /// LUKS volumes found on the machine.
    LuksVolumes(Vec<String>),
    /// `load-key.d` hooks.
    LoadKeyHooks(Vec<PathBuf>),
    /// A locked encryption root.
    LockedEncryptionRoot(String),
    /// An import policy other than the default, `hostid`.
    ImportPolicy(String),
    /// `zbm.prefer`.
    PreferredPool(String),
    /// `zbm.wait_for`.
    WaitForDevices(String),
    /// `zbm.hookroot`.
    HookRoot(String),
    /// The menu: `zbm.show`, no default boot environment, or a key pressed during
    /// the countdown.
    Menu(&'static str),
}

impl fmt::Display for Unsupported {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Unsupported::LuksConfiguration(LuksMode::Crypttab) => {
                write!(formatter, "LUKS volumes from {CRYPTTAB}")
            }
            Unsupported::LuksConfiguration(_) => formatter.write_str("LUKS volumes from zbm.luks"),
            Unsupported::LuksVolumes(volumes) => {
                write!(formatter, "LUKS volumes: {}", volumes.join(", "))
            }
            Unsupported::LoadKeyHooks(hooks) => {
                let names: Vec<String> = hooks
                    .iter()
                    .map(|hook| hook.display().to_string())
                    .collect();
                write!(formatter, "load-key hooks: {}", names.join(", "))
            }
            Unsupported::LockedEncryptionRoot(root) => {
                write!(formatter, "the locked encryption root {root}")
            }
            Unsupported::ImportPolicy(policy) => {
                write!(formatter, "the import policy {policy}")
            }
            Unsupported::PreferredPool(pool) => write!(formatter, "zbm.prefer={pool}"),
            Unsupported::WaitForDevices(devices) => write!(formatter, "zbm.wait_for={devices}"),
            Unsupported::HookRoot(root) => write!(formatter, "zbm.hookroot={root}"),
            Unsupported::Menu(reason) => write!(formatter, "the menu ({reason})"),
        }
    }
}

/// Everything in `options` and `facts` that the Phase 5 runtime cannot handle
/// yet, in the order the legacy start-up meets it: [`before_start`], then
/// [`before_boot`].
pub fn unsupported(options: &ZbmOptions, facts: &Facts) -> Vec<Unsupported> {
    let mut list = before_start(options, facts);
    list.extend(before_boot(options, facts));
    list
}

/// What the start-up stages cannot handle yet: options beyond the default
/// boot and LUKS volumes.
pub fn before_start(options: &ZbmOptions, facts: &Facts) -> Vec<Unsupported> {
    let mut list = Vec::new();
    if !options.wait_for_devices.is_empty() {
        list.push(Unsupported::WaitForDevices(
            options.wait_for_devices.clone(),
        ));
    }
    if !options.hook_root.is_empty() {
        list.push(Unsupported::HookRoot(options.hook_root.clone()));
    }
    match facts.luks.unwrap_or(LuksMode::Unavailable) {
        mode @ (LuksMode::CommandLine | LuksMode::Crypttab) => {
            list.push(Unsupported::LuksConfiguration(mode));
        }
        LuksMode::Auto if !facts.luks_volumes.is_empty() => {
            list.push(Unsupported::LuksVolumes(facts.luks_volumes.clone()));
        }
        _ => {}
    }
    if options.import_policy != "hostid" {
        list.push(Unsupported::ImportPolicy(options.import_policy.clone()));
    }
    if !options.prefer_pool.is_empty() {
        let prefer = if options.prefer_bootfs.is_empty() {
            &options.prefer_pool
        } else {
            &options.prefer_bootfs
        };
        list.push(Unsupported::PreferredPool(prefer.clone()));
    }
    list
}

/// What booting the default boot environment cannot handle yet: the menu,
/// locked encryption roots and `load-key.d` hooks.
pub fn before_boot(options: &ZbmOptions, facts: &Facts) -> Vec<Unsupported> {
    let mut list = Vec::new();
    // `[ "${menu_timeout}" -ge 0 ] && [ -n "${BOOTFS}" ]` decides between the
    // countdown and the menu
    if !test_integer(&options.menu_timeout).is_some_and(|timeout| timeout >= 0) {
        list.push(Unsupported::Menu("zbm.show"));
    } else if facts.bootfs.is_none() {
        list.push(Unsupported::Menu("no default boot environment"));
    } else if facts.countdown_interrupted {
        list.push(Unsupported::Menu("a key was pressed during the countdown"));
    }
    if let Some(root) = &facts.locked_encryption_root {
        list.push(Unsupported::LockedEncryptionRoot(root.clone()));
    }
    if !facts.load_key_hooks.is_empty() {
        list.push(Unsupported::LoadKeyHooks(facts.load_key_hooks.clone()));
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbm_cmdline::args::ZbmArgs;
    use zbm_testutil::fakes::FakeFiles;

    fn options(cmdline: &str) -> ZbmOptions {
        ZbmOptions::parse(&ZbmArgs::from_cmdline(cmdline)).0
    }

    fn supported() -> Facts {
        Facts {
            luks: Some(LuksMode::Auto),
            bootfs: Some("tank/ROOT/void".to_owned()),
            ..Facts::default()
        }
    }

    #[test]
    fn the_default_boot_is_supported() {
        assert_eq!(unsupported(&options(""), &supported()), []);
        assert_eq!(unsupported(&options("zbm.timeout=0"), &supported()), []);
        assert_eq!(unsupported(&options("zbm.skip"), &supported()), []);
        assert_eq!(
            unsupported(
                &options("zbm.luks=off zbm.import_policy=hostid"),
                &supported()
            ),
            []
        );
        let none = Facts {
            luks: None,
            ..supported()
        };
        assert_eq!(unsupported(&options(""), &none), []);
    }

    #[test]
    fn options_beyond_the_default_boot() {
        let list = unsupported(
            &options(concat!(
                "zbm.wait_for=/dev/sda zbm.hookroot=tank/hooks ",
                "zbm.import_policy=force zbm.prefer=tank/ROOT/x!",
            )),
            &supported(),
        );
        let text: Vec<String> = list.iter().map(ToString::to_string).collect();
        assert_eq!(
            text,
            [
                "zbm.wait_for=/dev/sda",
                "zbm.hookroot=tank/hooks",
                "the import policy force",
                "zbm.prefer=tank/ROOT/x",
            ]
        );
        assert_eq!(
            unsupported(&options("zbm.force_import"), &supported()),
            [Unsupported::ImportPolicy("force".to_owned())]
        );
        assert_eq!(
            unsupported(&options("zbm.prefer=tank"), &supported()),
            [Unsupported::PreferredPool("tank".to_owned())]
        );
    }

    #[test]
    fn the_menu() {
        assert_eq!(
            unsupported(&options("zbm.show"), &supported()),
            [Unsupported::Menu("zbm.show")]
        );
        let no_bootfs = Facts {
            bootfs: None,
            ..supported()
        };
        assert_eq!(
            unsupported(&options(""), &no_bootfs)[0].to_string(),
            "the menu (no default boot environment)"
        );
        let interrupted = Facts {
            countdown_interrupted: true,
            ..supported()
        };
        assert_eq!(
            unsupported(&options(""), &interrupted),
            [Unsupported::Menu("a key was pressed during the countdown")]
        );
    }

    #[test]
    fn encryption_and_luks() {
        let facts = Facts {
            luks_volumes: vec!["/dev/vda2".to_owned(), "/dev/vdb2".to_owned()],
            locked_encryption_root: Some("tank/ROOT".to_owned()),
            load_key_hooks: vec![PathBuf::from("/libexec/hooks/load-key.d/10-key")],
            ..supported()
        };
        let text: Vec<String> = unsupported(&options(""), &facts)
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            text,
            [
                "LUKS volumes: /dev/vda2, /dev/vdb2",
                "the locked encryption root tank/ROOT",
                "load-key hooks: /libexec/hooks/load-key.d/10-key",
            ]
        );
        for (mode, message) in [
            (
                LuksMode::Crypttab,
                "LUKS volumes from /etc/zfsbootmenu/crypttab",
            ),
            (LuksMode::CommandLine, "LUKS volumes from zbm.luks"),
        ] {
            let facts = Facts {
                luks: Some(mode),
                ..supported()
            };
            assert_eq!(unsupported(&options(""), &facts)[0].to_string(), message);
        }
        // Volumes found while LUKS is off do not matter.
        let off = Facts {
            luks: Some(LuksMode::Disabled),
            luks_volumes: vec!["/dev/vda2".to_owned()],
            ..supported()
        };
        assert_eq!(unsupported(&options(""), &off), []);
    }

    #[test]
    fn luks_modes_follow_the_legacy_order() {
        use LuksMode::*;
        assert_eq!(LuksMode::decide(Some("off"), true, false), Unavailable);
        for value in ["off", "No", "NONE", "0", "false", "disable", "Disabled"] {
            assert_eq!(
                LuksMode::decide(Some(value), true, true),
                Disabled,
                "{value}"
            );
        }
        for value in ["", "auto", "On", "yes", "1", "TRUE"] {
            assert_eq!(LuksMode::decide(Some(value), true, true), Auto, "{value}");
        }
        assert_eq!(LuksMode::decide(Some("UUID=x"), true, true), CommandLine);
        assert_eq!(LuksMode::decide(None, true, true), Crypttab);
        assert_eq!(LuksMode::decide(None, false, true), Auto);
    }

    #[test]
    fn facts_from_the_files() {
        let files = FakeFiles::new();
        files.file_with_mode("/libexec/hooks/load-key.d/10-key", "", 0o755);
        files.file_with_mode("/libexec/hooks/load-key.d/20-off", "", 0o644);
        let facts = Facts::from_files(&files, Path::new(hooks::HOOKS), None, true);
        assert_eq!(facts.luks, Some(LuksMode::Auto));
        assert_eq!(
            facts.load_key_hooks,
            [PathBuf::from("/libexec/hooks/load-key.d/10-key")]
        );
        files.file(CRYPTTAB, "zfs0 PARTLABEL=zfs0 none\n");
        let facts = Facts::from_files(&files, Path::new(hooks::HOOKS), None, true);
        assert_eq!(facts.luks, Some(LuksMode::Crypttab));
        let facts = Facts::from_files(&files, Path::new(hooks::HOOKS), Some("off"), true);
        assert_eq!(facts.luks, Some(LuksMode::Disabled));
        assert_eq!(MESSAGE, "not supported by this build of ZFSBootMenu yet");
    }
}
