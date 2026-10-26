//! The mappings ZFSBootMenu opened (`luks-mappings`) and the volumes the user
//! skipped (`luks-skipped`), kept by a [`MappingStore`].
//!
//! `luks-mappings` has a line `name<TAB>device<TAB>auto` for each mapping,
//! where `auto` is `1` for a volume found by discovery; the teardown hook and
//! `zluks` close what it lists. `luks-skipped` lists a device per line.

use std::path::Path;
use std::time::Duration;

use zbm_platform::clock::Clock;
use zbm_platform::files::Files;

use crate::device::is_open;
use crate::unlocker::Unlocker;

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
        .map(|mapping| mapping.line() + "\n")
        .collect()
}

/// How a volume came to be unlocked, recorded with its mapping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Origin {
    /// The configuration named it (crypttab or `zbm.luks`).
    Configured,
    /// Discovery found it (no crypttab and no `zbm.luks`).
    Discovered,
}

impl Origin {
    /// The value of the `auto` field.
    fn field(self) -> &'static str {
        match self {
            Origin::Configured => "0",
            Origin::Discovered => "1",
        }
    }
}

/// The state files of the mappings ZFSBootMenu opened and of the volumes the
/// user skipped: the repository the unlocking, closing and `zluks` code keeps
/// its records in.
#[derive(Clone, Copy)]
pub struct MappingStore<'a> {
    files: &'a dyn Files,
    clock: &'a dyn Clock,
    mappings: &'a Path,
    skipped: &'a Path,
}

impl<'a> MappingStore<'a> {
    /// The store of the files `mappings` and `skipped`.
    pub fn new(
        files: &'a dyn Files,
        clock: &'a dyn Clock,
        mappings: &'a Path,
        skipped: &'a Path,
    ) -> MappingStore<'a> {
        MappingStore {
            files,
            clock,
            mappings,
            skipped,
        }
    }

    /// The recorded mappings, or `None` when there is no record.
    pub fn read(&self) -> Option<Vec<Mapping>> {
        let text = self.files.read(self.mappings).ok()?;
        Some(parse_mappings(&String::from_utf8_lossy(&text)))
    }

    /// Replaces the recorded mappings.
    pub fn write(&self, mappings: &[Mapping]) {
        let path = self.mappings;
        if let Err(error) = self.files.write(path, format_mappings(mappings).as_bytes()) {
            zbm_trace::error!("unable to write {}: {error}", path.display());
        }
    }

    /// Runs `work` holding the lock of the record (an atomic `mkdir`), which
    /// the console and SSH sessions share; after five seconds the lock
    /// counts as stale and `work` runs anyway.
    fn locked<T>(&self, work: impl FnOnce() -> T) -> T {
        let path = self.mappings;
        if let Some(parent) = path.parent() {
            let _ = self.files.create_dir_all(parent);
        }
        let mut lock = path.as_os_str().to_owned();
        lock.push(".lock");
        let lock = Path::new(&lock);
        let mut tries = 0;
        while self.files.create_dir(lock).is_err() {
            tries += 1;
            if tries >= 50 {
                break;
            }
            self.clock.sleep(Duration::from_millis(100));
        }
        let done = work();
        let _ = self.files.remove_dir(lock);
        done
    }

    /// Records a mapping ZFSBootMenu opened, once. Concurrent sessions
    /// (console and SSH) may record the same mapping at the same time, so
    /// the lock is held across the check and the append.
    pub fn record(&self, name: &str, device: &str, origin: Origin) {
        let path = self.mappings;
        self.locked(|| {
            let recorded = self.files.read(path).is_ok_and(|text| {
                String::from_utf8_lossy(&text)
                    .split('\n')
                    .any(|line| line.starts_with(&format!("{name}\t")))
            });
            if !recorded {
                let mapping = Mapping {
                    name: name.to_owned(),
                    device: device.to_owned(),
                    auto: origin.field().to_owned(),
                };
                let line = format!("{}\n", mapping.line());
                if let Err(error) = self.files.append(path, line.as_bytes()) {
                    zbm_trace::error!("unable to write {}: {error}", path.display());
                }
            }
        });
    }

    /// Forgets the mappings named `names`, which were closed. The record is
    /// read again under the lock, so that a mapping another session
    /// recorded meanwhile stays.
    pub fn remove(&self, names: &[String]) {
        if names.is_empty() {
            return;
        }
        self.locked(|| {
            let Some(mappings) = self.read() else {
                return;
            };
            let kept: Vec<Mapping> = mappings
                .into_iter()
                .filter(|mapping| !names.contains(&mapping.name))
                .collect();
            self.write(&kept);
        });
    }

    /// Whether the user skipped `device` in this boot.
    pub fn is_skipped(&self, device: &str) -> bool {
        self.files.read(self.skipped).is_ok_and(|text| {
            text.split(|byte| *byte == b'\n')
                .any(|line| line == device.as_bytes())
        })
    }

    /// Records that the user skipped `device`, so that retries leave it alone.
    pub fn mark_skipped(&self, device: &str) {
        let path = self.skipped;
        if let Some(parent) = path.parent() {
            let _ = self.files.create_dir_all(parent);
        }
        if !self.is_skipped(device)
            && let Err(error) = self.files.append(path, format!("{device}\n").as_bytes())
        {
            zbm_trace::error!("unable to write {}: {error}", path.display());
        }
    }
}

