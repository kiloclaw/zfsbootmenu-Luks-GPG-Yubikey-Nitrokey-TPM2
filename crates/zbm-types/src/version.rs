//! Version order, as `sort -V` gives it, and kernel versions.
//!
//! `sort -V` is GNU coreutils' `filevercmp` ([`filevercmp`]): it compares runs
//! of digits by value and other runs by character, ignoring a file suffix such
//! as `.tar.gz` unless the rest is equal. ZFSBootMenu orders kernels this way,
//! so the newest kernel of a boot environment comes last.

use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

use crate::error::{ParseError, Reason};
use crate::zfs::MAX_LEN;

/// The byte of `s` at `pos`, if there is one.
fn at(s: &[u8], pos: usize) -> Option<u8> {
    s.get(pos).copied()
}

/// The length of the prefix of `s` without its file suffix: the longest
/// suffix matching `(\.[A-Za-z~][A-Za-z0-9~]*)*$`.
fn prefix_len(s: &[u8]) -> usize {
    let n = s.len();
    let mut prefix = 0;
    let mut i = 0;
    while i < n {
        i += 1;
        prefix = i;
        while i + 1 < n
            && at(s, i) == Some(b'.')
            && at(s, i + 1).is_some_and(|c| c.is_ascii_alphabetic() || c == b'~')
        {
            i += 2;
            while at(s, i).is_some_and(|c| c.is_ascii_alphanumeric() || c == b'~') {
                i += 1;
            }
        }
    }
    prefix
}

/// The weight of the byte at `pos` for version sorting: the end sorts before
/// everything but `~`, letters before other characters.
fn order(s: &[u8], pos: usize) -> i32 {
    match at(s, pos) {
        None => -1,
        Some(c) if c.is_ascii_digit() => 0,
        Some(c) if c.is_ascii_alphabetic() => i32::from(c),
        Some(b'~') => -2,
        Some(c) => i32::from(c) + 256,
    }
}

/// GNU's `verrevcmp`: compares runs of non-digits by [`order`] and runs of
/// digits by value.
fn verrevcmp(s1: &[u8], s2: &[u8]) -> i32 {
    let digit = |s: &[u8], pos: usize| at(s, pos).is_some_and(|c| c.is_ascii_digit());
    let value = |s: &[u8], pos: usize| at(s, pos).map_or(0, i32::from);
    let (mut p1, mut p2) = (0, 0);
    while p1 < s1.len() || p2 < s2.len() {
        let mut first_diff = 0;
        while (p1 < s1.len() && !digit(s1, p1)) || (p2 < s2.len() && !digit(s2, p2)) {
            let (c1, c2) = (order(s1, p1), order(s2, p2));
            if c1 != c2 {
                return c1 - c2;
            }
            p1 += 1;
            p2 += 1;
        }
        while at(s1, p1) == Some(b'0') {
            p1 += 1;
        }
        while at(s2, p2) == Some(b'0') {
            p2 += 1;
        }
        while digit(s1, p1) && digit(s2, p2) {
            if first_diff == 0 {
                first_diff = value(s1, p1) - value(s2, p2);
            }
            p1 += 1;
            p2 += 1;
        }
        if digit(s1, p1) {
            return 1;
        }
        if digit(s2, p2) {
            return -1;
        }
        if first_diff != 0 {
            return first_diff;
        }
    }
    0
}

/// GNU coreutils' `filevercmp`, which `sort -V` uses.
///
/// ```
/// use std::cmp::Ordering;
/// use zbm_types::version::filevercmp;
///
/// assert_eq!(filevercmp(b"vmlinuz-6.9", b"vmlinuz-6.10.1"), Ordering::Less);
/// assert_eq!(filevercmp(b"linux-6.1~rc3", b"linux-6.1"), Ordering::Less);
/// ```
pub fn filevercmp(a: &[u8], b: &[u8]) -> Ordering {
    let (Some(&a_first), Some(&b_first)) = (a.first(), b.first()) else {
        return a.len().cmp(&b.len());
    };
    // "." sorts first, then "..", then other names that start with ".", then
    // the rest.
    match (a_first == b'.', b_first == b'.') {
        (true, false) => return Ordering::Less,
        (false, true) => return Ordering::Greater,
        (true, true) => {
            for special in [&b"."[..], b".."] {
                match (a == special, b == special) {
                    (true, true) => return Ordering::Equal,
                    (true, false) => return Ordering::Less,
                    (false, true) => return Ordering::Greater,
                    _ => {}
                }
            }
        }
        (false, false) => {}
    }
    let (a_prefix, b_prefix) = (prefix_len(a), prefix_len(b));
    let one_pass = a_prefix == a.len() && b_prefix == b.len();
    let mut result = verrevcmp(
        a.get(..a_prefix).unwrap_or(a),
        b.get(..b_prefix).unwrap_or(b),
    );
    if result == 0 && !one_pass {
        result = verrevcmp(a, b);
    }
    result.cmp(&0)
}

/// The order of `LC_ALL=C sort -V`: [`filevercmp`], then by bytes.
pub fn version_cmp(a: &str, b: &str) -> Ordering {
    filevercmp(a.as_bytes(), b.as_bytes()).then_with(|| a.as_bytes().cmp(b.as_bytes()))
}

