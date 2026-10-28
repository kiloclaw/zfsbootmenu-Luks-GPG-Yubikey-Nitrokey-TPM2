//! Looking up options of ZFSBootMenu's own command line.
//!
//! The command line is split into arguments as the kernel splits it
//! ([`KernelCommandLine`]). Start-up stores every argument as a file
//! `${BASE}/cmdline.d/<key>` holding its value (`1` for an argument without
//! `=`), so the last occurrence of a key wins. [`ZbmArgs`] keeps the same
//! key-value pairs in memory; [`CmdlineDir`] reads the files.
//!
//! Both are [`Args`], which look an option up by its specification in the
//! [table of options](crate::spec): by its name, then by its aliases, with a
//! warning for an alias.
//!
//! ```
//! use zbm_config::args::{Args, ZbmArgs};
//! use zbm_config::spec::cmdline::{SET_HOSTID, TIMEOUT};
//!
//! let args = ZbmArgs::from_cmdline("quiet timeout=3");
//! let mut messages = Vec::new();
//! assert_eq!(args.lookup(&TIMEOUT, &mut messages).as_deref(), Some("3"));
//! assert_eq!(messages.len(), 1); // timeout is a deprecated name
//! assert!(args.enabled(&SET_HOSTID, &mut messages)); // on by default
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use zbm_platform::files::Files;
use zbm_trace::Message;
use zbm_types::KernelCommandLine;

use crate::spec::OptionSpec;

/// An option found on the command line: the name it was given under, and
/// its value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Found {
    /// The name, the option's or one of its aliases.
    pub name: &'static str,
    /// The value; `1` for an argument without `=`.
    pub value: String,
}

/// The options of a command line, looked up by their specification.
pub trait Args {
    /// The stored value of the argument `name`.
    ///
    /// Look options up with [`Args::lookup`] or [`Args::enabled`] instead, which
    /// know their names from the table; outside this crate, this method is
    /// disallowed (`clippy.toml`).
    fn raw(&self, name: &str) -> Option<String>;

    /// The option `spec`, under the first of its names the command line has.
    #[allow(
        clippy::disallowed_methods,
        reason = "the lookup by name, for the names of the specification"
    )]
    fn find(&self, spec: &OptionSpec) -> Option<Found> {
        spec.names().find_map(|name| {
            Some(Found {
                name,
                value: self.raw(name)?,
            })
        })
    }

    /// The value of `spec`, as the legacy runtime reads it; finding it under an
    /// alias adds a warning to `messages`.
    fn lookup(&self, spec: &OptionSpec, messages: &mut Vec<Message>) -> Option<String> {
        let found = self.find(spec)?;
        if found.name != spec.name {
            messages.push(deprecated(found.name, spec.name));
        }
        Some(found.value)
    }

    /// Whether the boolean option `spec` is on, as the legacy runtime
    /// decides: when it is not given, its default decides (off when it has
    /// none).
    fn enabled(&self, spec: &OptionSpec, messages: &mut Vec<Message>) -> bool {
        let value = self
            .lookup(spec, messages)
            .unwrap_or_else(|| spec.default.unwrap_or("0").to_owned());
        is_on(&value)
    }
}

/// The warning about a deprecated name.
pub(crate) fn deprecated(name: &str, preferred: &str) -> Message {
    Message::warning(format!(
        "using deprecated arg '{name}' when '{preferred}' is preferred"
    ))
}

/// Whether a boolean value means on: an empty value and `no`, `off` and `0`
/// in any case mean off.
pub(crate) fn is_on(value: &str) -> bool {
    !matches!(value.to_lowercase().as_str(), "" | "no" | "off" | "0")
}

/// The arguments of ZFSBootMenu's command line, by key.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ZbmArgs {
    values: BTreeMap<String, String>,
}

impl ZbmArgs {
    /// The arguments of `cmdline`, by name: the value of each, or `1` for an
    /// argument without one; a later argument replaces an earlier one with
    /// the same name. Names that cannot be file names in `cmdline.d` are
    /// left out, as start-up cannot store them (see [`cmdline_files`]).
    pub fn new(cmdline: &KernelCommandLine) -> ZbmArgs {
        ZbmArgs {
            values: stored(cmdline).collect(),
        }
    }

    /// The arguments of a command line given as text, such as the contents
    /// of `/proc/cmdline` or of `zbm.cmdline`, split as the kernel splits it;
    /// what is not an argument is left out.
    pub fn from_cmdline(cmdline: &str) -> ZbmArgs {
        ZbmArgs::new(&KernelCommandLine::lenient(cmdline).0)
    }

    /// The stored value of `key`, if the command line has it.
    pub(crate) fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }
}

/// The names and values of the arguments of `cmdline` that start-up stores,
/// in order.
fn stored(cmdline: &KernelCommandLine) -> impl Iterator<Item = (String, String)> + '_ {
    cmdline
        .args()
        .iter()
        .filter(|arg| can_be_file_name(arg.name()))
        .map(|arg| (arg.name().to_owned(), arg.value().unwrap_or("1").to_owned()))
}

impl Args for ZbmArgs {
    fn raw(&self, name: &str) -> Option<String> {
        self.get(name).map(str::to_owned)
    }
}

/// The options the start-up stored in `${BASE}/cmdline.d`, one file each,
/// read when they are looked up.
pub struct CmdlineDir<'a> {
    files: &'a dyn Files,
    dir: PathBuf,
}

impl<'a> CmdlineDir<'a> {
    /// The options stored below the state directory `base`.
    pub fn new(files: &'a dyn Files, base: &Path) -> CmdlineDir<'a> {
        CmdlineDir {
            files,
            dir: base.join("cmdline.d"),
        }
    }
}

impl Args for CmdlineDir<'_> {
    /// The first line of the file of `name`, without the blanks around it.
    fn raw(&self, name: &str) -> Option<String> {
        if name.is_empty() || name.contains('/') {
            return None;
        }
        let text = self.files.read_to_string(&self.dir.join(name)).ok()?;
        let line = text.lines().next().unwrap_or_default();
        Some(line.trim_matches([' ', '\t']).to_owned())
    }
}

/// The files written in `${BASE}/cmdline.d` for `cmdline`: each name with
/// its value and a newline (`1` for an argument without `=`), in the order
/// written, so a later one replaces an earlier one with the same name.
pub fn cmdline_files(cmdline: &KernelCommandLine) -> Vec<(String, String)> {
    stored(cmdline)
        .map(|(name, value)| (name, format!("{value}\n")))
        .collect()
}

/// Whether `key` can be stored as a file in `cmdline.d`: names with `/`, the
/// names `.` and `..`, and names longer than a file name may be cannot be
/// written, so those arguments are ignored.
fn can_be_file_name(key: &str) -> bool {
    !key.contains('/') && key != "." && key != ".." && key.len() <= 255
}

#[cfg(test)]
mod tests;
