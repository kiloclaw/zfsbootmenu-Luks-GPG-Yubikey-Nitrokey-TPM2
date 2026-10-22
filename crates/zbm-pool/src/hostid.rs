//! Host IDs: the number in `/etc/hostid` that ZFS records in each pool a host
//! imports, and that decides whether another host may import it.
//!
//! [`HostId`] is the ID as eight or more lowercase hexadecimal digits;
//! [`write_hex`] puts one in `/etc/hostid` and [`apparent_hostid`] says
//! which one ZFS uses now.
//!
//! ```
//! use zbm_pool::hostid::HostId;
//!
//! let id = HostId::from_hex("deadbeef").unwrap();
//! assert_eq!(id.bytes(), [0xef, 0xbe, 0xad, 0xde]);
//! assert_eq!(HostId::from_hex("a").unwrap().as_str(), "0000000a");
//! assert_eq!(HostId::from_hex("zz"), None);
//! ```

use std::fmt;
use std::path::Path;

use zbm_platform::files::Files;
use zbm_state::State;
use zbm_trace::Message;

/// The module parameter that holds the host ID SPL uses.
pub const SPL_HOSTID: &str = "/sys/module/spl/parameters/spl_hostid";

/// The host ID file.
pub const ETC_HOSTID: &str = "/etc/hostid";

/// The host ID ZFSBootMenu uses when none is given.
pub const DEFAULT_HOSTID: &str = "00bab10c";

/// A host ID: at least eight lowercase hexadecimal digits, as
/// `printf "%08x"` writes the number.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct HostId(String);

