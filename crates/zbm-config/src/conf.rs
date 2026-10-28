//! Reading `/etc/zfsbootmenu.conf`.
//!
//! The image build writes the first part of the file (`install-helpers.sh`:
//! an include guard, then `export NAME="value"` lines), and the start-up
//! appends the options parsed from the command line
//! ([`crate::options::ZbmOptions::conf`], `zfsbootmenu-preinit.sh`:
//! `export name='value'` lines). Shell code sources the file; every Rust
//! program reads it with [`Conf::parse`] instead, as `crates/DESIGN.md`
//! explains ("Configuration and environment").
//!
//! The parser accepts exactly the shell syntax those writers produce, and gives
//! each variable the value sourcing the file would give it. A line that needs
//! more of the shell (a command, an expansion, a glob) is skipped rather than
//! guessed at, and [`Conf::skipped`] names it: the legacy runtime sources the
//! file with Bash, so one odd line must not cost the whole configuration.

/// Where the configuration is.
pub const PATH: &str = "/etc/zfsbootmenu.conf";

/// The include guard the image build writes at the top of the file.
const GUARD: &str = r#"[ -n "${_ETC_ZFSBOOTMENU_CONF}" ] && return"#;

/// The variable the include guard sets, which is not exported.
const GUARD_SET: &str = "readonly _ETC_ZFSBOOTMENU_CONF=1";

/// The variables of `/etc/zfsbootmenu.conf`, in the order they first appear.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Conf {
    vars: Vec<(String, String)>,
    skipped: Vec<String>,
}

impl Conf {
    /// Reads the text of the file; a line that needs more of the shell than
    /// an assignment is skipped, and [`Conf::skipped`] names it.
    pub fn parse(text: &str) -> Conf {
        let mut conf = Conf::default();
        for (index, line) in text.lines().enumerate() {
            let line = line.trim_matches([' ', '\t']);
            if line.is_empty() || line.starts_with('#') || line == GUARD || line == GUARD_SET {
                continue;
            }
            let assignment = line.strip_prefix("export ").and_then(|rest| {
                let (name, value) = rest.trim_start_matches([' ', '\t']).split_once('=')?;
                Some((name, word(value)?))
            });
            match assignment {
                Some((name, value)) if is_name(name) => conf.set(name, value),
                _ => conf
                    .skipped
                    .push(format!("{PATH} line {}: cannot read '{line}'", index + 1)),
            }
        }
        conf
    }

    /// The lines that were skipped, each with what is wrong with it.
    pub fn skipped(&self) -> &[String] {
        &self.skipped
    }

    /// Sets a variable; a later assignment replaces an earlier one, as in the
    /// shell.
    fn set(&mut self, name: &str, value: String) {
        match self.vars.iter_mut().find(|(existing, _)| existing == name) {
            Some((_, existing)) => *existing = value,
            None => self.vars.push((name.to_owned(), value)),
        }
    }

    /// The value of a variable, if the file sets it.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.vars
            .iter()
            .find(|(existing, _)| existing == name)
            .map(|(_, value)| value.as_str())
    }

    /// Every variable, in the order it first appears.
    pub fn vars(&self) -> &[(String, String)] {
        &self.vars
    }
}

/// Whether `name` is a shell variable name.
fn is_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first == '_' || first.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

/// Characters a word may hold outside quotes without the shell changing them.
fn is_plain(c: char) -> bool {
    c.is_ascii_alphanumeric() || "_./:+,=@%-".contains(c)
}

