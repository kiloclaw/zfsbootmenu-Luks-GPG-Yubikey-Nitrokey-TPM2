//! One configuration from the kernel command line and the crypttab, with
//! the origin of each value.
//!
//! ```
//! use zbm_config::Config;
//! use zbm_config::args::ZbmArgs;
//! use zbm_config::origin::Origin;
//! use zbm_config::spec::cmdline::TIMEOUT;
//!
//! let args = ZbmArgs::from_cmdline("zbm.timeout=5 zbm.luks=off");
//! let (config, _) = Config::read(&args, None, "/etc/zfsbootmenu/crypttab");
//! assert_eq!(config.options.menu_timeout, "5");
//! let described = config.describe();
//! let timeout = described.iter().find(|line| line.spec == &TIMEOUT).unwrap();
//! assert_eq!(timeout.origin, Origin::CommandLine { name: "zbm.timeout" });
//! assert_eq!(timeout.to_string(), "zbm.timeout = 5 (command line)");
//! ```

use std::fmt;

use zbm_trace::Message;

use crate::args::{Args, ZbmArgs};
use crate::crypttab::{self, Mode};
use crate::options::ZbmOptions;
use crate::origin::{Origin, Origins};
use crate::spec::OptionSpec;
use crate::spec::cmdline::luks::LUKS;

/// The configuration of a boot: the `zbm.*` options and the LUKS volumes to
/// unlock, with where each option came from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    /// The options of the command line.
    pub options: ZbmOptions,
    /// Which LUKS volumes to unlock: from `zbm.luks`, the crypttab, or
    /// discovery.
    pub luks: Mode,
    /// Where each option came from.
    pub origins: Origins,
}

/// One line of [`Config::describe`]: an option, its value and its origin.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Described {
    /// The option.
    pub spec: &'static OptionSpec,
    /// Its value, as the start-up keeps it; empty when it has none.
    pub value: String,
    /// Where the value came from.
    pub origin: Origin,
}

impl fmt::Display for Described {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = self.spec.name;
        let value = &self.value;
        match &self.origin {
            Origin::Default => write!(formatter, "{name} = {value} (default)"),
            Origin::CommandLine { name: given } if *given == name => {
                write!(formatter, "{name} = {value} (command line)")
            }
            Origin::CommandLine { name: given } => {
                write!(formatter, "{name} = {value} (command line, as {given})")
            }
            Origin::Invalid {
                name: given,
                value: invalid,
            } => write!(
                formatter,
                "{name} = {value} (default: {given}={invalid} is not valid)"
            ),
        }
    }
}

impl Config {
    /// Reads the options of `args` and the volumes to unlock, from
    /// `zbm.luks` or else the text of the crypttab at `crypttab_path`
    /// (`None` when there is none). Returns the messages of both, in order.
    pub fn read(
        args: &ZbmArgs,
        crypttab: Option<&str>,
        crypttab_path: &str,
    ) -> (Config, Vec<Message>) {
        let (options, mut origins, mut log) = ZbmOptions::read(args);
        let zbm_luks = args.find(&LUKS);
        origins.record(
            &LUKS,
            zbm_luks
                .as_ref()
                .map_or(Origin::Default, |found| Origin::CommandLine {
                    name: found.name,
                }),
        );
        let value = zbm_luks.map(|found| found.value);
        let (luks, messages) = crypttab::load_config(value.as_deref(), crypttab, crypttab_path);
        log.extend(messages);
        let config = Config {
            options,
            luks,
            origins,
        };
        (config, log)
    }

    /// Every option the start-up read, with its value and origin, in the
    /// order it read them.
    pub fn describe(&self) -> Vec<Described> {
        self.origins
            .iter()
            .map(|(spec, origin)| Described {
                spec,
                value: self.value_of(spec),
                origin: origin.clone(),
            })
            .collect()
    }

    /// The value the start-up keeps for `spec`.
    fn value_of(&self, spec: &OptionSpec) -> String {
        if *spec == LUKS {
            return self.luks.name().to_owned();
        }
        self.options.value_of(spec).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::cmdline::{FORCE_IMPORT, IMPORT_POLICY, PREFER, SORT_KEY, TIMEOUT};
    use pretty_assertions::assert_eq;

    fn describe(cmdline: &str) -> Vec<String> {
        let args = ZbmArgs::from_cmdline(cmdline);
        let (config, _) = Config::read(&args, Some("v /dev/sda none\n"), "/crypttab");
        config.describe().iter().map(ToString::to_string).collect()
    }

    #[test]
    fn each_value_says_where_it_came_from() {
        let lines = describe("timeout=3 zbm.sort_key=size zbm.prefer=tank/ROOT/be!");
        let line = |name: &str| {
            lines
                .iter()
                .find(|line| line.starts_with(&format!("{name} = ")))
                .cloned()
                .unwrap_or_default()
        };
        assert_eq!(
            line("zbm.timeout"),
            "zbm.timeout = 3 (command line, as timeout)"
        );
        assert_eq!(
            line("zbm.sort_key"),
            "zbm.sort_key = name;creation;used (default: zbm.sort_key=size is not valid)"
        );
        assert_eq!(
            line("zbm.prefer"),
            "zbm.prefer = tank/ROOT/be! (command line)"
        );
        assert_eq!(line("zbm.retry_delay"), "zbm.retry_delay = 5 (default)");
        assert_eq!(line("zbm.luks"), "zbm.luks = crypttab (default)");
        // zbm.show is not read when zbm.timeout is given.
        assert_eq!(line("zbm.show"), "");
    }

    #[test]
    fn the_luks_mode_follows_zbm_luks() {
        let args = ZbmArgs::from_cmdline("zbm.luks=off");
        let (config, messages) = Config::read(&args, None, "/crypttab");
        assert_eq!(config.luks, Mode::Disabled);
        assert_eq!(config.value_of(&LUKS), "disabled");
        assert!(!messages.is_empty());
        let args = ZbmArgs::from_cmdline("");
        let (config, _) = Config::read(&args, None, "/crypttab");
        assert_eq!(config.luks, Mode::Auto);
    }

    #[test]
    fn values_are_those_the_start_up_keeps() {
        let args = ZbmArgs::from_cmdline("zbm.force_import zbm.prefer=tank!! zbm.sort_key=used");
        let (config, _) = Config::read(&args, None, "/crypttab");
        let value = |spec: &OptionSpec| config.value_of(spec);
        assert_eq!(value(&IMPORT_POLICY), "force");
        assert_eq!(value(&FORCE_IMPORT), "force");
        assert_eq!(value(&PREFER), "tank!!");
        assert_eq!(value(&SORT_KEY), "used;name;creation");
        assert_eq!(value(&TIMEOUT), "10");
        assert_eq!(value(&LUKS), "auto");
    }
}
