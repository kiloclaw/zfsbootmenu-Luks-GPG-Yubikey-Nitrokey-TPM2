//! What the runtime cannot handle yet.
//!
//! The Phase 5 runtime booted only the default boot environment of an
//! unencrypted pool; package A of Phase 6 added every start-up and import
//! option, and package B LUKS volumes. Before booting, the runtime lists what
//! it still cannot handle with [`unsupported`] and, if the list is not empty,
//! logs each item and opens the recovery shell (see "The minimal runtime of
//! Phase 5" in `crates/DESIGN.md`). Phase 6 shortens the list as its packages
//! arrive.

use std::fmt;
use std::path::{Path, PathBuf};

use zbm_cmdline::options::ZbmOptions;
use zbm_core::shell::test_integer;
use zbm_system::files::Files;

use crate::hooks;
use zbm_types::HookStage;

/// The message of the recovery shell when something is not supported yet.
pub const MESSAGE: &str = "not supported by this build of ZFSBootMenu yet";

/// What the runtime found out about the machine before booting.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Facts {
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
    /// Fills in what the files tell: the hooks below `hooks_root`.
    pub fn from_files(files: &dyn Files, hooks_root: &Path) -> Facts {
        Facts {
            load_key_hooks: hooks::executable_hooks(files, hooks_root, HookStage::LoadKey),
            ..Facts::default()
        }
    }
}

/// One thing the runtime cannot handle yet.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Unsupported {
    /// `load-key.d` hooks.
    LoadKeyHooks(Vec<PathBuf>),
    /// A locked encryption root.
    LockedEncryptionRoot(String),
    /// The menu: `zbm.show`, no default boot environment, or a key pressed during
    /// the countdown.
    Menu(&'static str),
}

impl fmt::Display for Unsupported {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
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
            Unsupported::Menu(reason) => write!(formatter, "the menu ({reason})"),
        }
    }
}

/// Everything in `options` and `facts` that the runtime cannot handle yet: the
/// menu, locked encryption roots and `load-key.d` hooks, in the order the
/// legacy start-up meets them.
pub fn unsupported(options: &ZbmOptions, facts: &Facts) -> Vec<Unsupported> {
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
    use zbm_testkit::fakes::FakeFiles;

    fn options(cmdline: &str) -> ZbmOptions {
        ZbmOptions::parse(&ZbmArgs::from_cmdline(cmdline)).0
    }

    fn supported() -> Facts {
        Facts {
            bootfs: Some("tank/ROOT/void".to_owned()),
            ..Facts::default()
        }
    }

    #[test]
    fn the_default_boot_is_supported() {
        assert_eq!(unsupported(&options(""), &supported()), []);
        assert_eq!(unsupported(&options("zbm.timeout=0"), &supported()), []);
        assert_eq!(unsupported(&options("zbm.skip"), &supported()), []);
    }

    #[test]
    fn start_up_import_and_luks_options_are_supported() {
        let list = unsupported(
            &options(concat!(
                "zbm.wait_for=/dev/sda zbm.hookroot=tank/hooks ",
                "zbm.import_policy=strict zbm.prefer=tank/ROOT/x! zbm.force_import ",
                "zbm.luks=UUID=1234 zbm.luks_key=/k zbm.luks_tokens=0",
            )),
            &supported(),
        );
        assert_eq!(list, []);
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
    fn encryption() {
        let facts = Facts {
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
                "the locked encryption root tank/ROOT",
                "load-key hooks: /libexec/hooks/load-key.d/10-key",
            ]
        );
    }

    #[test]
    fn facts_from_the_files() {
        let files = FakeFiles::new();
        files.file_with_mode("/libexec/hooks/load-key.d/10-key", "", 0o755);
        files.file_with_mode("/libexec/hooks/load-key.d/20-off", "", 0o644);
        let facts = Facts::from_files(&files, Path::new(hooks::HOOKS));
        assert_eq!(
            facts.load_key_hooks,
            [PathBuf::from("/libexec/hooks/load-key.d/10-key")]
        );
        assert_eq!(MESSAGE, "not supported by this build of ZFSBootMenu yet");
    }
}
