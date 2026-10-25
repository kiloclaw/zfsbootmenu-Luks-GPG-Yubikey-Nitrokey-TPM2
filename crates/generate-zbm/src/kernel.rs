//! Recognising kernel versions from the file name and from the printable
//! strings in the kernel.

/// The end of the run of ASCII digits that starts at `start`, if there is at
/// least one digit.
fn digits_end(text: &[u8], start: usize) -> Option<usize> {
    let count = text
        .get(start..)?
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    (count > 0).then_some(start + count)
}

/// Where the third digit run of `[0-9]+\.[0-9]+\.[0-9]+` starts when the
/// pattern matches at `start`.
fn third_number(text: &[u8], start: usize) -> Option<usize> {
    let first = digits_end(text, start)?;
    if text.get(first) != Some(&b'.') {
        return None;
    }
    let second = digits_end(text, first + 1)?;
    if text.get(second) != Some(&b'.') {
        return None;
    }
    digits_end(text, second + 1).map(|_| second + 1)
}

/// The version in a kernel's file name: `m/-([0-9]+\.[0-9]+\.[0-9]+.*)/` on its
/// base name, which is everything from the first `-` followed by three dotted
/// numbers up to the end of the line.
pub fn version_from_name(name: &str) -> Option<String> {
    let bytes = name.as_bytes();
    (0..bytes.len())
        .filter(|&dash| bytes[dash] == b'-')
        .find(|&dash| third_number(bytes, dash + 1).is_some())
        .map(|dash| {
            let rest = &name[dash + 1..];
            rest.split('\n').next().unwrap_or(rest).to_owned()
        })
}

/// The first version-like string in a line of `strings` output:
/// `/([0-9]+\.[0-9]+\.[0-9]+\S+)/`, three dotted numbers followed by at least
/// one more character that is not white space.
pub fn version_in_line(line: &[u8]) -> Option<String> {
    (0..line.len()).find_map(|start| {
        let third = third_number(line, start)?;
        // `[0-9]+\S+` needs two characters that are not white space; it extends to the
        // next white space.
        let end = line[third..]
            .iter()
            .position(|byte| crate::perl::is_space(*byte))
            .map_or(line.len(), |offset| third + offset);
        (end - third >= 2).then(|| String::from_utf8_lossy(&line[start..end]).into_owned())
    })
}

/// How the versions from the file name and from the kernel's strings
/// combine.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Decision {
    /// Neither gives a version: the kernel is unusable.
    None,
    /// Only the file name gives a version.
    FromName(String),
    /// Only the strings give a version.
    FromStrings(String),
    /// Both agree.
    Agreed(String),
    /// They disagree: the kernel is skipped with a warning.
    Inconsistent {
        /// The version in the file name.
        name: String,
        /// The version in the strings.
        strings: String,
    },
}

/// Picks the version from the strings: the first version-like string, unless a
/// later one equals the version in the file name; the search stops at the first
/// match when the name gives no version.
pub fn decide<'a>(name: Option<&str>, lines: impl IntoIterator<Item = &'a [u8]>) -> Decision {
    let mut from_strings: Option<String> = None;
    for line in lines {
        let Some(found) = version_in_line(line) else {
            continue;
        };
        if from_strings.is_none() {
            from_strings = Some(found.clone());
        }
        match name {
            None => break,
            Some(name) if name == found => {
                from_strings = Some(found);
                break;
            }
            Some(_) => {}
        }
    }
    match (name, from_strings) {
        (None, None) => Decision::None,
        (Some(name), None) => Decision::FromName(name.to_owned()),
        (None, Some(strings)) => Decision::FromStrings(strings),
        (Some(name), Some(strings)) if name == strings => Decision::Agreed(strings),
        (Some(name), Some(strings)) => Decision::Inconsistent {
            name: name.to_owned(),
            strings,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbm_testkit::legacy::perl;
    use zbm_testkit::{Rng, can_run, have_program, parity};

    #[test]
    fn names() {
        assert_eq!(
            version_from_name("vmlinuz-6.1.0-13-amd64").as_deref(),
            Some("6.1.0-13-amd64")
        );
        assert_eq!(
            version_from_name("linux-lts-6.6.1").as_deref(),
            Some("6.6.1")
        );
        assert_eq!(version_from_name("vmlinuz-6.1"), None);
        assert_eq!(version_from_name("vmlinuz"), None);
    }

    #[test]
    fn strings() {
        assert_eq!(
            version_in_line(b"Linux version 6.1.0-13-amd64 (debian)").as_deref(),
            Some("6.1.0-13-amd64")
        );
        assert_eq!(version_in_line(b"x1.2.34 "), Some("1.2.34".to_owned()));
        assert_eq!(version_in_line(b"1.2.3 "), None);
        assert_eq!(version_in_line(b"1.2.3.4"), Some("1.2.3.4".to_owned()));
        let lines: Vec<&[u8]> = vec![b"gcc 12.2.0-14", b"6.1.0 x", b"6.1.0-13 y"];
        assert_eq!(
            decide(Some("6.1.0-13"), lines.iter().copied()),
            Decision::Agreed("6.1.0-13".to_owned())
        );
        assert_eq!(
            decide(None, lines.iter().copied()),
            Decision::FromStrings("12.2.0-14".to_owned())
        );
        assert_eq!(
            decide(Some("9.9.9"), lines.iter().copied()),
            Decision::Inconsistent {
                name: "9.9.9".to_owned(),
                strings: "12.2.0-14".to_owned()
            }
        );
    }

    #[test]
    fn matches_perl_regexes() {
        let mut parity = parity!("matches_perl_regexes");
        if !can_run(
            !parity.mode().needs_legacy() || have_program("perl"),
            "perl",
        ) {
            return;
        }
        let pieces = [
            "1", "12", "0", ".", "-", " ", "\t", "a", "x86", "_", "linux", "vmlinuz", "+", "rc1",
        ];
        let mut rng = Rng::from_env(13);
        let program = "for my $t (@ARGV) { \
                       my $n = $t =~ m/-([0-9]+\\.[0-9]+\\.[0-9]+.*)/ ? $1 : '<none>'; \
                       my $s = $t =~ /([0-9]+\\.[0-9]+\\.[0-9]+\\S+)/ ? $1 : '<none>'; \
                       print \"$n|$s\\n\"; }";
        let texts: Vec<String> = (0..300).map(|_| rng.join(&pieces, 8)).collect();
        let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
        let output: Vec<String> = parity.legacy(format!("{texts:?}"), || {
            let output = perl(program, &refs, b"").stdout_text();
            output.lines().map(str::to_owned).collect()
        });
        assert_eq!(output.len(), texts.len());
        for (text, expected) in texts.iter().zip(&output) {
            let name = version_from_name(text).unwrap_or_else(|| "<none>".to_owned());
            let strings = version_in_line(text.as_bytes()).unwrap_or_else(|| "<none>".to_owned());
            assert_eq!(&format!("{name}|{strings}"), expected, "text {text:?}");
        }
    }
}
