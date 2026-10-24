//! The `zbm.*` options and their defaults, as the boot image reads them at
//! start-up.
//!
//! [`ZbmOptions::parse`] does what `zfsbootmenu-parse-commandline.sh` does: it
//! reads the options from the command line, falls back to defaults for missing
//! and invalid values, and records the same log messages. The values keep the
//! text the legacy script exports to `/etc/zfsbootmenu.conf` (see
//! [`ZbmOptions::conf`]): a valid timeout of `+5` stays `+5`, as the shell
//! keeps it.

mod hostid;
mod read;

use zbm_core::shell::{quote, test_integer};
use zbm_trace::Message;
use zbm_types::ImportPolicy;

pub use self::hostid::parse_spl_hostid;
use self::read::Reader;
use crate::args::ZbmArgs;
use crate::kcl;
use crate::origin::Origins;
use crate::spec::OptionSpec;
use crate::spec::cmdline::{
    COLUMNS, CONSOLE, FORCE_IMPORT, HOOKROOT, IMPORT_POLICY, KCL_OVERRIDE, LINES, LOGLEVEL, PREFER,
    RETRY_DELAY, SET_HOSTID, SHOW, SKIP, SORT_KEY, SPL_HOSTID, TIMEOUT, WAIT_FOR,
};

/// How [`ZbmOptions::value_of`] finds the value of an option.
type Value = fn(&ZbmOptions) -> Option<String>;

/// The options [`ZbmOptions`] keeps, and where.
static VALUES: &[(&OptionSpec, Value)] = &[
    (&LOGLEVEL, |options| Some(options.loglevel.clone())),
    (&SPL_HOSTID, |options| Some(options.spl_hostid.clone())),
    (&CONSOLE, |options| Some(options.control_term.clone())),
    (&IMPORT_POLICY, |options| {
        Some(options.import_policy.to_string())
    }),
    (&FORCE_IMPORT, |options| {
        Some(options.import_policy.to_string())
    }),
    (&TIMEOUT, |options| Some(options.menu_timeout.clone())),
    (&SHOW, |options| Some(options.menu_timeout.clone())),
    (&SKIP, |options| Some(options.menu_timeout.clone())),
    (&RETRY_DELAY, |options| Some(options.retry_delay.clone())),
    (&LINES, |options| options.lines.clone()),
    (&COLUMNS, |options| options.columns.clone()),
    (&SORT_KEY, |options| Some(options.sort.clone())),
    (&HOOKROOT, |options| Some(options.hook_root.clone())),
    (&SET_HOSTID, |options| {
        Some(if options.set_hostid { "1" } else { "0" }.to_owned())
    }),
    (&KCL_OVERRIDE, |options| {
        options.kcl_override.as_deref().map(kcl::assemble)
    }),
    (&WAIT_FOR, |options| Some(options.wait_for_devices.clone())),
    (&PREFER, ZbmOptions::preference),
];

/// The settings ZFSBootMenu takes from its command line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ZbmOptions {
    /// `loglevel`: the kernel log level of ZFSBootMenu's messages; at least 4.
    pub loglevel: String,
    /// `spl.spl_hostid` or `spl_hostid`: the host ID as eight or more hex digits,
    /// `0`, or empty.
    pub spl_hostid: String,
    /// `console`: the device the menu runs on.
    pub control_term: String,
    /// `zbm.import_policy` (or `zbm.force_import`).
    pub import_policy: ImportPolicy,
    /// `zbm.timeout`, `zbm.show` or `zbm.skip`: seconds before booting the default,
    /// `-1` to always show the menu.
    pub menu_timeout: String,
    /// `zbm.retry_delay` or `zbm.import_delay`: seconds between import and device
    /// retries.
    pub retry_delay: String,
    /// `zbm.lines`: the number of terminal lines, when given.
    pub lines: Option<String>,
    /// `zbm.columns`: the number of terminal columns, when given.
    pub columns: Option<String>,
    /// `zbm.sort_key`: the sort keys of the boot environment list, separated by
    /// `;`.
    pub sort: String,
    /// `zbm.hookroot`: a filesystem holding user hooks, or empty.
    pub hook_root: String,
    /// `zbm.set_hostid`: whether ZFSBootMenu replaces `spl_hostid` in boot
    /// environment command lines.
    pub set_hostid: bool,
    /// `zbm.kcl_override`: the tokenized command line that replaces those of all
    /// boot environments, as written to `${BASE}/cmdline`.
    pub kcl_override: Option<String>,
    /// `zbm.wait_for`: devices to wait for, or empty.
    pub wait_for_devices: String,
    /// `zbm.prefer`: the preferred pool, or empty.
    pub prefer_pool: String,
    /// `zbm.prefer` with `!` (`yes`) or `!!` (`only`): whether the preferred pool
    /// is required, or empty.
    pub require_pool: String,
    /// `zbm.prefer` naming a dataset: the boot environment to prefer, or empty.
    pub prefer_bootfs: String,
}

impl ZbmOptions {
    /// The menu timeout in seconds: `None` when it is not an integer.
    pub fn menu_timeout_seconds(&self) -> Option<i64> {
        self.menu_timeout.trim().parse().ok()
    }

    /// Reads the options from the command line and returns them with the messages
    /// the legacy script logs, in order.
    pub fn parse(args: &ZbmArgs) -> (ZbmOptions, Vec<Message>) {
        let (options, _, log) = ZbmOptions::read(args);
        (options, log)
    }

