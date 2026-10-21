//! The mappings ZFSBootMenu opened (`luks-mappings`) and the volumes the user
//! skipped (`luks-skipped`): `luks_record_mapping`, `luks_mark_skipped` and
//! `luks_retry_ignores`.
//!
//! `luks-mappings` has a line `name<TAB>device<TAB>auto` for each mapping,
//! where `auto` is `1` for a volume found by discovery; the teardown hook and
//! `zluks` close what it lists. `luks-skipped` lists a device per line.

use std::path::Path;
use std::time::Duration;

use crate::device::is_open;
use crate::session::Session;

/// A line of `luks-mappings`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Mapping {
    /// The mapping name.
    pub name: String,
    /// The LUKS device.
    pub device: String,
    /// `1` if the volume was found by discovery.
    pub auto: String,
}

impl Mapping {
    /// The line for the file, without its newline.
    pub fn line(&self) -> String {
        format!("{}\t{}\t{}", self.name, self.device, self.auto)
    }
}

/// The mappings in the text of `luks-mappings`, as
/// `while IFS=$'\t' read -r name dev disc` reads them: fields at runs of tabs,
/// the rest of the line in the last one, and a last line without a newline
/// left out. Lines without a name are skipped.
pub fn parse_mappings(text: &str) -> Vec<Mapping> {
    let complete = text.rfind('\n').map_or("", |end| &text[..end]);
    if text.find('\n').is_none() {
        return Vec::new();
    }
    complete
        .split('\n')
        .filter_map(|line| {
            let line = line.trim_matches('\t');
            let (name, rest) = line.split_once('\t').unwrap_or((line, ""));
            let rest = rest.trim_start_matches('\t');
            let (device, auto) = rest.split_once('\t').unwrap_or((rest, ""));
            let auto = auto.trim_start_matches('\t');
            (!name.is_empty()).then(|| Mapping {
                name: name.to_owned(),
                device: device.to_owned(),
                auto: auto.to_owned(),
            })
        })
        .collect()
}

/// The text of `luks-mappings` with `mappings`, as `printf '%s\n'` writes them,
/// or empty, as `: >` leaves the file.
pub fn format_mappings(mappings: &[Mapping]) -> String {
    mappings
        .iter()
        .map(|mapping| format!("{}\n", mapping.line()))
        .collect()
}

