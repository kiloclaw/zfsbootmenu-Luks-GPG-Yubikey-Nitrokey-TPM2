//! The start-up stage `init.d/20-hostid`, which writes the host ID that
//! `zbm-pool` imports pools with.

use std::path::Path;

use zbm_platform::files::Files;
pub use zbm_pool::hostid::{DEFAULT_HOSTID, write_hex};

use crate::commandline::ETC_HOSTID;

/// The stage `init.d/20-hostid`: writes the host ID from the command line
/// (`spl_hostid`), or the default one when that is invalid or when neither the
/// command line nor `/etc/hostid` has one.
pub fn stage(files: &dyn Files, spl_hostid: &str) {
    if !spl_hostid.is_empty() {
        if write_hex(files, spl_hostid).is_some() {
            zbm_trace::info!("writing /etc/hostid from command line: {spl_hostid}");
        } else {
            write_hex(files, DEFAULT_HOSTID);
            zbm_trace::info!("defaulting hostid to {DEFAULT_HOSTID}");
        }
    } else if !files.exists(Path::new(ETC_HOSTID)) {
        zbm_trace::info!("no hostid found on kernel command line or /etc/hostid");
        zbm_trace::info!("defaulting hostid to {DEFAULT_HOSTID}");
        write_hex(files, DEFAULT_HOSTID);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commandline::SPL_HOSTID;
    use crate::testing::Redirect;
    use zbm_platform::{Installed, KernelLog};
    use zbm_testkit::fakes::{FakeFiles, FakeSystem};
    use zbm_testkit::legacy::{Bash, LOG_STUBS, path};
    use zbm_testkit::{Rng, TempDir, parity};

    fn log(fake: &FakeSystem) -> Installed {
        KernelLog::new(&fake.files, "7", Path::new("/zfsbootmenu"), 1).install()
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
                        legacy.status.is_success(),
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
            let rust = write_hex(&files, &value);
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
