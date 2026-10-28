//! Host IDs: the number in `/etc/hostid` that ZFS records in each pool a
//! host imports, and that decides whether another host may import it.

use std::fmt;
use std::str::FromStr;

use crate::error::{ParseError, Reason};

/// A host ID: at least eight lowercase hexadecimal digits, as
/// `printf "%08x"` writes the number.
///
/// ```
/// use zbm_types::HostId;
///
/// let id: HostId = "deadbeef".parse()?;
/// assert_eq!(id.bytes(), [0xef, 0xbe, 0xad, 0xde]);
/// assert_eq!(HostId::from_hex("a").unwrap().as_str(), "0000000a");
/// assert_eq!(HostId::from_hex("zz"), None);
/// # Ok::<(), zbm_types::ParseError>(())
/// ```
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct HostId(String);

impl HostId {
    /// The digits of the host ID ZFSBootMenu uses when none is given.
    pub const DEFAULT: &str = "00bab10c";

    /// The host ID ZFSBootMenu uses when none is given, [`HostId::DEFAULT`].
    pub fn default_id() -> HostId {
        HostId(HostId::DEFAULT.to_owned())
    }

    /// The host ID `printf "%08x" "0x<value>"` gives, or `None` when `value`
    /// is not a hexadecimal number Bash can read. An empty value is 0.
    pub fn from_hex(value: &str) -> Option<HostId> {
        let digits = if value.is_empty() { "0" } else { value };
        if !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        let number = u64::from_str_radix(digits, 16).ok()?;
        Some(HostId(format!("{number:08x}")))
    }

    /// The host ID in a message of `zpool import` about a pool that another
    /// host used last, such as `Last accessed by host (hostid=deadbeef)`.
    pub fn scrape(text: &str) -> Option<HostId> {
        scrape_hostid(text).and_then(HostId::from_hex)
    }

    /// The digits.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether it is 0: no host ID, which lets ZFS fall back to
    /// `/etc/hostid`.
    pub fn is_zero(&self) -> bool {
        self.0.bytes().all(|digit| digit == b'0')
    }

    /// The four bytes of `/etc/hostid`: the first eight digits, in
    /// little-endian order; longer IDs lose their last digits, as they do
    /// in the legacy runtime.
    pub fn bytes(&self) -> [u8; 4] {
        let byte = |start: usize| {
            self.0
                .get(start..start + 2)
                .and_then(|digits| u8::from_str_radix(digits, 16).ok())
                .unwrap_or(0)
        };
        [byte(6), byte(4), byte(2), byte(0)]
    }
}

impl FromStr for HostId {
    type Err = ParseError;

    fn from_str(text: &str) -> Result<HostId, ParseError> {
        HostId::from_hex(text)
            .ok_or_else(|| ParseError::new("host ID", text, Reason::NotHexadecimal))
    }
}

impl fmt::Display for HostId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// The digits `hostid=([A-Fa-f0-9]{1,8})` captures in `text`: the first
/// `hostid=` followed by a hex digit, and up to eight digits.
pub fn scrape_hostid(text: &str) -> Option<&str> {
    text.match_indices("hostid=").find_map(|(start, needle)| {
        let digits = text.get(start + needle.len()..)?;
        let len = digits
            .bytes()
            .take(8)
            .take_while(u8::is_ascii_hexdigit)
            .count();
        (len > 0).then(|| digits.get(..len)).flatten()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_ids_are_scraped_as_the_legacy_pattern_matches() {
        let text = "Last accessed by host (hostid=deadbeef1) at ...";
        assert_eq!(scrape_hostid(text), Some("deadbeef"));
        assert_eq!(scrape_hostid("hostid=x hostid=A1"), Some("A1"));
        assert_eq!(scrape_hostid("hostid="), None);
        assert_eq!(scrape_hostid("myhostid=0b"), Some("0b"));
        // Found by the fuzz target program_output: text that is not ASCII
        // before the match.
        assert_eq!(scrape_hostid("tan\u{fffd}\tDEGRADED"), None);
        assert_eq!(scrape_hostid("pool «tank» hostid=0c"), Some("0c"));
        assert_eq!(HostId::scrape("(hostid=A1)").unwrap().as_str(), "000000a1");
    }

    #[test]
    fn host_ids_print_as_printf_writes_them() {
        assert_eq!(HostId::from_hex("").unwrap().as_str(), "00000000");
        assert!(HostId::from_hex("0").unwrap().is_zero());
        assert!(!HostId::default_id().is_zero());
        assert_eq!(
            HostId::from_hex("1deadbeef").unwrap().to_string(),
            "1deadbeef"
        );
        assert_eq!(
            HostId::from_hex("1deadbeef").unwrap().bytes(),
            [0xee, 0xdb, 0xea, 0x1d]
        );
        let error = "0xg".parse::<HostId>().unwrap_err();
        assert_eq!(error.reason(), &Reason::NotHexadecimal);
    }
}