/// The value of one shell word that ends the line: single-quoted parts,
/// double-quoted parts without expansions, backslash escapes and plain
/// characters, in any sequence. `None` if the word needs more of the shell.
fn word(text: &str) -> Option<String> {
    let mut value = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' => loop {
                match chars.next()? {
                    '\'' => break,
                    c => value.push(c),
                }
            },
            '"' => loop {
                match chars.next()? {
                    '"' => break,
                    '$' | '`' | '\\' => return None,
                    c => value.push(c),
                }
            },
            '\\' => value.push(chars.next()?),
            ' ' | '\t' => {
                // Only blanks may follow the word.
                return chars.all(|c| c == ' ' || c == '\t').then_some(value);
            }
            c if is_plain(c) => value.push(c),
            _ => return None,
        }
    }
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::ZbmOptions;
    use zbm_testkit::legacy::{Bash, path, shell_quote};
    use zbm_testkit::{Rng, TempDir, parity};
    use zbm_types::{
        BootfsPreference, HostId, ImportPolicy, PoolName, PoolRequirement, RetryDelay, Timeout,
    };

    fn parse(text: &str) -> Conf {
        Conf::parse(text)
    }

    #[test]
    fn reads_both_parts_of_the_file() {
        let conf = parse(
            "# Include guard\n\
             [ -n \"${_ETC_ZFSBOOTMENU_CONF}\" ] && return\n\
             readonly _ETC_ZFSBOOTMENU_CONF=1\n\
             export HAS_REFRESH=\"1\"\n\
             export HAS_RAW=\"\"\n\
             export ZBM_RUNTIME=\"legacy\"\n\
             \n\
             # BEGIN additions by zfsbootmenu-preinit.sh\n\
             export BASE='/zfsbootmenu'\n\
             export zbm_wait_for_devices='a'\\''b c'\n\
             export default_hostid=00bab10c\n\
             export control_term='/dev/ttyS0'  \n",
        );
        assert_eq!(conf.get("HAS_REFRESH"), Some("1"));
        assert_eq!(conf.get("HAS_RAW"), Some(""));
        assert_eq!(conf.get("ZBM_RUNTIME"), Some("legacy"));
        assert_eq!(conf.get("BASE"), Some("/zfsbootmenu"));
        assert_eq!(conf.get("zbm_wait_for_devices"), Some("a'b c"));
        assert_eq!(conf.get("default_hostid"), Some("00bab10c"));
        assert_eq!(conf.get("control_term"), Some("/dev/ttyS0"));
        assert_eq!(conf.get("_ETC_ZFSBOOTMENU_CONF"), None);
        assert_eq!(conf.get("missing"), None);
        assert_eq!(conf.vars().len(), 7);
        assert_eq!(conf.vars()[0].0, "HAS_REFRESH");
    }

    #[test]
    fn later_assignments_win() {
        let conf = parse("export loglevel='4'\nexport BASE=/x\nexport loglevel='7'\n");
        assert_eq!(conf.get("loglevel"), Some("7"));
        assert_eq!(conf.vars()[0], ("loglevel".to_owned(), "7".to_owned()));
        assert_eq!(parse(""), Conf::default());
    }

    #[test]
    fn words_combine_quotes_and_escapes() {
        assert_eq!(word("'a'\"b\"\\ c").as_deref(), Some("ab c"));
        assert_eq!(word("''").as_deref(), Some(""));
        assert_eq!(word("").as_deref(), Some(""));
        assert_eq!(word("'x$y `z` \\'").as_deref(), Some("x$y `z` \\"));
        assert_eq!(word("\"it's\"").as_deref(), Some("it's"));
        assert_eq!(word("a=b,c:d").as_deref(), Some("a=b,c:d"));
    }

    #[test]
    fn an_embedded_command_line_keeps_every_character() {
        // What module-setup.sh writes for: quiet a="b c" x=$y it's \ `id`
        let conf = parse("export embedded_kcl='quiet a=\"b c\" x=$y it'\\''s \\ `id`'\n");
        assert_eq!(
            conf.get("embedded_kcl"),
            Some("quiet a=\"b c\" x=$y it's \\ `id`")
        );
        assert!(conf.skipped().is_empty());
    }

    #[test]
    fn rejects_what_needs_more_of_the_shell() {
        for line in [
            "export BASE='/zfsbootmenu",
            "export BASE=\"$HOME\"",
            "export BASE=\"`id`\"",
            "export BASE=\"a\\b\"",
            "export BASE=$HOME",
            "export BASE=*",
            "export BASE=~root",
            "export BASE='a';reboot",
            "export BASE='a' b",
            "export BASE=a\\",
            "export 1BASE=x",
            "export BA-SE=x",
            "export BASE",
            "BASE=x",
            "reboot",
            "readonly OTHER=1",
        ] {
            let conf = Conf::parse(&format!("export OK=1\n{line}\nexport LAST=2\n"));
            assert_eq!(
                conf.skipped(),
                [format!(
                    "/etc/zfsbootmenu.conf line 2: cannot read '{line}'"
                )],
                "{line}"
            );
            // The lines around it count.
            assert_eq!((conf.get("OK"), conf.get("LAST")), (Some("1"), Some("2")));
        }
    }

    /// Values with every character that needs care in the shell.
    fn values(rng: &mut Rng) -> Vec<String> {
        let alphabet = [
            'a', 'Z', '0', '/', ' ', '\'', '"', '\\', '$', '`', ';', '*', '=', '!',
        ];
        (0..14).map(|_| rng.string(&alphabet, 10)).collect()
    }

    /// Options of random values: text where the options keep text, and for
    /// the others a value of their type chosen by the length of the text.
    fn options(values: &[String]) -> ZbmOptions {
        let pick = |index: usize| values[index].len();
        let pools = ["tank", "zroot", "a.b-c_d:e"];
        ZbmOptions {
            loglevel: (4 + pick(0) % 4).to_string().parse().unwrap(),
            spl_hostid: [None, HostId::from_hex("0"), HostId::from_hex("deadbeef")][pick(1) % 3]
                .clone(),
            control_term: values[2].clone(),
            // A policy is one of three names; the others take any text.
            import_policy: [
                ImportPolicy::Hostid,
                ImportPolicy::Force,
                ImportPolicy::Strict,
            ][values[3].len() % 3],
            menu_timeout: [Timeout::Wait, Timeout::Seconds(0), Timeout::Seconds(10)][pick(4) % 3],
            retry_delay: RetryDelay::new(1 + pick(5) as u64).unwrap(),
            lines: None,
            columns: None,
            sort: values[6].clone(),
            hook_root: values[7].clone(),
            set_hostid: values[8].len().is_multiple_of(2),
            kcl_override: None,
            wait_for_devices: values[9].clone(),
            prefer_pool: (pick(10) % 4 > 0).then(|| pools[pick(10) % 3].parse().unwrap()),
            require_pool: [
                PoolRequirement::Optional,
                PoolRequirement::Required,
                PoolRequirement::Only,
            ][pick(11) % 3],
            // Any text: what cannot name a dataset is kept as it is.
            prefer_bootfs: BootfsPreference::from_conf(&values[12]),
        }
    }

    /// The function and the heredoc of `zfsbootmenu-preinit.sh` that write the
    /// options, writing to `$CONF` instead of `/etc/zfsbootmenu.conf`.
    fn legacy_writer() -> String {
        let script =
            std::fs::read_to_string(path("zfsbootmenu/pre-init/zfsbootmenu-preinit.sh")).unwrap();
        let function_start = script.find("zbm_conf_quote() {").unwrap();
        let function_end = function_start + script[function_start..].find("\n}\n").unwrap() + 3;
        let heredoc_start = script
            .find("cat >> \"/etc/zfsbootmenu.conf\" <<EOF")
            .unwrap();
        let heredoc_end = heredoc_start + script[heredoc_start..].find("\nEOF\n").unwrap() + 5;
        format!(
            "{}\n{}",
            &script[function_start..function_end],
            script[heredoc_start..heredoc_end].replace("\"/etc/zfsbootmenu.conf\"", "\"$CONF\"")
        )
    }

    #[test]
    fn the_writer_matches_the_legacy_writer() {
        let mut parity = parity!("the_writer_matches_the_legacy_writer");
        let mut rng = Rng::from_env(11);
        for case in 0..40 {
            let values = values(&mut rng);
            let options = options(&values);
            let set_hostid = if options.set_hostid { "1" } else { "0" };
            let base = &values[13];
            let hostid = options
                .spl_hostid
                .as_ref()
                .map(HostId::to_string)
                .unwrap_or_default();
            let pool = options
                .prefer_pool
                .as_ref()
                .map(PoolName::to_string)
                .unwrap_or_default();
            let legacy: String = parity.legacy(format!("{options:?}, base {base:?}"), || {
                let dir = TempDir::new("zbm-conf");
                let conf = dir.join("zfsbootmenu.conf");
                let output = Bash::new()
                    .env("CONF", &conf.to_string_lossy())
                    .env("BASE", base)
                    .env("spl_hostid", &hostid)
                    .env("import_policy", options.import_policy.as_str())
                    .env("menu_timeout", &options.menu_timeout.to_string())
                    .env("loglevel", &options.loglevel.to_string())
                    .env("zbm_prefer_pool", &pool)
                    .env("zbm_require_pool", options.require_pool.as_str())
                    .env("zbm_prefer_bootfs", &options.prefer_bootfs.to_string())
                    .env("zbm_sort", &options.sort)
                    .env("zbm_set_hostid", set_hostid)
                    .env("zbm_retry_delay", &options.retry_delay.to_string())
                    .env("zbm_hook_root", &options.hook_root)
                    .env("zbm_wait_for_devices", &options.wait_for_devices)
                    .env("control_term", &options.control_term)
                    .code(&legacy_writer())
                    .call("true", &[], b"");
                assert!(output.status.is_success(), "{}", output.stderr_text());
                std::fs::read_to_string(&conf).unwrap()
            });
            assert_eq!(options.conf(base), legacy, "case {case}: {values:?}");
        }
    }

    /// Sources `file` in Bash and prints each exported name and value.
    fn sourced(file: &std::path::Path) -> Vec<(String, String)> {
        let output = Bash::new()
            .code(&format!(
                "source {} || exit 1\n\
                 for name in $(compgen -e); do printf '%s\\0%s\\0' \"$name\" \"${{!name}}\"; done",
                shell_quote(&file.to_string_lossy())
            ))
            .call("true", &[], b"");
        assert!(output.status.is_success(), "{}", output.stderr_text());
        let mut fields = output.stdout.split(|byte| *byte == 0);
        let mut vars = Vec::new();
        while let (Some(name), Some(value)) = (fields.next(), fields.next()) {
            vars.push((
                String::from_utf8_lossy(name).into_owned(),
                String::from_utf8_lossy(value).into_owned(),
            ));
        }
        vars
    }

    #[test]
    fn reads_the_file_as_the_shell_sources_it() {
        let mut parity = parity!("reads_the_file_as_the_shell_sources_it");
        // The first part, as the image build writes it
        let header: String = parity.legacy("create_zbm_conf", || {
            let dir = TempDir::new("zbm-conf");
            std::fs::create_dir_all(dir.join("etc")).unwrap();
            let build = Bash::new()
                .source("zfsbootmenu/install-helpers.sh")
                .env("BUILDROOT", &dir.path().to_string_lossy())
                .env("ZBM_BUILDSTYLE", "dracut")
                .env("zfsbootmenu_release_build", "yes")
                .call("create_zbm_conf", &[], b"");
            assert!(build.status.is_success(), "{}", build.stderr_text());
            std::fs::read_to_string(dir.join("etc/zfsbootmenu.conf")).unwrap()
        });
        assert!(header.contains("export ZBM_RUNTIME=\"legacy\""));

        let mut rng = Rng::from_env(12);
        for case in 0..40 {
            let values = values(&mut rng);
            let text = format!("{header}{}", options(&values).conf(&values[13]));
            let conf = Conf::parse(&text);
            let shell: Vec<(String, String)> = parity.legacy(format!("{text:?}"), || {
                let dir = TempDir::new("zbm-conf");
                // Only the variables of the file: the rest is this process's environment.
                sourced(&dir.write("zfsbootmenu.conf", &text))
                    .into_iter()
                    .filter(|(name, _)| conf.vars().iter().any(|(own, _)| own == name))
                    .collect()
            });
            for (name, value) in conf.vars() {
                let exported = shell.iter().find(|(shell_name, _)| shell_name == name);
                assert_eq!(
                    exported.map(|(_, value)| value.as_str()),
                    Some(value.as_str()),
                    "case {case}: {name}"
                );
            }
            assert_eq!(conf.vars().len(), 9 + 15, "case {case}");
        }
    }
}