/// Sorts as `LC_ALL=C sort -V` does ([`version_cmp`]).
///
/// ```
/// let mut kernels = vec!["vmlinuz-6.10".to_owned(), "vmlinuz-6.9".to_owned()];
/// zbm_types::version::version_sort(&mut kernels);
/// assert_eq!(kernels, ["vmlinuz-6.9", "vmlinuz-6.10"]);
/// ```
pub fn version_sort(items: &mut [String]) {
    items.sort_by(|a, b| version_cmp(a, b));
}

/// The version of a kernel, such as `6.12.9_1` or `6.1.0-13-amd64`, ordered
/// as `sort -V` orders it.
///
/// It is not empty, has at most 255 bytes, and has no `/`, blank or control
/// character, so it can be part of a file name.
///
/// ```
/// use zbm_types::KernelVersion;
///
/// let old: KernelVersion = "6.9.12".parse()?;
/// let new: KernelVersion = "6.10.1".parse()?;
/// assert!(old < new);
/// assert!("6.1 rc".parse::<KernelVersion>().is_err());
/// # Ok::<(), zbm_types::ParseError>(())
/// ```
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct KernelVersion(String);

impl KernelVersion {
    /// The version as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for KernelVersion {
    type Err = ParseError;

    fn from_str(text: &str) -> Result<KernelVersion, ParseError> {
        let error = |reason| ParseError::new("kernel version", text, reason);
        if text.is_empty() {
            return Err(error(Reason::Empty));
        }
        if text.len() > MAX_LEN {
            return Err(error(Reason::TooLong { max: MAX_LEN }));
        }
        if let Some(c) = text
            .chars()
            .find(|c| *c == '/' || c.is_whitespace() || c.is_control())
        {
            return Err(error(Reason::InvalidCharacter(c)));
        }
        Ok(KernelVersion(text.to_owned()))
    }
}

impl Ord for KernelVersion {
    fn cmp(&self, other: &KernelVersion) -> Ordering {
        version_cmp(&self.0, &other.0)
    }
}

impl PartialOrd for KernelVersion {
    fn partial_cmp(&self, other: &KernelVersion) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for KernelVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    fn sorted(items: &[&str]) -> Vec<String> {
        let mut items: Vec<String> = items.iter().map(|item| (*item).to_owned()).collect();
        version_sort(&mut items);
        items
    }

    #[test]
    fn versions_sort_by_their_numbers() {
        assert_eq!(
            sorted(&[
                "vmlinuz-6.10.1",
                "vmlinuz-6.9",
                "vmlinuz-6.1.0-13-amd64",
                "vmlinuz"
            ]),
            [
                "vmlinuz",
                "vmlinuz-6.1.0-13-amd64",
                "vmlinuz-6.9",
                "vmlinuz-6.10.1"
            ]
        );
        assert_eq!(
            sorted(&["linux-6.1.0", "linux-6.1.0~rc3", "linux-6.1.0-rc3"]),
            ["linux-6.1.0~rc3", "linux-6.1.0", "linux-6.1.0-rc3"]
        );
        assert_eq!(
            sorted(&["a", "", ".b", ".", ".."]),
            ["", ".", "..", ".b", "a"]
        );
        assert_eq!(sorted(&["x-01", "x-1"]), ["x-01", "x-1"]);
        assert_eq!(filevercmp(b"a.tar.gz", b"a.tar.gz"), Ordering::Equal);
        assert_eq!(filevercmp(b"a-1.gz", b"a-2"), Ordering::Less);
        assert_eq!(prefix_len(b"name.tar.gz"), 4);
        assert_eq!(prefix_len(b"6.1"), 3);
    }

    #[test]
    fn kernel_versions_say_why_they_are_invalid() {
        let reason = |text: &str| text.parse::<KernelVersion>().unwrap_err().reason().clone();

        assert_eq!(reason(""), Reason::Empty);
        assert_eq!(reason("6.1/x"), Reason::InvalidCharacter('/'));
        assert_eq!(reason("6.1\t"), Reason::InvalidCharacter('\t'));
        assert_eq!(reason(&"6".repeat(256)), Reason::TooLong { max: 255 });
    }

    proptest! {
        #[test]
        fn the_order_is_antisymmetric(a in "[0-9a-z.~_-]{0,12}", b in "[0-9a-z.~_-]{0,12}") {
            prop_assert_eq!(version_cmp(&a, &b), version_cmp(&b, &a).reverse());
            prop_assert_eq!(version_cmp(&a, &b) == Ordering::Equal, a == b);
        }

        #[test]
        fn valid_versions_round_trip(text in "[0-9][0-9a-z._+~-]{0,20}") {
            let version: KernelVersion = text.parse().unwrap();

            prop_assert_eq!(version.to_string(), text);
        }

        #[test]
        fn comparing_any_bytes_never_panics(a in any::<Vec<u8>>(), b in any::<Vec<u8>>()) {
            let _ = filevercmp(&a, &b);
        }
    }
}
