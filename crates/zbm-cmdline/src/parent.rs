//! Expanding `%{parent}` in boot environment command lines.
//!
//! A boot environment's `org.zfsbootmenu:commandline` may contain `%{parent}`,
//! which stands for the command line of its parent dataset, expanded in turn.
//! [`read_kcl_prop`] and [`read_kcl_value`] do what the legacy functions of the
//! same names do, with the property values coming from a [`CommandLineSource`]
//! (normally the `zfs` command; see the `zbm-zfs` crate).

use zbm_core::shell::{command_substitution, echo, read_first_line};
use zbm_log::Message;

/// The property that holds boot environment command lines.
pub const COMMANDLINE_PROPERTY: &str = "org.zfsbootmenu:commandline";

/// Where command line properties come from.
pub trait CommandLineSource {
    /// The value of `org.zfsbootmenu:commandline` on `filesystem` (`-` when it is
    /// not set), or `None` when it cannot be read.
    fn commandline(&self, filesystem: &str) -> Option<String>;

    /// The source of the property when it is inherited (`inherited from
    /// <ancestor>`): empty when it is set locally or not at all, `None` when it
    /// cannot be read.
    fn inherited_source(&self, filesystem: &str) -> Option<String>;
}

/// The command line of `filesystem`, with `%{parent}` expanded recursively, or
/// `None` when it cannot be read.
///
/// An inherited property expands to the expansion at the ancestor that sets it,
/// so `%{parent}` is not applied twice.
pub fn read_kcl_prop(
    source: &dyn CommandLineSource,
    filesystem: &str,
    log: &mut Vec<Message>,
) -> Option<String> {
    if filesystem.is_empty() {
        log.push(Message::error("zfsbe is undefined"));
        return None;
    }
    let Some(args) = source.commandline(filesystem) else {
        log.push(Message::error(format!(
            "unable to read {COMMANDLINE_PROPERTY} on {filesystem}"
        )));
        return None;
    };
    if args == "-" {
        log.push(Message::debug(format!(
            "{COMMANDLINE_PROPERTY} on {filesystem} has no value"
        )));
        return Some(String::new());
    }
    if !args.contains("%{parent}") {
        log.push(Message::debug(format!(
            "no parent reference in {COMMANDLINE_PROPERTY} on {filesystem}"
        )));
        return Some(printed(&args));
    }

    let parent = filesystem
        .rsplit_once('/')
        .map_or(filesystem, |(parent, _)| parent);
    let parent_args = if parent.is_empty() || parent == filesystem {
        String::new()
    } else {
        let parent_args = read_kcl_prop(source, parent, log).unwrap_or_else(|| {
            log.push(Message::warning(format!(
                "failed to invoke read_kcl_prop on parent {parent}"
            )));
            String::new()
        });
        if source
            .inherited_source(filesystem)
            .is_some_and(|inherited| !inherited.is_empty())
        {
            log.push(Message::debug(format!(
                "{COMMANDLINE_PROPERTY} on {filesystem} is inherited, \
                 using parent expansion verbatim"
            )));
            return Some(printed(&parent_args));
        }
        parent_args
    };
    Some(printed(&args.replace("%{parent}", &parent_args)))
}

/// The command line in `kclfile`, with `%{parent}` expanded from the property
/// of `filesystem`; when the file cannot be read, the property itself, as
/// [`read_kcl_prop`] reads it.
pub fn read_kcl_value(
    source: &dyn CommandLineSource,
    filesystem: &str,
    kclfile: &std::path::Path,
    log: &mut Vec<Message>,
) -> Option<String> {
    let contents = zbm_core::fs::is_readable(kclfile)
        .then(|| std::fs::read(kclfile).ok())
        .flatten();
    read_kcl_contents(source, filesystem, kclfile, contents.as_deref(), log)
}

/// [`read_kcl_value`] with the contents of `kclfile` already read: `None` when
/// it was not readable. The runtime reads the file through its files interface.
pub fn read_kcl_contents(
    source: &dyn CommandLineSource,
    filesystem: &str,
    kclfile: &std::path::Path,
    contents: Option<&[u8]>,
    log: &mut Vec<Message>,
) -> Option<String> {
    if filesystem.is_empty() {
        log.push(Message::error("zfsbe is undefined"));
        return None;
    }
    let Some(contents) = contents else {
        return read_kcl_prop(source, filesystem, log);
    };
    let args = read_first_line(&String::from_utf8_lossy(contents));
    if !args.contains("%{parent}") {
        log.push(Message::debug(format!(
            "no parent reference in {} on {filesystem}",
            kclfile.display()
        )));
        return Some(printed(&args));
    }
    let parent_args = read_kcl_prop(source, filesystem, log).unwrap_or_default();
    Some(printed(&args.replace("%{parent}", &parent_args)))
}