impl Unlocker<'_> {
    /// The store of the mappings and skipped volumes of the settings.
    pub fn mappings(&self) -> MappingStore<'_> {
        let settings = self.settings();
        MappingStore::new(
            self.system().files,
            self.system().clock,
            &settings.mappings,
            &settings.skipped,
        )
    }

    /// Whether a retry leaves `device` alone because it is open or skipped.
    pub fn retry_ignores(&self, device: &str) -> bool {
        is_open(self.system().files, device, &self.settings().open_check)
            || self.mappings().is_skipped(device)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::OpenCheck;
    use crate::testing::{NoPrompts, legacy_luks, log};
    use crate::unlocker::{Luks, Settings};
    use zbm_platform::clock::Clock;
    use zbm_platform::files::Files;
    use zbm_testkit::fakes::FakeSystem;
    use zbm_testkit::legacy::shell_quote;
    use zbm_testkit::{TempDir, parity};

    fn with_session<T>(fake: &FakeSystem, body: impl FnOnce(&Unlocker<'_>) -> T) -> T {
        let mut settings = Settings::new("/zfsbootmenu");
        settings.open_check = OpenCheck::Markers("/fake/holders".into());
        let _log = log(fake);
        let luks = Luks {
            system: fake.system(),
            settings: &settings,
            prompter: &NoPrompts,
        };
        body(&Unlocker::new(luks))
    }

    #[test]
    fn mappings_are_recorded_once() {
        let fake = FakeSystem::new();
        with_session(&fake, |session| {
            assert_eq!(session.mappings().read(), None);
            session
                .mappings()
                .record("a", "/dev/sda", Origin::Discovered);
            session
                .mappings()
                .record("ab", "/dev/sdb", Origin::Configured);
            session
                .mappings()
                .record("a", "/dev/sdc", Origin::Configured);
            assert_eq!(
                session.mappings().read().unwrap(),
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
    fn closed_mappings_are_forgotten_and_those_recorded_meanwhile_kept() {
        let fake = FakeSystem::new();
        with_session(&fake, |session| {
            let store = session.mappings();
            store.record("a", "/dev/sda", Origin::Discovered);
            store.record("b", "/dev/sdb", Origin::Discovered);
            // Read before another session records `c`; the removal reads
            // the record again.
            let before = store.read().unwrap();
            store.record("c", "/dev/sdc", Origin::Configured);

            store.remove(&[before[0].name.clone()]);
            store.remove(&[]);
        });
        assert_eq!(
            fake.files.text("/zfsbootmenu/luks-mappings").unwrap(),
            "b\t/dev/sdb\t1\nc\t/dev/sdc\t0\n"
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
            session
                .mappings()
                .record("a", "/dev/sda", Origin::Configured);
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
            session.mappings().mark_skipped("/dev/sda");
            session.mappings().mark_skipped("/dev/sda");
            assert!(session.retry_ignores("/dev/sda"));
            assert!(!session.mappings().is_skipped("/dev/sd"));
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
        let mut parity = parity!("mapping_files_read_as_the_legacy_loop_reads_them");
        for text in texts {
            let legacy: String = parity.legacy(format!("{text:?}"), || {
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
                legacy_luks()
                    .code(&show)
                    .call("show", &[], b"")
                    .stdout_text()
            });
            let rust: String = parse_mappings(text)
                .iter()
                .map(|mapping| {
                    let fields = [&mapping.name, &mapping.device, &mapping.auto];
                    ["[", fields[0], "] [", fields[1], "] [", fields[2], "]\n"].concat()
                })
                .collect();
            assert_eq!(rust, legacy, "{text:?}");
        }
        assert_eq!(format_mappings(&[]), "");
        assert_eq!(format_mappings(&parse_mappings("a\tb\tc\n")), "a\tb\tc\n");
    }

    #[test]
    fn records_match_the_legacy_function() {
        let mut parity = parity!("records_match_the_legacy_function");
        let recorded: String = parity.legacy("a, b, a, axb, a.b, a*", || {
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
            assert!(legacy.status.is_success());
            std::fs::read_to_string(state).unwrap()
        });
        let fake = FakeSystem::new();
        with_session(&fake, |session| {
            session
                .mappings()
                .record("a", "/dev/sda", Origin::Discovered);
            session
                .mappings()
                .record("b", "/dev/sdb", Origin::Configured);
            session
                .mappings()
                .record("a", "/dev/sdc", Origin::Configured);
            // Names are text, not patterns (fixed in the legacy code too).
            session
                .mappings()
                .record("axb", "/dev/sdd", Origin::Configured);
            session
                .mappings()
                .record("a.b", "/dev/sde", Origin::Configured);
            session
                .mappings()
                .record("a*", "/dev/sdf", Origin::Configured);
        });
        assert_eq!(
            fake.files.text("/zfsbootmenu/luks-mappings").unwrap(),
            recorded
        );
    }
}
