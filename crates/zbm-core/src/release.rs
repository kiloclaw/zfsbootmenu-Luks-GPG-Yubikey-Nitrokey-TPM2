//! The release information in `/etc/zbm-release`.
//!
//! The file uses the `os-release` format: `KEY=value` lines whose values may be
//! quoted as in the shell. The legacy code sources it with Bash;
//! [`Release::parse`] reads the same assignments without running anything.

/// The assignments of a release file, in file order.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Release {
    fields: Vec<(String, String)>,
}

impl Release {
    /// Parses a release file. Lines that are not assignments, and comments, are
    /// ignored.
    pub fn parse(text: &str) -> Release {
        let fields = text
            .lines()
            .filter_map(|line| {
                let line = line.trim_start();
                if line.starts_with('#') {
                    return None;
                }
                let (key, value) = line.split_once('=')?;
                let valid_key = !key.is_empty()
                    && !key.starts_with(|c: char| c.is_ascii_digit())
                    && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
                valid_key.then(|| (key.to_owned(), unquote(value)))
            })
            .collect();
        Release { fields }
    }

    /// The value of `key`; the last assignment wins, as when the file is sourced.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.fields
            .iter()
            .rev()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }

    /// The `VERSION` field.
    pub fn version(&self) -> Option<&str> {
        self.get("VERSION")
    }

    /// The `PRETTY_NAME` field.
    pub fn pretty_name(&self) -> Option<&str> {
        self.get("PRETTY_NAME")
    }

    /// Whether the version is a development version: one that ends with `dev`,
    /// which the menu shows in red.
    pub fn is_development(&self) -> bool {
        self.version()
            .is_some_and(|version| version.ends_with("dev"))
    }
}

/// Removes shell quoting from a value: double quotes with backslash escapes,
/// single quotes, and unquoted text up to the first unquoted blank.
fn unquote(value: &str) -> String {
    let mut result = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => match chars.next() {
                            Some(next @ ('"' | '\\' | '$' | '`')) => result.push(next),
                            Some('\n') | None => {}
                            Some(next) => {
                                result.push('\\');
                                result.push(next);
                            }
                        },
                        c => result.push(c),
                    }
                }
            }
            '\'' => result.extend(chars.by_ref().take_while(|&c| c != '\'')),
            '\\' => result.extend(chars.next()),
            ' ' | '\t' => break,
            c => result.push(c),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_legacy_release_file() {
        let release = Release::parse(include_str!("../../../legacy/zfsbootmenu/zbm-release"));
        assert_eq!(release.get("NAME"), Some("ZFSBootMenu"));
        assert_eq!(release.pretty_name(), Some("ZFSBootMenu"));
        assert_eq!(release.get("ID_LIKE"), Some("void"));
        assert_eq!(release.version(), Some(crate::version::ZBM_VERSION));
        assert!(release.is_development());
    }

    #[test]
    fn unquotes_like_the_shell() {
        let release = Release::parse(concat!(
            "# comment\n",
            "PLAIN=3.1.0\n",
            "DOUBLE=\"a \\\"b\\\" \\$c \\x\"\n",
            "SINGLE='it is \"raw\"'\n",
            "MIXED=a\"b c\"'d'\n",
            "TRAILING=word ignored\n",
            "  INDENTED=yes\n",
            "1BAD=no\n",
            "BAD-KEY=no\n",
            "PLAIN=3.2.0\n",
            "not an assignment\n",
        ));
        assert_eq!(release.get("PLAIN"), Some("3.2.0"));
        assert_eq!(release.get("DOUBLE"), Some("a \"b\" $c \\x"));
        assert_eq!(release.get("SINGLE"), Some("it is \"raw\""));
        assert_eq!(release.get("MIXED"), Some("ab cd"));
        assert_eq!(release.get("TRAILING"), Some("word"));
        assert_eq!(release.get("INDENTED"), Some("yes"));
        assert_eq!(release.get("1BAD"), None);
        assert_eq!(release.get("BAD-KEY"), None);
    }

    #[test]
    fn release_versions_are_development_versions_only_with_a_dev_suffix() {
        assert!(!Release::parse("VERSION=\"3.1.0\"").is_development());
        assert!(Release::parse("VERSION=\"3.2.0-dev\"").is_development());
        assert!(!Release::parse("NAME=x").is_development());
    }
}