/// What a caller gets from `echo "$value"` in a command substitution.
fn printed(value: &str) -> String {
    command_substitution(&echo(value)).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Properties by filesystem: (value, inherited source).
    struct Properties(HashMap<&'static str, (&'static str, &'static str)>);

    impl CommandLineSource for Properties {
        fn commandline(&self, filesystem: &str) -> Option<String> {
            self.0.get(filesystem).map(|(value, _)| (*value).to_owned())
        }

        fn inherited_source(&self, filesystem: &str) -> Option<String> {
            self.0
                .get(filesystem)
                .map(|(_, source)| (*source).to_owned())
        }
    }

    fn properties() -> Properties {
        Properties(HashMap::from([
            ("pool", ("spl_hostid=1 a&b", "")),
            ("pool/ROOT", ("%{parent} quiet", "")),
            (
                "pool/ROOT/be",
                ("%{parent} quiet", "inherited from pool/ROOT"),
            ),
            ("pool/ROOT/own", ("x %{parent} y %{parent}", "")),
            ("pool/unset", ("-", "")),
            ("pool/flag", ("-n", "")),
        ]))
    }

    #[test]
    fn expands_parents_recursively() {
        let source = properties();
        let mut log = Vec::new();
        assert_eq!(
            read_kcl_prop(&source, "pool/ROOT", &mut log).as_deref(),
            Some("spl_hostid=1 a&b quiet")
        );
        assert_eq!(
            read_kcl_prop(&source, "pool/ROOT/own", &mut log).as_deref(),
            Some("x spl_hostid=1 a&b quiet y spl_hostid=1 a&b quiet")
        );
        assert_eq!(
            read_kcl_prop(&source, "pool/unset", &mut log).as_deref(),
            Some("")
        );
        assert_eq!(
            read_kcl_prop(&source, "pool/flag", &mut log).as_deref(),
            Some("")
        );
    }

    #[test]
    fn inherited_properties_are_expanded_once() {
        let mut log = Vec::new();
        assert_eq!(
            read_kcl_prop(&properties(), "pool/ROOT/be", &mut log).as_deref(),
            Some("spl_hostid=1 a&b quiet")
        );
        assert!(log.contains(&Message::debug(
            "org.zfsbootmenu:commandline on pool/ROOT/be is inherited, \
             using parent expansion verbatim"
        )));
    }

    #[test]
    fn failures_are_reported() {
        let mut log = Vec::new();
        assert_eq!(read_kcl_prop(&properties(), "", &mut log), None);
        assert_eq!(read_kcl_prop(&properties(), "missing", &mut log), None);
        assert_eq!(
            log,
            [
                Message::error("zfsbe is undefined"),
                Message::error("unable to read org.zfsbootmenu:commandline on missing"),
            ]
        );
        let source = Properties(HashMap::from([("pool/a", ("%{parent} b", ""))]));
        let mut log = Vec::new();
        assert_eq!(
            read_kcl_prop(&source, "pool/a", &mut log).as_deref(),
            Some(" b")
        );
        assert!(log.contains(&Message::warning(
            "failed to invoke read_kcl_prop on parent pool"
        )));
    }

    #[test]
    fn command_line_files_take_precedence() {
        let dir = zbm_testkit::TempDir::new("zbm-cmdline-parent");
        let file = dir.write("cmdline", "  root=x %{parent} \nignored\n");
        let mut log = Vec::new();
        assert_eq!(
            read_kcl_value(&properties(), "pool/ROOT", &file, &mut log).as_deref(),
            Some("root=x spl_hostid=1 a&b quiet")
        );
        let plain = dir.write("plain", "a b");
        assert_eq!(
            read_kcl_value(&properties(), "pool/ROOT", &plain, &mut log).as_deref(),
            Some("a b")
        );
        assert_eq!(
            read_kcl_value(&properties(), "pool/ROOT", &dir.join("missing"), &mut log).as_deref(),
            Some("spl_hostid=1 a&b quiet")
        );
    }
}
