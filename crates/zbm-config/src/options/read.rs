//! Reading each `zbm.*` option from the command line, as
//! `zfsbootmenu-parse-commandline.sh` does: its value or default, the
//! messages the legacy script logs, and where the value came from.

use zbm_core::shell::test_integer;
use zbm_trace::Message;
use zbm_types::{
    BootfsPreference, HostId, ImportPolicy, LogLevel, PoolName, PoolRequirement, RetryDelay,
    Timeout,
};

use super::hostid::parse_spl_hostid;
use crate::args::{Args, ZbmArgs, deprecated, is_on};
use crate::kcl;
use crate::origin::{Origin, Origins};
use crate::spec::OptionSpec;
use crate::spec::cmdline::{
    COLUMNS, CONSOLE, FORCE_IMPORT, HOOKROOT, IMPORT_POLICY, KCL_OVERRIDE, LINES, LOGLEVEL, PREFER,
    RETRY_DELAY, SET_HOSTID, SHOW, SKIP, SORT_KEY, SPL_HOSTID, TIMEOUT, WAIT_FOR,
};

/// The sort keys the menu supports, in their default order.
const SORT_KEYS: [&str; 3] = ["name", "creation", "used"];

/// The pool part of a pool or dataset name.
fn pool_of(name: &str) -> &str {
    name.split_once('/').map_or(name, |(pool, _)| pool)
}

/// `zbm.prefer`: the preferred pool, whether it is required, and the
/// preferred boot environment.
pub(super) struct Preference {
    pub(super) pool: Option<PoolName>,
    pub(super) require: PoolRequirement,
    pub(super) bootfs: BootfsPreference,
}

/// Reads options from a command line, logging and recording as it goes.
pub(super) struct Reader<'a> {
    args: &'a ZbmArgs,
    /// The messages, in the order the legacy script logs them.
    pub(super) log: Vec<Message>,
    /// Where each value came from.
    pub(super) origins: Origins,
}