impl Session<'_> {
    /// The recorded mappings.
    pub fn read_mappings(&self) -> Option<Vec<Mapping>> {
        let text = self.system().files.read(&self.settings().mappings).ok()?;
        Some(parse_mappings(&String::from_utf8_lossy(&text)))
    }

    /// Replaces the recorded mappings.
    pub fn write_mappings(&self, mappings: &[Mapping]) {
        let path = &self.settings().mappings;
        if let Err(error) = self
            .system()
            .files
            .write(path, format_mappings(mappings).as_bytes())
        {
            self.log()
                .error(format!("unable to write {}: {error}", path.display()));
        }
    }

    /// Records a mapping ZFSBootMenu opened, once (`luks_record_mapping`).
    /// Concurrent sessions (console and SSH) may record the same mapping at
    /// the same time, so a lock (an atomic `mkdir`) is held across the check
    /// and the append; after five seconds the lock counts as stale.
    pub fn record_mapping(&self, name: &str, device: &str, auto: bool) {
        let system = self.system();
        let path = &self.settings().mappings;
        if let Some(parent) = path.parent() {
            let _ = system.files.create_dir_all(parent);
        }
        let mut lock = path.clone().into_os_string();
        lock.push(".lock");
        let lock = Path::new(&lock);
        let mut tries = 0;
        while system.files.create_dir(lock).is_err() {
            tries += 1;
            if tries >= 50 {
                break;
            }
            system.clock.sleep(Duration::from_millis(100));
        }

        let recorded = self.read_mappings_raw().is_some_and(|text| {
            text.split('\n')
                .any(|line| line.starts_with(&format!("{name}\t")))
        });
        if !recorded {
            let mapping = Mapping {
                name: name.to_owned(),
                device: device.to_owned(),
                auto: if auto { "1" } else { "0" }.to_owned(),
            };
            let line = format!("{}\n", mapping.line());
            if let Err(error) = system.files.append(path, line.as_bytes()) {
                self.log()
                    .error(format!("unable to write {}: {error}", path.display()));
            }
        }
        let _ = system.files.remove_dir(lock);
    }

    fn read_mappings_raw(&self) -> Option<String> {
        let bytes = self.system().files.read(&self.settings().mappings).ok()?;
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Whether the user skipped `device` in this boot.
    pub fn is_skipped(&self, device: &str) -> bool {
        let files = self.system().files;
        files.read(&self.settings().skipped).is_ok_and(|text| {
            text.split(|byte| *byte == b'\n')
                .any(|line| line == device.as_bytes())
        })
    }

    /// Records that the user skipped `device`, so that retries leave it alone
    /// (`luks_mark_skipped`).
    pub fn mark_skipped(&self, device: &str) {
        let files = self.system().files;
        let path = &self.settings().skipped;
        if let Some(parent) = path.parent() {
            let _ = files.create_dir_all(parent);
        }
        if !self.is_skipped(device) {
            if let Err(error) = files.append(path, format!("{device}\n").as_bytes()) {
                self.log()
                    .error(format!("unable to write {}: {error}", path.display()));
            }
        }
    }

    /// Whether a retry leaves `device` alone because it is open or skipped
    /// (`luks_retry_ignores`).
    pub fn retry_ignores(&self, device: &str) -> bool {
        is_open(self.system().files, device, &self.settings().open_check) || self.is_skipped(device)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::OpenCheck;
    use crate::session::{Luks, Settings};
    use crate::testing::{NoPrompts, legacy_luks, log};
    use zbm_system::clock::Clock;
    use zbm_system::files::Files;
    use zbm_testutil::TempDir;
    use zbm_testutil::fakes::FakeSystem;
    use zbm_testutil::legacy::shell_quote;

    fn with_session<T>(fake: &FakeSystem, body: impl FnOnce(&Session<'_>) -> T) -> T {
        let mut settings = Settings::new("/zfsbootmenu");
        settings.open_check = OpenCheck::Markers("/fake/holders".into());
        let log = log(fake);
        let luks = Luks {
            system: fake.system(),
            log: &log,
            settings: &settings,
            prompter: &NoPrompts,
        };
        body(&Session::new(luks))
    }

    #[test]
    fn mappings_are_recorded_once() {
        let fake = FakeSystem::new();
        with_session(&fake, |session| {
            assert_eq!(session.read_mappings(), None);
            session.record_mapping("a", "/dev/sda", true);
            session.record_mapping("ab", "/dev/sdb", false);
            session.record_mapping("a", "/dev/sdc", false);
            assert_eq!(
                session.read_mappings().unwrap(),
                [
                    Mapping {
                        name: "a".to_owned(),
                        device: "/dev/sda".to_owned(),
                        auto: "1".to_owned()
                    },
                    Mapping {
                        name: "ab".to_owned(),
                        device: "/dev/sdb".to_owned(),
                        auto: "0".to_owned()
                    },
                ]
            );
        });
        assert_eq!(
            fake.files.text("/zfsbootmenu/luks-mappings").unwrap(),
            "a\t/dev/sda\t1\nab\t/dev/sdb\t0\n"
        );
        assert!(
            !fake
                .files
                .exists(Path::new("/zfsbootmenu/luks-mappings.lock"))
        );
    }

    #[test]
    fn a_stale_lock_is_waited_for_and_ignored() {
        let fake = FakeSystem::new();
        fake.files.dir("/zfsbootmenu/luks-mappings.lock");
        with_session(&fake, |session| {
            session.record_mapping("a", "/dev/sda", false)
        });
        assert_eq!(
            fake.files.text("/zfsbootmenu/luks-mappings").unwrap(),
            "a\t/dev/sda\t0\n"
        );
        assert_eq!(fake.clock.now(), Duration::from_millis(4900));
        // The stale lock is removed with the record.
        assert!(
            !fake
                .files
                .exists(Path::new("/zfsbootmenu/luks-mappings.lock"))
        );
    }

    #[test]
    fn skipped_volumes_are_left_alone_by_retries() {
        let fake = FakeSystem::new();
        fake.files.block_device("/dev/sda").block_device("/dev/sdb");
        with_session(&fake, |session| {
            assert!(!session.retry_ignores("/dev/sda"));
            session.mark_skipped("/dev/sda");
            session.mark_skipped("/dev/sda");
            assert!(session.retry_ignores("/dev/sda"));
            assert!(!session.is_skipped("/dev/sd"));
            assert!(!session.retry_ignores("/dev/sdb"));
            fake.files.file("/fake/holders/sdb", "v\n");
            assert!(session.retry_ignores("/dev/sdb"));
        });
        assert_eq!(
            fake.files.text("/zfsbootmenu/luks-skipped").unwrap(),
            "/dev/sda\n"
        );
    }

    #[test]
    fn mapping_files_read_as_the_legacy_loop_reads_them() {
        let texts = [
            "",
            "a\t/dev/sda\t1\n",
            "a\t/dev/sda\t1\nb\t/dev/sdb\t0",
            "\t\ta\t\t/dev/x\t\t1\t\n",
            "a\n\nb\t/dev/b\n\t\n",
            "a\t/d\t1\textra\tfields\n",
            "a b\t/dev/s d\t1\n",
            "a\\b\t/d\\x\t1\n",
        ];
        for text in texts {
            let dir = TempDir::new("zbm-luks-mappings");
            let path = dir.write("luks-mappings", text);
            let show = format!(
                r#"show() {{
                     local name dev disc
                     while IFS=$'\t' read -r name dev disc; do
                       [ -n "${{name}}" ] || continue
                       printf '[%s] [%s] [%s]\n' "${{name}}" "${{dev}}" "${{disc}}"
                     done < {}
                   }}"#,
                shell_quote(&path.to_string_lossy())
            );
            let legacy = legacy_luks().code(&show).call("show", &[], b"");
            let rust: String = parse_mappings(text)
                .iter()
                .map(|mapping| {
                    format!(
                        "[{}] [{}] [{}]\n",
                        mapping.name, mapping.device, mapping.auto
                    )
                })
                .collect();
            assert_eq!(rust, legacy.stdout_text(), "{text:?}");
        }
        assert_eq!(format_mappings(&[]), "");
        assert_eq!(format_mappings(&parse_mappings("a\tb\tc\n")), "a\tb\tc\n");
    }

    #[test]
    fn records_match_the_legacy_function() {
        let dir = TempDir::new("zbm-luks-record");
        let state = dir.join("base/luks-mappings");
        let legacy = legacy_luks()
            .env("LUKS_STATE", &state.to_string_lossy())
            .code(
                "record() { luks_record_mapping a /dev/sda 1; luks_record_mapping b /dev/sdb; \
                 luks_record_mapping a /dev/sdc; luks_record_mapping axb /dev/sdd; \
                 luks_record_mapping a.b /dev/sde; luks_record_mapping 'a*' /dev/sdf; }",
            )
            .call("record", &[], b"");
        assert!(legacy.status.success());
        let fake = FakeSystem::new();
        with_session(&fake, |session| {
            session.record_mapping("a", "/dev/sda", true);
            session.record_mapping("b", "/dev/sdb", false);
            session.record_mapping("a", "/dev/sdc", false);
            // Names are text, not patterns (fixed in the legacy code too).
            session.record_mapping("axb", "/dev/sdd", false);
            session.record_mapping("a.b", "/dev/sde", false);
            session.record_mapping("a*", "/dev/sdf", false);
        });
        assert_eq!(
            fake.files.text("/zfsbootmenu/luks-mappings").unwrap(),
            std::fs::read_to_string(state).unwrap()
        );
    }
}
