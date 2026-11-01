//! File name patterns, as `generate-zbm` matches them: in the last part of a
//! path alone, `*`, `?` and `[...]` (with `!` or `^` for "not" and `a-z` for a
//! range), case-sensitively, as the file system names files (ADR 0013). The
//! directory part is taken as it is, spaces and all. Names that start with a
//! dot match only a pattern that does, and names that are not valid UTF-8 are
//! skipped.

/// The paths that `pattern` names, in byte order; a pattern without a
/// wildcard names itself, whether or not the file exists.
pub fn glob(pattern: &str) -> Vec<String> {
    let (dir, name) = match pattern.rsplit_once('/') {
        Some((dir, name)) => (Some(dir), name),
        None => (None, pattern),
    };
    if !name.contains(['*', '?', '[']) {
        return vec![pattern.to_owned()];
    }
    let listing = match dir {
        Some("") => "/",
        Some(dir) => dir,
        None => ".",
    };
    let mut found: Vec<String> = std::fs::read_dir(listing)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|candidate| matches(name, candidate))
        .map(|candidate| match dir {
            Some(dir) => format!("{dir}/{candidate}"),
            None => candidate,
        })
        .collect();
    found.sort();
    found
}

/// Whether `name` matches the pattern `pattern`.
pub fn matches(pattern: &str, name: &str) -> bool {
    if name.starts_with('.') && !pattern.starts_with('.') {
        return false;
    }
    let pattern: Vec<char> = pattern.chars().collect();
    let name: Vec<char> = name.chars().collect();
    matches_from(&pattern, &name)
}

/// Whether `name` matches `pattern`, both as characters.
fn matches_from(pattern: &[char], name: &[char]) -> bool {
    match pattern.split_first() {
        None => name.is_empty(),
        Some(('*', rest)) => (0..=name.len()).any(|skip| matches_from(rest, &name[skip..])),
        Some(('?', rest)) => name
            .split_first()
            .is_some_and(|(_, tail)| matches_from(rest, tail)),
        Some(('[', rest)) => match (bracket(rest), name.split_first()) {
            (Some((set, after)), Some((first, tail))) => {
                set.contains(*first) && matches_from(after, tail)
            }
            // An unclosed bracket is a plain `[`
            (None, Some(('[', tail))) => matches_from(rest, tail),
            _ => false,
        },
        Some((c, rest)) => name
            .split_first()
            .is_some_and(|(first, tail)| first == c && matches_from(rest, tail)),
    }
}

/// The characters a bracket expression takes.
struct Set {
    negated: bool,
    ranges: Vec<(char, char)>,
}

impl Set {
    fn contains(&self, c: char) -> bool {
        let inside = self
            .ranges
            .iter()
            .any(|(low, high)| (*low..=*high).contains(&c));
        inside != self.negated
    }
}

/// The bracket expression after a `[`, and what follows its `]`; `None`
/// when it does not close.
fn bracket(pattern: &[char]) -> Option<(Set, &[char])> {
    let (negated, mut rest) = match pattern.split_first() {
        Some(('!' | '^', rest)) => (true, rest),
        _ => (false, pattern),
    };
    let mut ranges = Vec::new();
    let mut first = true;
    loop {
        let (&c, tail) = rest.split_first()?;
        if c == ']' && !first {
            return Some((Set { negated, ranges }, tail));
        }
        first = false;
        match tail {
            ['-', high, after @ ..] if *high != ']' => {
                ranges.push((c, *high));
                rest = after;
            }
            _ => {
                ranges.push((c, c));
                rest = tail;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use zbm_testkit::TempDir;

    use super::*;

    #[test]
    fn patterns_match_case_sensitively() {
        assert!(matches("vmlinuz*", "vmlinuz-6.1"));
        assert!(!matches("vmlinuz*", "VMLINUZ-6.1"));
        assert!(matches("linux-?.1", "linux-6.1"));
        assert!(matches("vmlinu[xz]-*", "vmlinux-6.1"));
        assert!(!matches("vmlinu[!xz]-*", "vmlinux-6.1"));
        assert!(matches("[a-c]*", "b"));
        assert!(matches("[^a-c]*", "d"));
        assert!(matches("[]]", "]"));
        assert!(matches("[a-]", "-"));
        assert!(matches("a[b", "a[b"));
        assert!(!matches("a[b", "ab"));
        assert!(!matches("[ab", "b"));
        assert!(!matches("*", ".hidden"));
        assert!(matches(".*", ".hidden"));
        assert!(!matches("a?", "a"));
        assert!(!matches("[a]", ""));
    }

    #[test]
    fn globs_list_the_directory_in_byte_order() {
        let dir = TempDir::new("zbm-glob");
        for name in [
            "vmlinuz-6.2",
            "vmlinuz-6.10",
            "Vmlinuz-7",
            ".vmlinuz-x",
            "initrd",
        ] {
            dir.write(name, "");
        }
        let root = dir.path().display().to_string();
        let names = |pattern: &str| -> Vec<String> {
            glob(&format!("{root}/{pattern}"))
                .into_iter()
                .map(|path| path.rsplit('/').next().unwrap_or_default().to_owned())
                .collect()
        };
        assert_eq!(names("vmlinuz*"), ["vmlinuz-6.10", "vmlinuz-6.2"]);
        assert_eq!(names("none*"), Vec::<String>::new());
        assert_eq!(glob(&format!("{root}/plain")), [format!("{root}/plain")]);
        assert_eq!(glob("plain"), ["plain"]);
        assert!(glob("/*").iter().all(|path| path.starts_with('/')));
        assert!(!glob("*").is_empty());
    }
}
