//! Version order, as `sort -V` gives it, and kernel versions.
//!
//! The order is that of uutils coreutils, the coreutils of ZFSBootMenu's
//! images, taken from `uucore` ([`version_cmp`]): it compares runs of digits by
//! value and other runs by character, ignoring a file suffix such as `.tar.gz`
//! unless the rest is equal. ZFSBootMenu orders kernels this way, so the
//! newest kernel of a boot environment comes last. GNU coreutils' `sort -V`
//! differs only on names no distribution gives kernels (see "Version order" in
//! `crates/DESIGN.md`).

use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

use crate::error::{ParseError, Reason};
use crate::zfs::MAX_LEN;

/// The order of uutils' `LC_ALL=C sort -V`: uucore's `version_cmp`, then by
/// bytes for versions it finds equal, such as `x-01` and `x-1`.
///
/// ```
/// use std::cmp::Ordering;
/// use zbm_types::version::version_cmp;
///
/// assert_eq!(version_cmp("vmlinuz-6.9", "vmlinuz-6.10.1"), Ordering::Less);
/// assert_eq!(version_cmp("linux-6.1~rc3", "linux-6.1"), Ordering::Less);
/// ```
pub fn version_cmp(a: &str, b: &str) -> Ordering {
    uucore::version_cmp::version_cmp(a.as_bytes(), b.as_bytes())
        .then_with(|| a.as_bytes().cmp(b.as_bytes()))
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
        assert_eq!(version_cmp("a.tar.gz", "a.tar.gz"), Ordering::Equal);
        assert_eq!(version_cmp("..", "."), Ordering::Greater);
        assert_eq!(version_cmp("a-1.gz", "a-2"), Ordering::Less);
    }

    #[test]
    fn kernel_versions_say_why_they_are_invalid() {
        let reason = |text: &str| text.parse::<KernelVersion>().unwrap_err().reason().clone();

        assert_eq!(reason(""), Reason::Empty);
        assert_eq!(reason("6.1/x"), Reason::InvalidCharacter('/'));
        assert_eq!(reason("6.1\t"), Reason::InvalidCharacter('\t'));
        assert_eq!(reason(&"6".repeat(256)), Reason::TooLong { max: 255 });
    }

    #[test]
    fn kernel_versions_compare_in_version_order() {
        let version = |text: &str| text.parse::<KernelVersion>().unwrap();
        let mut versions = [version("6.10.1"), version("6.9"), version("6.1.0-13-amd64")];
        versions.sort();

        let texts: Vec<&str> = versions.iter().map(KernelVersion::as_str).collect();
        assert_eq!(texts, ["6.1.0-13-amd64", "6.9", "6.10.1"]);
        assert!(version("6.9") < version("6.10"));
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
        fn comparing_any_text_never_panics(a in any::<String>(), b in any::<String>()) {
            let _ = version_cmp(&a, &b);
        }
    }
}