impl<'a> Reader<'a> {
    /// A reader of `args`.
    pub(super) fn new(args: &'a ZbmArgs) -> Reader<'a> {
        Reader {
            args,
            log: Vec::new(),
            origins: Origins::default(),
        }
    }

    fn info(&mut self, text: impl Into<String>) {
        self.log.push(Message::info(text));
    }

    fn warn(&mut self, text: impl Into<String>) {
        self.log.push(Message::warning(text));
    }

    /// The value of `spec`, warning when it is given under an alias.
    fn value(&mut self, spec: &'static OptionSpec) -> Option<String> {
        let Some(found) = self.args.find(spec) else {
            self.origins.record(spec, Origin::Default);
            return None;
        };
        if found.name != spec.name {
            self.log.push(deprecated(found.name, spec.name));
        }
        self.origins
            .record(spec, Origin::CommandLine { name: found.name });
        Some(found.value)
    }

    /// Whether the boolean option `spec` is on.
    fn enabled(&mut self, spec: &'static OptionSpec) -> bool {
        let value = self.value(spec);
        is_on(value.as_deref().unwrap_or(spec.default.unwrap_or("0")))
    }

    /// Records that the value of `spec` was not valid.
    fn invalid(&mut self, spec: &'static OptionSpec, value: &str) {
        if let Some(Origin::CommandLine { name }) = self.origins.of(spec).cloned() {
            let value = value.to_owned();
            self.origins.record(spec, Origin::Invalid { name, value });
        }
    }

    /// `loglevel`: at least 4.
    pub(super) fn loglevel(&mut self) -> LogLevel {
        let Some(text) = self.value(&LOGLEVEL) else {
            return LogLevel::MIN;
        };
        if let Ok(level) = text.parse() {
            self.info(format!("setting log level from command line: {text}"));
            level
        } else {
            self.invalid(&LOGLEVEL, &text);
            LogLevel::MIN
        }
    }

    /// `spl.spl_hostid`: a host ID, 0 among them, or none.
    pub(super) fn spl_hostid(&mut self) -> Option<HostId> {
        let value = self.value(&SPL_HOSTID)?;
        let hostid = parse_spl_hostid(&value).and_then(|digits| HostId::from_hex(&digits));
        if hostid.is_none() {
            self.warn(format!("invalid hostid value {value}, ignoring"));
            self.invalid(&SPL_HOSTID, &value);
        }
        hostid
    }

    /// `console`: the device of the menu.
    pub(super) fn control_term(&mut self) -> String {
        if let Some(console) = self.value(&CONSOLE) {
            let device = console
                .rsplit_once(',')
                .map_or(console.as_str(), |(device, _)| device);
            let term = format!("/dev/{device}");
            self.info(format!("setting controlling terminal to: {term}"));
            term
        } else {
            let term = "/dev/tty1".to_owned();
            self.info(format!("defaulting controlling terminal to: {term}"));
            term
        }
    }

    /// `zbm.import_policy`, or the deprecated `zbm.force_import`.
    pub(super) fn import_policy(&mut self) -> ImportPolicy {
        if let Some(policy) = self.value(&IMPORT_POLICY) {
            match policy.parse() {
                Ok(ImportPolicy::Hostid) => {
                    self.info("setting import_policy to hostid matching");
                    ImportPolicy::Hostid
                }
                Ok(parsed) => {
                    self.info(format!("setting import_policy to {parsed}"));
                    parsed
                }
                Err(_) => {
                    self.warn(format!(
                        "unknown import policy '{policy}', defaulting to hostid"
                    ));
                    self.invalid(&IMPORT_POLICY, &policy);
                    ImportPolicy::Hostid
                }
            }
        } else if self.enabled(&FORCE_IMPORT) {
            self.info("setting import_policy to force");
            ImportPolicy::Force
        } else {
            self.info("defaulting import_policy to hostid");
            ImportPolicy::Hostid
        }
    }

    /// `zbm.timeout`, or `zbm.show` and `zbm.skip`.
    pub(super) fn menu_timeout(&mut self) -> Timeout {
        if let Some(text) = self.value(&TIMEOUT) {
            if let Ok(timeout) = text.parse() {
                self.info(format!("setting menu timeout from command line: {text}"));
                return timeout;
            }
            self.warn(format!(
                "invalid menu timeout: '{text}', defaulting to 10 seconds"
            ));
            self.invalid(&TIMEOUT, &text);
            Timeout::DEFAULT
        } else if self.enabled(&SHOW) {
            self.info("forcing display of menu");
            Timeout::Wait
        } else if self.enabled(&SKIP) {
            self.info("skipping display of menu");
            Timeout::Seconds(0)
        } else {
            self.info("defaulting menu timeout to 10");
            Timeout::DEFAULT
        }
    }

    /// `zbm.retry_delay`: a positive number of seconds, 5 otherwise.
    pub(super) fn retry_delay(&mut self) -> RetryDelay {
        let Some(text) = self.value(&RETRY_DELAY) else {
            return RetryDelay::DEFAULT;
        };
        // `[ "${zbm_retry_delay:-0}" -gt 0 ]`
        if let Ok(delay) = text.parse() {
            self.info(format!("import/waitfor retry delay is {text} seconds"));
            delay
        } else {
            self.invalid(&RETRY_DELAY, &text);
            RetryDelay::DEFAULT
        }
    }

    /// `zbm.lines` and `zbm.columns`: a positive integer, or `default` with
    /// a warning when the value is not one.
    fn positive(&mut self, spec: &'static OptionSpec, default: &str, unit: &str) -> Option<String> {
        let value = self.value(spec)?;
        if test_integer(&value).is_some_and(|number| number > 0) {
            return Some(value);
        }
        self.warn(format!(
            "invalid {}: '{value}', defaulting to {default} {unit}",
            spec.name
        ));
        self.invalid(spec, &value);
        Some(default.to_owned())
    }

    /// `zbm.lines`.
    pub(super) fn lines(&mut self) -> Option<String> {
        self.positive(&LINES, "25", "lines")
    }

    /// `zbm.columns`.
    pub(super) fn columns(&mut self) -> Option<String> {
        self.positive(&COLUMNS, "80", "columns")
    }

    /// `zbm.sort_key`: the sort keys, the given one first.
    pub(super) fn sort(&mut self) -> String {
        let Some(key) = self.value(&SORT_KEY) else {
            let sort = SORT_KEYS.join(";");
            self.info(format!("defaulting sort key order to {sort}"));
            return sort;
        };
        let first = if let Some(valid) = SORT_KEYS.iter().find(|&&valid| valid == key) {
            *valid
        } else {
            self.invalid(&SORT_KEY, &key);
            "name"
        };
        let mut order = vec![first];
        order.extend(SORT_KEYS.iter().filter(|&&other| other != first));
        let sort = order.join(";");
        self.info(format!("setting sort key order to {sort}"));
        sort
    }

    /// `zbm.hookroot`, or empty.
    pub(super) fn hook_root(&mut self) -> String {
        let Some(root) = self.value(&HOOKROOT) else {
            return String::new();
        };
        self.info(format!("setting user hook root to {root}"));
        root
    }

    /// `zbm.set_hostid`.
    pub(super) fn set_hostid_enabled(&mut self) -> bool {
        let set_hostid = self.enabled(&SET_HOSTID);
        self.info(if set_hostid {
            "enabling automatic replacement of spl_hostid"
        } else {
            "disabling automatic replacement of spl_hostid"
        });
        set_hostid
    }

    /// `zbm.kcl_override`: the tokenized command line, without `root` and,
    /// when `set_hostid`, without host IDs.
    pub(super) fn kcl_override(&mut self, set_hostid: bool) -> Option<String> {
        let value = self.value(&KCL_OVERRIDE)?;
        // Remove one leading and one trailing quote, then up-convert single quotes to
        // double quotes.
        let value = value.strip_prefix('"').unwrap_or(&value);
        let value = value.strip_suffix('"').unwrap_or(value).replace('\'', "\"");
        let mut removals = vec!["root"];
        if set_hostid {
            removals.extend(["spl_hostid", "spl.spl_hostid"]);
        }
        let text = kcl::suppress(&kcl::tokenize(&format!("{value}\n")), &removals);
        self.info(format!(
            "overriding all BE KCLs with: '{}'",
            kcl::assemble(&text)
        ));
        Some(text)
    }

    /// `zbm.wait_for`, or empty.
    pub(super) fn wait_for(&mut self) -> String {
        let Some(devices) = self.value(&WAIT_FOR) else {
            return String::new();
        };
        self.info(format!("system will wait for {devices}"));
        devices
    }

    /// `zbm.prefer`: a pool or dataset, with `!` to require the pool and
    /// `!!` to import only it.
    pub(super) fn prefer(&mut self) -> Preference {
        let mut preference = Preference {
            pool: None,
            require: PoolRequirement::Optional,
            bootfs: BootfsPreference::FromPool,
        };
        let Some(prefer) = self.value(&PREFER) else {
            return preference;
        };
        let prefer = if let Some(pool) = prefer.strip_suffix("!!") {
            preference.require = PoolRequirement::Only;
            self.info(format!("will only attempt to import {}", pool_of(pool)));
            pool.to_owned()
        } else if let Some(pool) = prefer.strip_suffix('!') {
            preference.require = PoolRequirement::Required;
            self.info(format!("requiring pool {}", pool_of(pool)));
            pool.to_owned()
        } else {
            prefer
        };
        let pool = pool_of(&prefer);
        // A name that no pool can have prefers none.
        preference.pool = pool.parse().ok();
        if pool != prefer {
            preference.bootfs = BootfsPreference::from_conf(&prefer);
        }
        preference
    }
}