    /// Reads the options from the command line, as [`ZbmOptions::parse`]
    /// does, and also returns where each value came from.
    pub fn read(args: &ZbmArgs) -> (ZbmOptions, Origins, Vec<Message>) {
        let mut reader = Reader::new(args);
        let loglevel = reader.loglevel();
        let spl_hostid = reader.spl_hostid();
        let control_term = reader.control_term();
        let import_policy = reader.import_policy();
        let menu_timeout = reader.menu_timeout();
        let retry_delay = reader.retry_delay();
        let lines = reader.lines();
        let columns = reader.columns();
        let sort = reader.sort();
        let hook_root = reader.hook_root();
        let set_hostid = reader.set_hostid_enabled();
        let kcl_override = reader.kcl_override(set_hostid);
        let wait_for_devices = reader.wait_for();
        let prefer = reader.prefer();
        let options = ZbmOptions {
            loglevel,
            spl_hostid,
            control_term,
            import_policy,
            menu_timeout,
            retry_delay,
            lines,
            columns,
            sort,
            hook_root,
            set_hostid,
            kcl_override,
            wait_for_devices,
            prefer_pool: prefer.pool,
            require_pool: prefer.require,
            prefer_bootfs: prefer.bootfs,
        };
        (options, reader.origins, reader.log)
    }

    /// `zbm.prefer` as given: the boot environment or pool, followed by `!`
    /// or `!!` when the pool is required.
    fn preference(&self) -> Option<String> {
        let target = if self.prefer_bootfs.is_empty() {
            &self.prefer_pool
        } else {
            &self.prefer_bootfs
        };
        let suffix = match self.require_pool.as_str() {
            "yes" => "!",
            "only" => "!!",
            _ => "",
        };
        (!target.is_empty()).then(|| format!("{target}{suffix}"))
    }

    /// The value this start-up keeps for the option `spec`, as text; `None`
    /// for an option it does not keep, or that has no value.
    pub fn value_of(&self, spec: &OptionSpec) -> Option<String> {
        VALUES
            .iter()
            .find(|(known, _)| *known == spec)
            .and_then(|(_, value)| value(self))
    }

    /// The options `/etc/zfsbootmenu.conf` holds, as the start-up wrote them
    /// with [`ZbmOptions::conf`]. The file has no `zbm.lines`, `zbm.columns` or
    /// `zbm.kcl_override` (which goes to `${BASE}/cmdline`), so they are
    /// `None`; a missing variable is empty, as an unset shell variable is.
    pub fn from_conf(conf: &crate::conf::Conf) -> ZbmOptions {
        let get = |name: &str| conf.get(name).unwrap_or_default().to_owned();
        ZbmOptions {
            loglevel: get("loglevel"),
            spl_hostid: get("spl_hostid"),
            control_term: get("control_term"),
            // The start-up writes one of the three names; anything else
            // imports as `strict` does, as the legacy stage reads it.
            import_policy: get("import_policy").parse().unwrap_or(ImportPolicy::Strict),
            menu_timeout: get("menu_timeout"),
            retry_delay: get("zbm_retry_delay"),
            lines: None,
            columns: None,
            sort: get("zbm_sort"),
            hook_root: get("zbm_hook_root"),
            // `[ "${zbm_set_hostid}" -eq 1 ]`
            set_hostid: test_integer(&get("zbm_set_hostid")) == Some(1),
            kcl_override: None,
            wait_for_devices: get("zbm_wait_for_devices"),
            prefer_pool: get("zbm_prefer_pool"),
            require_pool: get("zbm_require_pool"),
            prefer_bootfs: get("zbm_prefer_bootfs"),
        }
    }

    /// The lines `zfsbootmenu-preinit.sh` appends to `/etc/zfsbootmenu.conf`.
    /// Values are written between single quotes, with each single quote in them
    /// written as `'\''`, so that sourcing the file (or
    /// [`crate::conf::Conf::parse`]) gives the values back.
    pub fn conf(&self, base: &str) -> String {
        let set_hostid = if self.set_hostid { "1" } else { "0" };
        let mut conf = String::from("# BEGIN additions by zfsbootmenu-preinit.sh\n");
        for (name, value) in [
            ("BASE", base),
            ("spl_hostid", &self.spl_hostid),
            ("import_policy", self.import_policy.as_str()),
            ("menu_timeout", &self.menu_timeout),
            ("loglevel", &self.loglevel),
            ("zbm_prefer_pool", &self.prefer_pool),
            ("zbm_require_pool", &self.require_pool),
            ("zbm_prefer_bootfs", &self.prefer_bootfs),
        ] {
            conf.push_str(&export(name, value));
        }
        conf.push_str("export default_hostid=00bab10c\n");
        for (name, value) in [
            ("zbm_sort", self.sort.as_str()),
            ("zbm_set_hostid", set_hostid),
            ("zbm_retry_delay", &self.retry_delay),
            ("zbm_hook_root", &self.hook_root),
            ("zbm_wait_for_devices", &self.wait_for_devices),
            ("control_term", &self.control_term),
        ] {
            conf.push_str(&export(name, value));
        }
        conf.push_str("# END additions by zfsbootmenu-preinit.sh\n");
        conf
    }
}

/// The line that exports `name` with `value`, quoted for the shell.
fn export(name: &str, value: &str) -> String {
    ["export ", name, "=", &quote(value), "\n"].concat()
}

#[cfg(test)]
mod tests;
