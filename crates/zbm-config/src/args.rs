//! Looking up options of ZFSBootMenu's own command line.
//!
//! The legacy code stores every argument as a file `${BASE}/cmdline.d/<key>`
//! holding its value (`1` for an argument without `=`), so the last occurrence
//! of a key wins, and reads them back with `get_zbm_arg` and `get_zbm_bool`.
//! [`ZbmArgs`] keeps the same key-value pairs in memory, with the values
//! exactly as reading the files back produces them; [`CmdlineDir`] reads the
//! files.
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

use zbm_core::shell::{command_substitution, echo, read_first_line, read_lines};
use zbm_platform::files::Files;
use zbm_trace::Message;

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
    /// The arguments of a tokenized command line (see
    /// [`crate::kcl::build_zbm_kcl`]), as `make_cmdline_dir` stores them.
    pub fn from_kcl(text: &str) -> ZbmArgs {
        // `while read -r karg ... done <<< "$( _build_zbm_kcl )"`: the command
        // substitution and the here-string end the text with a single newline, so every
        // argument is read.
        let input = format!("{}\n", command_substitution(text));
        let mut values = BTreeMap::new();
        for arg in read_lines(&input) {
            let key = arg.split_once('=').map_or(arg.as_str(), |(key, _)| key);
            if key.is_empty() || !can_be_file_name(key) {
                continue;
            }
            let value = if key.len() == arg.len() {
                "1"
            } else {
                &arg[key.len() + 1..]
            };
            // `echo "${value}" > file`, read back with `read -r kval < file`.
            values.insert(key.to_owned(), read_first_line(&echo(value)));
        }
        ZbmArgs { values }
    }

    /// The arguments of a command line given as text, such as the contents of
    /// `/proc/cmdline`.
    pub fn from_cmdline(cmdline: &str) -> ZbmArgs {
        ZbmArgs::from_kcl(&crate::kcl::tokenize(cmdline))
    }

    /// The stored value of `key`, if the command line has it.
    pub(crate) fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }
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
    /// The first line of the file of `name`, as `read -r` reads it.
    fn raw(&self, name: &str) -> Option<String> {
        if name.is_empty() || name.contains('/') {
            return None;
        }
        let text = self.files.read_to_string(&self.dir.join(name)).ok()?;
        Some(read_first_line(&text).to_owned())
    }
}

/// The files `make_cmdline_dir` writes in `${BASE}/cmdline.d` for a tokenized
/// command line: each key with the `echo` output of its value (`1` for an
/// argument without `=`), in the order written, so a later one replaces an
/// earlier one with the same key.
pub fn cmdline_files(text: &str) -> Vec<(String, String)> {
    let input = format!("{}\n", command_substitution(text));
    read_lines(&input)
        .into_iter()
        .filter_map(|arg| {
            let key = arg.split_once('=').map_or(arg.as_str(), |(key, _)| key);
            if key.is_empty() || !can_be_file_name(key) {
                return None;
            }
            let contents = if key.len() == arg.len() {
                "1\n".to_owned()
            } else {
                echo(&arg[key.len() + 1..])
            };
            Some((key.to_owned(), contents))
        })
        .collect()
}

