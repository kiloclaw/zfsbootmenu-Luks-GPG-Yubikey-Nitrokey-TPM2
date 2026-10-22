//! Looking up arguments of ZFSBootMenu's own command line.
//!
//! The legacy code stores every argument as a file `${BASE}/cmdline.d/<key>`
//! holding its value (`1` for an argument without `=`), so the last occurrence
//! of a key wins, and reads them back with `get_zbm_arg` and `get_zbm_bool`.
//! [`ZbmArgs`] keeps the same key-value pairs in memory, with the values
//! exactly as reading the files back produces them.

use std::collections::BTreeMap;

use zbm_core::shell::{command_substitution, echo, read_first_line, read_lines};
use zbm_log::Message;

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
    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    /// The value of the first of `names` the command line has, as `get_zbm_arg`
    /// returns it.
    ///
    /// `names` lists a preferred name followed by deprecated alternatives; using an
    /// alternative adds a warning to `messages`. An argument without `=` has the
    /// value `1`.
    pub fn lookup(&self, names: &[&str], messages: &mut Vec<Message>) -> Option<String> {
        let preferred = names.first().filter(|name| !name.is_empty())?;
        names.iter().find_map(|&name| {
            let value = self.get(name)?;
            if name != *preferred {
                messages.push(Message::warning(format!(
                    "using deprecated arg '{name}' when '{preferred}' is preferred"
                )));
            }
            Some(value.to_owned())
        })
    }

    /// Whether a boolean option is on, as `get_zbm_bool` decides: when none of
    /// `names` is given, `default` decides. An empty value and `no`, `off` and `0`
    /// in any case mean off; everything else means on.
    pub fn flag(&self, default: &str, names: &[&str], messages: &mut Vec<Message>) -> bool {
        let value = self
            .lookup(names, messages)
            .unwrap_or_else(|| default.to_owned());
        !matches!(value.to_lowercase().as_str(), "" | "no" | "off" | "0")
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
    use zbm_testkit::legacy::{Bash, LOG_STUBS, log_lines};
    use zbm_testkit::{Rng, TempDir};

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

    #[test]
    fn lookup_warns_about_deprecated_names() {
        let args = ZbmArgs::from_cmdline("timeout=3");
        let mut messages = Vec::new();
        assert_eq!(
            args.lookup(&["zbm.timeout", "timeout"], &mut messages),
            Some("3".to_owned())
        );
        assert_eq!(
            messages,
            [Message::warning(
                "using deprecated arg 'timeout' when 'zbm.timeout' is preferred"
            )]
        );
        assert_eq!(args.lookup(&[], &mut messages), None);
        assert_eq!(args.lookup(&["", "timeout"], &mut messages), None);
    }

    #[test]
    fn flags_follow_get_zbm_bool() {
        let args = ZbmArgs::from_cmdline("a b=0 c=OFF d=No e=yes f=");
        let mut messages = Vec::new();
        assert!(args.flag("0", &["a"], &mut messages));
        assert!(!args.flag("1", &["b"], &mut messages));
        assert!(!args.flag("1", &["c"], &mut messages));
        assert!(!args.flag("1", &["d"], &mut messages));
        assert!(args.flag("0", &["e"], &mut messages));
        assert!(!args.flag("1", &["f"], &mut messages));
        assert!(!args.flag("0", &["missing"], &mut messages));
        assert!(args.flag("1", &["missing"], &mut messages));
        assert!(args.flag("yes", &["missing"], &mut messages));
        assert!(!args.flag("", &["missing"], &mut messages));
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
        let mut rng = Rng::from_env(5);
        for _ in 0..40 {
            let cmdline = rng.join(PIECES, 10);
            let base = TempDir::new("zbm-cmdline-dir");
            let file = base.write("cmdline", &cmdline);
            Bash::new()
                .code(LOG_STUBS)
                .source("zfsbootmenu/lib/zfsbootmenu-kcl.sh")
                .code(&format!(
                    "BASE={}; _build_zbm_kcl() {{ kcl_tokenize < {}; }}",
                    base.path().display(),
                    file.display()
                ))
                .call("make_cmdline_dir", &[], b"");
            let mut legacy: Vec<(String, String)> = std::fs::read_dir(base.join("cmdline.d"))
                .unwrap()
                .map(|entry| {
                    let entry = entry.unwrap();
                    let name = entry.file_name().to_string_lossy().into_owned();
                    (name, std::fs::read_to_string(entry.path()).unwrap())
                })
                .collect();
            legacy.sort();
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
        for _ in 0..60 {
            let cmdline = rng.join(PIECES, 10);
            let base = TempDir::new("zbm-cmdline-args");
            let file = base.write("cmdline", &cmdline);
            let lib = Bash::new()
                .code(LOG_STUBS)
                .source("zfsbootmenu/lib/zfsbootmenu-kcl.sh")
                .code(&format!(
                    "BASE={}; _build_zbm_kcl() {{ kcl_tokenize < {}; }}",
                    base.path().display(),
                    file.display()
                ));
            let args = ZbmArgs::from_kcl(&tokenize(&cmdline));
            let lookup: Vec<&str> = (0..1 + rng.below(2)).map(|_| *rng.choose(&names)).collect();

            let legacy = lib.call("get_zbm_arg", &lookup, b"");
            let mut messages = Vec::new();
            let rust = args.lookup(&lookup, &mut messages);
            let expected = legacy
                .status
                .success()
                .then(|| legacy.stdout_text().trim_end_matches('\n').to_owned());
            assert_eq!(rust, expected, "cmdline {cmdline:?}, lookup {lookup:?}");
            let warnings: String = messages
                .iter()
                .map(|message| format!("{}\n", message.stderr_line()))
                .collect();
            assert_eq!(
                warnings,
                log_lines(&legacy.stderr_text()),
                "cmdline {cmdline:?}"
            );

            let default = *rng.choose(&["0", "1", "", "no", "On"]);
            let mut bool_args = vec![default];
            bool_args.extend(&lookup);
            let legacy = lib.call("get_zbm_bool", &bool_args, b"");
            let rust = args.flag(default, &lookup, &mut Vec::new());
            assert_eq!(
                rust,
                legacy.status.success(),
                "cmdline {cmdline:?}, flag {bool_args:?}"
            );
        }
    }
}
