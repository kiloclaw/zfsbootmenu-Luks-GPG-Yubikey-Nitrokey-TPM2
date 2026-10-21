//! The host ID: `write_hostid` and the start-up stage `init.d/20-hostid`.

use std::path::Path;

use zbm_system::KernelLog;
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
pub fn write_hostid(files: &dyn Files, log: &KernelLog<'_>, value: &str) -> Option<String> {
    let Some(hostid) = normalize(value) else {
        log.error(format!("invalid hostid {value}"));
        return None;
    };
    let byte = |start: usize| u8::from_str_radix(&hostid[start..start + 2], 16).unwrap_or(0);
    let bytes = [byte(6), byte(4), byte(2), byte(0)];
    if files.write(Path::new(ETC_HOSTID), &bytes).is_err() {
        log.error(format!("unable to write {hostid} to /etc/hostid"));
        return None;
    }
    log.debug(format!("wrote hostid {hostid} to /etc/hostid"));
    let spl = Path::new(SPL_HOSTID);
    if files.exists(spl) && files.write(spl, b"0\n").is_err() {
        log.warning("failed to force spl.spl_hostid=0 for host ID matching");
    }
    Some(hostid)
}

/// The stage `init.d/20-hostid`: writes the host ID from the command line
/// (`spl_hostid`), or the default one when that is invalid or when neither the
/// command line nor `/etc/hostid` has one.
pub fn stage(files: &dyn Files, log: &KernelLog<'_>, spl_hostid: &str) {
    if !spl_hostid.is_empty() {
        if write_hostid(files, log, spl_hostid).is_some() {
            log.info(format!(
                "writing /etc/hostid from command line: {spl_hostid}"
            ));
        } else {
            write_hostid(files, log, DEFAULT_HOSTID);
            log.info(format!("defaulting hostid to {DEFAULT_HOSTID}"));
        }
    } else if !files.exists(Path::new(ETC_HOSTID)) {
        log.info("no hostid found on kernel command line or /etc/hostid");
        log.info(format!("defaulting hostid to {DEFAULT_HOSTID}"));
        write_hostid(files, log, DEFAULT_HOSTID);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbm_system::files::RealFiles;
    use zbm_testutil::fakes::{FakeFiles, FakeSystem};
    use zbm_testutil::legacy::{Bash, LOG_STUBS, path};
    use zbm_testutil::{Rng, TempDir};

    fn log(fake: &FakeSystem) -> KernelLog<'_> {
        KernelLog::new(&fake.files, "7", Path::new("/zfsbootmenu"), 1)
    }

    #[test]
    fn host_ids_are_written_little_endian() {
        let fake = FakeSystem::new();
        fake.files.dir("/etc").dir("/zfsbootmenu");
        fake.files.file(SPL_HOSTID, "123\n");
        assert_eq!(
            write_hostid(&fake.files, &log(&fake), "00bab10c").as_deref(),
            Some("00bab10c")
        );
        assert_eq!(
            fake.files.contents(ETC_HOSTID).unwrap(),
            [0x0c, 0xb1, 0xba, 0x00]
        );
        assert_eq!(fake.files.text(SPL_HOSTID).as_deref(), Some("0\n"));
        assert_eq!(
            write_hostid(&fake.files, &log(&fake), "a").as_deref(),
            Some("0000000a")
        );
        assert_eq!(
            write_hostid(&fake.files, &log(&fake), "").as_deref(),
            Some("00000000")
        );
        assert_eq!(write_hostid(&fake.files, &log(&fake), "zz"), None);
        assert!(fake.kmsg().contains("<3>ZFSBootMenu: invalid hostid zz"));
        // Longer values keep their first eight digits, as the legacy slices do.
        assert_eq!(normalize("1deadbeef").as_deref(), Some("1deadbeef"));
        write_hostid(&fake.files, &log(&fake), "1deadbeef");
        assert_eq!(
            fake.files.contents(ETC_HOSTID).unwrap(),
            [0xee, 0xdb, 0xea, 0x1d]
        );
    }

    #[test]
    fn the_stage_prefers_the_command_line() {
        let fake = FakeSystem::new();
        fake.files.dir("/etc").dir("/zfsbootmenu");
        stage(&fake.files, &log(&fake), "");
        assert_eq!(
            fake.files.contents(ETC_HOSTID).unwrap(),
            [0x0c, 0xb1, 0xba, 0x00]
        );
        assert!(
            fake.kmsg()
                .contains("no hostid found on kernel command line or /etc/hostid")
        );
        stage(&fake.files, &log(&fake), "deadbeef");
        assert_eq!(
            fake.files.contents(ETC_HOSTID).unwrap(),
            [0xef, 0xbe, 0xad, 0xde]
        );
        assert!(
            fake.kmsg()
                .contains("writing /etc/hostid from command line: deadbeef")
        );
        stage(&fake.files, &log(&fake), "nothex");
        assert_eq!(
            fake.files.contents(ETC_HOSTID).unwrap(),
            [0x0c, 0xb1, 0xba, 0x00]
        );
        assert!(fake.kmsg().contains("defaulting hostid to 00bab10c"));

        // An existing /etc/hostid is kept when the command line has none.
        let files = FakeFiles::new();
        files.file(ETC_HOSTID, [1_u8, 2, 3, 4]);
        files.dir("/zfsbootmenu");
        let log = KernelLog::new(&files, "7", Path::new("/zfsbootmenu"), 1);
        stage(&files, &log, "");
        assert_eq!(files.contents(ETC_HOSTID).unwrap(), [1, 2, 3, 4]);
    }

    #[test]
    fn writes_what_the_legacy_function_writes() {
        let core = std::fs::read_to_string(path("zfsbootmenu/lib/zfsbootmenu-core.sh")).unwrap();
        let start = core.find("\nwrite_hostid() {\n").unwrap() + 1;
        let end = start + core[start..].find("\n}\n").unwrap() + 3;
        let mut rng = Rng::from_env(41);
        for case in 0..40 {
            let dir = TempDir::new("zbm-hostid");
            let etc = dir.join("hostid");
            let spl = dir.write("spl_hostid", "5\n");
            let function = core[start..end]
                .replace(
                    "/sys/module/spl/parameters/spl_hostid",
                    &spl.to_string_lossy(),
                )
                .replace("\"/etc/hostid\"", &format!("\"{}\"", etc.display()));
            let value = rng.string(&['0', '1', '9', 'a', 'f', 'B', 'x', 'g'], 10);
            let legacy =
                Bash::new()
                    .code(LOG_STUBS)
                    .code(&function)
                    .call("write_hostid", &[&value], b"");
            let legacy_bytes = std::fs::read(&etc).ok();
            let legacy_spl = std::fs::read_to_string(&spl).unwrap();
            let _ = std::fs::remove_file(&etc);
            std::fs::write(&spl, "5\n").unwrap();

            // The Rust code writes the same files through a view of the directory.
            let files = Redirect {
                etc: etc.clone(),
                spl: spl.clone(),
            };
            let log = KernelLog::new(&files, "7", dir.path(), 1).with_device(&dir.join("kmsg"));
            let rust = write_hostid(&files, &log, &value);
            assert_eq!(
                rust.is_some(),
                legacy.status.success(),
                "case {case}: {value}"
            );
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

    /// Real files, with `/etc/hostid` and the SPL parameter moved.
    struct Redirect {
        etc: std::path::PathBuf,
        spl: std::path::PathBuf,
    }

    impl Redirect {
        fn map<'a>(&'a self, path: &'a Path) -> &'a Path {
            if path == Path::new(ETC_HOSTID) {
                &self.etc
            } else if path == Path::new(SPL_HOSTID) {
                &self.spl
            } else {
                path
            }
        }
    }

    impl Files for Redirect {
        fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
            RealFiles.read(self.map(path))
        }
        fn write(&self, path: &Path, contents: &[u8]) -> std::io::Result<()> {
            RealFiles.write(self.map(path), contents)
        }
        fn append(&self, path: &Path, contents: &[u8]) -> std::io::Result<()> {
            RealFiles.append(self.map(path), contents)
        }
        fn create_dir(&self, path: &Path) -> std::io::Result<()> {
            RealFiles.create_dir(self.map(path))
        }
        fn create_dir_all(&self, path: &Path) -> std::io::Result<()> {
            RealFiles.create_dir_all(self.map(path))
        }
        fn remove_file(&self, path: &Path) -> std::io::Result<()> {
            RealFiles.remove_file(self.map(path))
        }
        fn remove_dir(&self, path: &Path) -> std::io::Result<()> {
            RealFiles.remove_dir(self.map(path))
        }
        fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
            RealFiles.rename(self.map(from), self.map(to))
        }
        fn read_dir(&self, path: &Path) -> std::io::Result<Vec<std::ffi::OsString>> {
            RealFiles.read_dir(self.map(path))
        }
        fn metadata(&self, path: &Path) -> std::io::Result<zbm_system::files::Metadata> {
            RealFiles.metadata(self.map(path))
        }
        fn set_mode(&self, path: &Path, mode: u32) -> std::io::Result<()> {
            RealFiles.set_mode(self.map(path), mode)
        }
        fn canonicalize(&self, path: &Path) -> std::io::Result<std::path::PathBuf> {
            RealFiles.canonicalize(self.map(path))
        }
    }
}
