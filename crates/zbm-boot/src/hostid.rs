//! The host ID: `write_hostid` and the start-up stage `init.d/20-hostid`.

use std::path::Path;

use zbm_system::files::Files;

use crate::commandline::{ETC_HOSTID, SPL_HOSTID};

/// The host ID ZFSBootMenu uses when none is given.
pub const DEFAULT_HOSTID: &str = "00bab10c";

/// The host ID `printf "%08x" "0x<value>"` gives, or `None` when the value is
/// not a hexadecimal number Bash can read. An empty value is 0.
pub fn normalize(value: &str) -> Option<String> {
    let digits = if value.is_empty() { "0" } else { value };
    if !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let number = u64::from_str_radix(digits, 16).ok()?;
    Some(format!("{number:08x}"))
}

/// Writes the host ID `value` to `/etc/hostid` as `write_hostid` does: the
/// first eight hex digits, as four bytes in little-endian order; then sets the
/// SPL module's own host ID to 0, so that `/etc/hostid` counts. Returns the
/// normalized host ID, or `None` after logging the error.
pub fn write_hostid(files: &dyn Files, value: &str) -> Option<String> {
    let Some(hostid) = normalize(value) else {
        zbm_trace::error!("invalid hostid {value}");
        return None;
    };
    let byte = |start: usize| u8::from_str_radix(&hostid[start..start + 2], 16).unwrap_or(0);
    let bytes = [byte(6), byte(4), byte(2), byte(0)];
    if files.write(Path::new(ETC_HOSTID), &bytes).is_err() {
        zbm_trace::error!("unable to write {hostid} to /etc/hostid");
        return None;
    }
    zbm_trace::debug!("wrote hostid {hostid} to /etc/hostid");
    let spl = Path::new(SPL_HOSTID);
    if files.exists(spl) && files.write(spl, b"0\n").is_err() {
        zbm_trace::warn!("failed to force spl.spl_hostid=0 for host ID matching");
    }
    Some(hostid)
}

