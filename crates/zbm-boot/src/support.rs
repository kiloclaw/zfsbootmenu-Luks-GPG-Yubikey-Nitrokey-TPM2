//! What the runtime cannot handle yet.
//!
//! The Phase 5 runtime booted only the default boot environment of an
//! unencrypted pool; package A of Phase 6 added every start-up and import
//! option, package B LUKS volumes, and R5 encryption keys and `load-key.d`
//! hooks. Before booting, the runtime lists what it still cannot handle with
//! [`unsupported`] and, if the list is not empty, logs each item and opens
//! the recovery shell (see "The minimal runtime of Phase 5" in
//! `crates/DESIGN.md`). Only the menu is left, in an image without the menu
//! program; R5 adds it to images.

use std::fmt;

use zbm_config::options::ZbmOptions;

/// The message of the recovery shell when something is not supported yet.
pub const MESSAGE: &str = "not supported by this build of ZFSBootMenu yet";

/// What the runtime found out about the machine before booting.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Facts {
    /// The default boot environment (`bootfs`), if one was found.
    pub bootfs: Option<String>,
    /// Whether a key was pressed during the countdown.
    pub countdown_interrupted: bool,
    /// Whether the image has the menu program.
    pub has_menu: bool,
}

/// One thing the runtime cannot handle yet.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Unsupported {
    /// The menu: `zbm.show`, no default boot environment, or a key pressed during
    /// the countdown.
    Menu(&'static str),
}

impl fmt::Display for Unsupported {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Unsupported::Menu(reason) => write!(formatter, "the menu ({reason})"),
        }
    }
}

/// Everything in `options` and `facts` that the runtime cannot handle yet:
/// the menu, when the image has none.
pub fn unsupported(options: &ZbmOptions, facts: &Facts) -> Vec<Unsupported> {
    let mut list = Vec::new();
    if facts.has_menu {
        return list;
    }
    // `[ "${menu_timeout}" -ge 0 ] && [ -n "${BOOTFS}" ]` decides between the
    // countdown and the menu
    if options
        .menu_timeout_seconds()
        .is_none_or(|timeout| timeout < 0)
    {
        list.push(Unsupported::Menu("zbm.show"));
    } else if facts.bootfs.is_none() {
        list.push(Unsupported::Menu("no default boot environment"));
    } else if facts.countdown_interrupted {
        list.push(Unsupported::Menu("a key was pressed during the countdown"));
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbm_config::args::ZbmArgs;

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
        let with_menu = Facts {
            has_menu: true,
            ..no_bootfs
        };
        assert_eq!(unsupported(&options("zbm.show"), &with_menu), []);
    }

    #[test]
    fn the_message() {
        assert_eq!(MESSAGE, "not supported by this build of ZFSBootMenu yet");
    }
}