/// Whether the legacy code can store `key` as a file in `cmdline.d`: names with
/// `/`, the names `.` and `..`, and names longer than a file name may be cannot
/// be written, so those arguments are ignored.
fn can_be_file_name(key: &str) -> bool {
    !key.contains('/') && key != "." && key != ".." && key.len() <= 255
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kcl::tokenize;
    use crate::spec::cmdline::luks::{LUKS, LUKS_KEY, LUKS_TOKENS};
    use crate::spec::cmdline::{HOOKROOT, RETRY_DELAY, SET_HOSTID, SHOW, TIMEOUT};
    use zbm_testkit::fakes::FakeSystem;
    use zbm_testkit::legacy::{Bash, LOG_STUBS, log_lines};
    use zbm_testkit::{Rng, TempDir, parity};

    /// The library of `get_zbm_arg`, with `_build_zbm_kcl` reading `cmdline`.
    fn kcl_lib(base: &TempDir, cmdline: &str) -> Bash {
        let file = base.write("cmdline", cmdline);
        Bash::new()
            .code(LOG_STUBS)
            .source("zfsbootmenu/lib/zfsbootmenu-kcl.sh")
            .code(&format!(
                "BASE={}; _build_zbm_kcl() {{ kcl_tokenize < {}; }}",
                base.path().display(),
                file.display()
            ))
    }

    #[test]
    fn last_occurrence_wins_and_flags_are_one() {
        let args = ZbmArgs::from_cmdline("quiet zbm.timeout=5 zbm.timeout=7 a=b=c x= -n=y");
        assert_eq!(args.get("quiet"), Some("1"));
        assert_eq!(args.get("zbm.timeout"), Some("7"));
        assert_eq!(args.get("a"), Some("b=c"));
        assert_eq!(args.get("x"), Some(""));
        assert_eq!(args.get("-n"), Some("y"));
        assert_eq!(args.get("missing"), None);
    }

    #[test]
    fn values_change_as_files_change_them() {
        let args = ZbmArgs::from_cmdline("a=-n b=-e c=\"x y\" d/e=1 .=1 ..=1 =z");
        assert_eq!(args.get("a"), Some(""));
        assert_eq!(args.get("b"), Some(""));
        assert_eq!(args.get("c"), Some("\"x y\""));
        assert_eq!(args.get("d/e"), None);
        assert_eq!(args.get("."), None);
        assert_eq!(args.get(".."), None);
        assert_eq!(args.get(""), None);
        // An unterminated quote is still read.
        assert_eq!(ZbmArgs::from_cmdline("k=\"open").get("k"), Some("\"open"));
    }

    /// `get_zbm_arg` with any list of names: the first given, with a warning
    /// for every name but the first.
    fn lookup_names(args: &ZbmArgs, names: &[&str], messages: &mut Vec<Message>) -> Option<String> {
        let preferred = names.first().filter(|name| !name.is_empty())?;
        names.iter().find_map(|&name| {
            let value = args.get(name)?;
            if name != *preferred {
                messages.push(deprecated(name, preferred));
            }
            Some(value.to_owned())
        })
    }

    /// `get_zbm_bool` with any default and list of names.
    fn flag_names(args: &ZbmArgs, default: &str, names: &[&str]) -> bool {
        is_on(&lookup_names(args, names, &mut Vec::new()).unwrap_or_else(|| default.to_owned()))
    }

    #[test]
    fn lookup_warns_about_deprecated_names() {
        let args = ZbmArgs::from_cmdline("timeout=3 zbm.retry_delay=4");
        let mut messages = Vec::new();
        assert_eq!(args.lookup(&TIMEOUT, &mut messages), Some("3".to_owned()));
        assert_eq!(
            messages,
            [Message::warning(
                "using deprecated arg 'timeout' when 'zbm.timeout' is preferred"
            )]
        );
        assert_eq!(
            args.find(&RETRY_DELAY),
            Some(Found {
                name: "zbm.retry_delay",
                value: "4".to_owned()
            })
        );
        assert_eq!(args.lookup(&HOOKROOT, &mut messages), None);
        assert_eq!(messages.len(), 1);
    }

    #[test]
    fn flags_follow_get_zbm_bool() {
        let on = |cmdline: &str, spec: &OptionSpec| {
            ZbmArgs::from_cmdline(cmdline).enabled(spec, &mut Vec::new())
        };
        // zbm.set_hostid is on by default, zbm.show off.
        assert!(on("", &SET_HOSTID));
        assert!(!on("", &SHOW));
        assert!(on("zbm.show", &SHOW));
        for off in ["0", "OFF", "No", ""] {
            assert!(!on(&format!("zbm.set_hostid={off}"), &SET_HOSTID), "{off}");
        }
        assert!(on("zbm.set_hostid=yes", &SET_HOSTID));
        // A specification without a default is off.
        assert!(!on("", &HOOKROOT));
        assert!(is_on("On") && !is_on("off"));
    }

    #[test]
    fn the_cmdline_directory_holds_the_first_line_of_each_value() {
        let fake = FakeSystem::new();
        fake.files
            .dir("/zfsbootmenu/cmdline.d")
            .file("/zfsbootmenu/cmdline.d/zbm.luks", "\tUUID=a, b \nmore\n")
            .file("/zfsbootmenu/cmdline.d/zbm.luks_key", "");
        let dir = CmdlineDir::new(&fake.files, Path::new("/zfsbootmenu"));
        let mut messages = Vec::new();
        assert_eq!(
            dir.lookup(&LUKS, &mut messages).as_deref(),
            Some("UUID=a, b")
        );
        assert_eq!(dir.lookup(&LUKS_KEY, &mut messages).as_deref(), Some(""));
        assert_eq!(dir.lookup(&LUKS_TOKENS, &mut messages), None);
        assert_eq!(dir.raw(""), None);
        assert_eq!(dir.raw("a/b"), None);
        assert!(messages.is_empty());
    }

    const PIECES: &[&str] = &[
        "a",
        "b=1",
        "b=2",
        "c=",
        "=x",
        "d=\"p q\"",
        " ",
        "\"",
        "-n",
        "e=-n",
        "f=-e",
        "g/h=1",
        ".=1",
        "k=a=b",
        "zbm.timeout=4",
        "timeout=9",
        "OFF",
        "x=no",
        "y=Off",
        "z=0",
    ];

    #[test]
    fn cmdline_files_match_make_cmdline_dir() {
        let mut parity = parity!("cmdline_files_match_make_cmdline_dir");
        let mut rng = Rng::from_env(5);
        for _ in 0..40 {
            let cmdline = rng.join(PIECES, 10);
            let legacy: Vec<(String, String)> = parity.legacy(format!("{cmdline:?}"), || {
                let base = TempDir::new("zbm-config-dir");
                kcl_lib(&base, &cmdline).call("make_cmdline_dir", &[], b"");
                let mut files: Vec<(String, String)> = std::fs::read_dir(base.join("cmdline.d"))
                    .unwrap()
                    .map(|entry| {
                        let entry = entry.unwrap();
                        let name = entry.file_name().to_string_lossy().into_owned();
                        (name, std::fs::read_to_string(entry.path()).unwrap())
                    })
                    .collect();
                files.sort();
                files
            });
            let mut rust = std::collections::BTreeMap::new();
            for (key, contents) in cmdline_files(&tokenize(&cmdline)) {
                rust.insert(key, contents);
            }
            let rust: Vec<(String, String)> = rust.into_iter().collect();
            assert_eq!(rust, legacy, "cmdline {cmdline:?}");
        }
    }

    #[test]
    fn lookups_match_get_zbm_arg_and_get_zbm_bool() {
        let mut rng = Rng::from_env(3);
        // Callers only look up fixed names, never "." (which the legacy code finds as
        // the directory itself).
        let names = [
            "a",
            "b",
            "c",
            "d",
            "e",
            "f",
            "g/h",
            "k",
            "zbm.timeout",
            "timeout",
            "x",
            "y",
            "z",
            "missing",
        ];
        let mut parity = parity!("lookups_match_get_zbm_arg_and_get_zbm_bool");
        for _ in 0..60 {
            let cmdline = rng.join(PIECES, 10);
            let lookup: Vec<&str> = (0..1 + rng.below(2)).map(|_| *rng.choose(&names)).collect();
            let default = *rng.choose(&["0", "1", "", "no", "On"]);
            let mut bool_args = vec![default];
            bool_args.extend(&lookup);
            let input = format!("cmdline {cmdline:?}, lookup {lookup:?}, flag {bool_args:?}");
            let (expected, logged, flag): (Option<String>, String, bool) =
                parity.legacy(input, || {
                    let base = TempDir::new("zbm-config-args");
                    let lib = kcl_lib(&base, &cmdline);
                    let arg = lib.call("get_zbm_arg", &lookup, b"");
                    let value = arg
                        .status
                        .is_success()
                        .then(|| arg.stdout_text().trim_end_matches('\n').to_owned());
                    let flag = lib.call("get_zbm_bool", &bool_args, b"");
                    (
                        value,
                        log_lines(&arg.stderr_text()),
                        flag.status.is_success(),
                    )
                });

            let args = ZbmArgs::from_kcl(&tokenize(&cmdline));
            let mut messages = Vec::new();
            let rust = lookup_names(&args, &lookup, &mut messages);
            assert_eq!(rust, expected, "cmdline {cmdline:?}, lookup {lookup:?}");
            let warnings: String = messages
                .iter()
                .map(|message| format!("{}\n", message.stderr_line()))
                .collect();
            assert_eq!(warnings, logged, "cmdline {cmdline:?}");
            let rust = flag_names(&args, default, &lookup);
            assert_eq!(rust, flag, "cmdline {cmdline:?}, flag {bool_args:?}");
        }
    }
}