/// The stage `init.d/20-hostid`: writes the host ID from the command line
/// (`spl_hostid`), or the default one when that is invalid or when neither the
/// command line nor `/etc/hostid` has one.
pub fn stage(files: &dyn Files, spl_hostid: &str) {
    if !spl_hostid.is_empty() {
        if write_hostid(files, spl_hostid).is_some() {
            zbm_trace::info!("writing /etc/hostid from command line: {spl_hostid}");
        } else {
            write_hostid(files, DEFAULT_HOSTID);
            zbm_trace::info!("defaulting hostid to {DEFAULT_HOSTID}");
        }
    } else if !files.exists(Path::new(ETC_HOSTID)) {
        zbm_trace::info!("no hostid found on kernel command line or /etc/hostid");
        zbm_trace::info!("defaulting hostid to {DEFAULT_HOSTID}");
        write_hostid(files, DEFAULT_HOSTID);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Redirect;
    use zbm_system::{Installed, KernelLog};
    use zbm_testkit::fakes::{FakeFiles, FakeSystem};
    use zbm_testkit::legacy::{Bash, LOG_STUBS, path};
    use zbm_testkit::{Rng, TempDir, parity};

    fn log(fake: &FakeSystem) -> Installed {
        KernelLog::new(&fake.files, "7", Path::new("/zfsbootmenu"), 1).install()
    }

    #[test]
    fn host_ids_are_written_little_endian() {
        let fake = FakeSystem::new();
        let _log = log(&fake);
        fake.files.dir("/etc").dir("/zfsbootmenu");
        fake.files.file(SPL_HOSTID, "123\n");
        assert_eq!(
            write_hostid(&fake.files, "00bab10c").as_deref(),
            Some("00bab10c")
        );
        assert_eq!(
            fake.files.contents(ETC_HOSTID).unwrap(),
            [0x0c, 0xb1, 0xba, 0x00]
        );
        assert_eq!(fake.files.text(SPL_HOSTID).as_deref(), Some("0\n"));
        assert_eq!(write_hostid(&fake.files, "a").as_deref(), Some("0000000a"));
        assert_eq!(write_hostid(&fake.files, "").as_deref(), Some("00000000"));
        assert_eq!(write_hostid(&fake.files, "zz"), None);
        assert!(fake.kmsg().contains("<3>ZFSBootMenu: invalid hostid zz"));
        // Longer values keep their first eight digits, as the legacy slices do.
        assert_eq!(normalize("1deadbeef").as_deref(), Some("1deadbeef"));
        write_hostid(&fake.files, "1deadbeef");
        assert_eq!(
            fake.files.contents(ETC_HOSTID).unwrap(),
            [0xee, 0xdb, 0xea, 0x1d]
        );
    }

    #[test]
    fn the_stage_prefers_the_command_line() {
        let fake = FakeSystem::new();
        let _log = log(&fake);
        fake.files.dir("/etc").dir("/zfsbootmenu");
        stage(&fake.files, "");
        assert_eq!(
            fake.files.contents(ETC_HOSTID).unwrap(),
            [0x0c, 0xb1, 0xba, 0x00]
        );
        assert!(
            fake.kmsg()
                .contains("no hostid found on kernel command line or /etc/hostid")
        );
        stage(&fake.files, "deadbeef");
        assert_eq!(
            fake.files.contents(ETC_HOSTID).unwrap(),
            [0xef, 0xbe, 0xad, 0xde]
        );
        assert!(
            fake.kmsg()
                .contains("writing /etc/hostid from command line: deadbeef")
        );
        stage(&fake.files, "nothex");
        assert_eq!(
            fake.files.contents(ETC_HOSTID).unwrap(),
            [0x0c, 0xb1, 0xba, 0x00]
        );
        assert!(fake.kmsg().contains("defaulting hostid to 00bab10c"));

        // An existing /etc/hostid is kept when the command line has none.
        let files = FakeFiles::new();
        files.file(ETC_HOSTID, [1_u8, 2, 3, 4]);
        files.dir("/zfsbootmenu");
        let _log = KernelLog::new(&files, "7", Path::new("/zfsbootmenu"), 1).install();
        stage(&files, "");
        assert_eq!(files.contents(ETC_HOSTID).unwrap(), [1, 2, 3, 4]);
    }

    #[test]
    fn writes_what_the_legacy_function_writes() {
        let mut parity = parity!("writes_what_the_legacy_function_writes");
        let mut rng = Rng::from_env(41);
        for case in 0..40 {
            let value = rng.string(&['0', '1', '9', 'a', 'f', 'B', 'x', 'g'], 10);
            let (success, legacy_bytes, legacy_spl): (bool, Option<Vec<u8>>, String) = parity
                .legacy(format!("{value:?}"), || {
                    let core = std::fs::read_to_string(path("zfsbootmenu/lib/zfsbootmenu-core.sh"))
                        .unwrap();
                    let start = core.find("\nwrite_hostid() {\n").unwrap() + 1;
                    let end = start + core[start..].find("\n}\n").unwrap() + 3;
                    let dir = TempDir::new("zbm-hostid");
                    let etc = dir.join("hostid");
                    let spl = dir.write("spl_hostid", "5\n");
                    let function = core[start..end]
                        .replace(
                            "/sys/module/spl/parameters/spl_hostid",
                            &spl.to_string_lossy(),
                        )
                        .replace("\"/etc/hostid\"", &format!("\"{}\"", etc.display()));
                    let legacy = Bash::new().code(LOG_STUBS).code(&function).call(
                        "write_hostid",
                        &[&value],
                        b"",
                    );
                    (
                        legacy.status.success(),
                        std::fs::read(&etc).ok(),
                        std::fs::read_to_string(&spl).unwrap(),
                    )
                });
            let dir = TempDir::new("zbm-hostid");
            let etc = dir.join("hostid");
            let spl = dir.write("spl_hostid", "5\n");

            // The Rust code writes the same files through a view of the directory.
            let files = Redirect::new(&[(ETC_HOSTID, &etc), (SPL_HOSTID, &spl)]);
            let _log = KernelLog::new(&files, "7", dir.path(), 1)
                .with_device(&dir.join("kmsg"))
                .install();
            let rust = write_hostid(&files, &value);
            assert_eq!(rust.is_some(), success, "case {case}: {value}");
            assert_eq!(
                std::fs::read(&etc).ok(),
                legacy_bytes,
                "case {case}: {value}"
            );
            assert_eq!(
                std::fs::read_to_string(&spl).unwrap(),
                legacy_spl,
                "case {case}"
            );
        }
    }
}