impl HostId {
    /// The host ID ZFSBootMenu uses when none is given, [`DEFAULT_HOSTID`].
    pub fn default_id() -> HostId {
        HostId(DEFAULT_HOSTID.to_owned())
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

    /// The four bytes of `/etc/hostid`: the first eight digits, in
    /// little-endian order; longer IDs lose their last digits, as they do in
    /// the legacy runtime.
    pub fn bytes(&self) -> [u8; 4] {
        let byte = |start: usize| {
            self.0
                .get(start..start + 2)
                .and_then(|digits| u8::from_str_radix(digits, 16).ok())
                .unwrap_or(0)
        };
        [byte(6), byte(4), byte(2), byte(0)]
    }

    /// Writes the ID to `/etc/hostid`, then sets the SPL module's own host ID
    /// to 0 so that `/etc/hostid` counts.
    ///
    /// # Errors
    ///
    /// When `/etc/hostid` cannot be written; it is logged.
    pub fn write(&self, files: &dyn Files) -> Result<(), crate::PoolError> {
        if files.write(Path::new(ETC_HOSTID), &self.bytes()).is_err() {
            zbm_trace::error!("unable to write {self} to /etc/hostid");
            return Err(crate::PoolError::WriteHostid);
        }
        zbm_trace::debug!("wrote hostid {self} to /etc/hostid");
        let spl = Path::new(SPL_HOSTID);
        if files.exists(spl) && files.write(spl, b"0\n").is_err() {
            zbm_trace::warn!("failed to force spl.spl_hostid=0 for host ID matching");
        }
        Ok(())
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

/// Writes the host ID `value` (hexadecimal text, from the command line or a
/// pool) to `/etc/hostid`. Returns the ID, or `None` after logging why not.
pub fn write_hex(files: &dyn Files, value: &str) -> Option<HostId> {
    let Some(hostid) = HostId::from_hex(value) else {
        zbm_trace::error!("invalid hostid {value}");
        return None;
    };
    hostid.write(files).ok()?;
    Some(hostid)
}

/// The host ID ZFS uses now, as `0x` and eight
/// or more hex digits): from the SPL module parameter if it is not 0, from
/// `/etc/hostid`, or from the host ID that pool import matched. Where it came
/// from goes to `log`.
pub fn apparent_hostid(files: &dyn Files, state: &State, log: &mut Vec<Message>) -> Option<String> {
    if let Ok(text) = files.read_to_string(Path::new(SPL_HOSTID)) {
        let value = text.lines().next().unwrap_or_default().trim();
        if let Some(number) = value.parse::<i64>().ok().filter(|number| *number != 0) {
            log.push(Message::debug(format!(
                "hostid from spl.spl_hostid: {value}"
            )));
            // printf "0x%08x" of the decimal value, negative ones as Bash
            // prints them: in two's complement
            return Some(format!(
                "0x{:08x}",
                u64::from_ne_bytes(number.to_ne_bytes())
            ));
        }
    }
    if let Ok(bytes) = files.read(Path::new(ETC_HOSTID))
        && !bytes.is_empty()
    {
        // od -tx4 -N4 reads the first four bytes as a native (little-endian)
        // word, padding a short file with zeros
        let mut word = [0_u8; 4];
        for (target, byte) in word.iter_mut().zip(&bytes) {
            *target = *byte;
        }
        let hostid = format!("{:08x}", u32::from_le_bytes(word));
        log.push(Message::debug(format!("hostid from /etc/hostid: {hostid}")));
        return Some(format!("0x{hostid}"));
    }
    let matched = state.spl_hostid()?;
    log.push(Message::debug(format!(
        "hostid from {}/spl_hostid: {matched}",
        state.base().display()
    )));
    Some(format!("0x{matched}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use zbm_testkit::fakes::FakeSystem;

    #[test]
    fn host_ids_are_written_little_endian() {
        let fake = FakeSystem::new();
        fake.files.dir("/etc").dir("/zfsbootmenu");
        fake.files.file(SPL_HOSTID, "123\n");
        let id = write_hex(&fake.files, "00bab10c").unwrap();
        assert_eq!(id, HostId::default_id());
        assert_eq!(
            fake.files.contents(ETC_HOSTID).unwrap(),
            [0x0c, 0xb1, 0xba, 0x00]
        );
        assert_eq!(fake.files.text(SPL_HOSTID).as_deref(), Some("0\n"));
        assert_eq!(write_hex(&fake.files, "").unwrap().as_str(), "00000000");
        assert_eq!(write_hex(&fake.files, "zz"), None);
        // Longer values keep their first eight digits, as the legacy slices do.
        let long = write_hex(&fake.files, "1deadbeef").unwrap();
        assert_eq!(long.to_string(), "1deadbeef");
        assert_eq!(
            fake.files.contents(ETC_HOSTID).unwrap(),
            [0xee, 0xdb, 0xea, 0x1d]
        );
        let unwritable = FakeSystem::new();
        assert_eq!(write_hex(&unwritable.files, "1"), None);
    }

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
    fn the_apparent_host_id_comes_from_spl_then_the_file_then_the_match() {
        let fake = FakeSystem::new();
        fake.files.dir("/zfsbootmenu");
        let state = State::new(&fake.files, "/zfsbootmenu");
        let mut log = Vec::new();
        assert_eq!(apparent_hostid(&fake.files, &state, &mut log), None);
        state.set_spl_hostid("deadbeef").unwrap();
        let apparent = |log: &mut Vec<Message>| apparent_hostid(&fake.files, &state, log);
        assert_eq!(apparent(&mut log).as_deref(), Some("0xdeadbeef"));
        fake.files.file(ETC_HOSTID, [0x0c_u8, 0xb1, 0xba, 0x00]);
        assert_eq!(apparent(&mut log).as_deref(), Some("0x00bab10c"));
        fake.files.file(ETC_HOSTID, [0x01_u8]);
        assert_eq!(apparent(&mut log).as_deref(), Some("0x00000001"));
        fake.files.file(SPL_HOSTID, "0\n");
        assert_eq!(apparent(&mut log).as_deref(), Some("0x00000001"));
        fake.files.file(SPL_HOSTID, "3735928559\n");
        assert_eq!(apparent(&mut log).as_deref(), Some("0xdeadbeef"));
        fake.files.file(SPL_HOSTID, "-1\n");
        assert_eq!(apparent(&mut log).as_deref(), Some("0xffffffffffffffff"));
        assert_eq!(log.len(), 6);
    }
}
