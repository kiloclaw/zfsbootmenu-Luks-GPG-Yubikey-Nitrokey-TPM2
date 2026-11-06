use std::io;

use zbm_platform::event::CancelToken;
use zbm_platform::exec::{Command, Output, Status};
use zbm_platform::{CancellingRunner, Context, Runner};
use zbm_testkit::FakeRunner;
use zbm_testkit::fakes::FakeSystem;
use zbm_types::Untrusted;

use super::*;
use crate::saved_logs::{MOUNT_POINT, list};

/// The partition table of a machine with one ESP, as `lsblk` lists it.
const ONE_ESP: &str = "/dev/vda \n/dev/vda1 C12A7328-F81F-11D2-BA4B-00A0C93EC93B\n";

/// The read-only mount of the ESP.
const MOUNT: &str = "mount -t vfat -o ro,nosuid,nodev,noexec /dev/vda1 /run/zbm-saved-logs-read";

/// The mount table while the ESP is mounted for reading.
const LEFT: &str = "/dev/vda1 /run/zbm-saved-logs-read vfat ro 0 0\n";

/// Where the saved boots are while the ESP is mounted for reading.
fn logs() -> std::path::PathBuf {
    Path::new(READ_MOUNT_POINT).join(DIRECTORY)
}

/// A machine with one ESP.
fn machine() -> FakeSystem {
    let fake = FakeSystem::new();
    fake.runner.respond("lsblk -rnpo NAME,PARTTYPE", 0, ONE_ESP);
    fake
}

/// The record of a boot of `environment`.
fn record(environment: &str) -> BootRecord {
    BootRecord {
        time: 1_793_246_955,
        environment: Untrusted::new(environment.to_owned()),
        kernel: Untrusted::new("/boot/vmlinuz-6.12.9_1".to_owned()),
        loaded_by: Untrusted::new("kexec_file_load".to_owned()),
        ..BootRecord::default()
    }
}

/// Saves a boot named `name` with `files`, each holding its name.
fn saved(fake: &FakeSystem, name: &str, files: &[&str]) {
    for file in files {
        fake.files.file(logs().join(name).join(file), *file);
    }
}

#[test]
fn the_boots_are_read_newest_first_with_their_records() {
    let fake = machine();
    let keep = FakeRunner::new();
    saved(
        &fake,
        "2026-10-21T08-00-00Z",
        &["journal.jsonl", "kernel.log"],
    );
    saved(&fake, "2026-10-22T08-00-00Z", &["journal.jsonl"]);
    let text = record("zroot/ROOT/void").to_json().unwrap();
    let newest = logs().join("2026-10-23T08-00-00Z");
    fake.files
        .file(newest.join("boot.json"), text.as_str())
        .file(newest.join("kernel.log"), "log")
        .file(newest.join("boot.json.tmp"), "{")
        .file(logs().join("README"), "not a boot");

    let read = history(&fake.context(), &Destination::Esp, &keep).unwrap();

    let names: Vec<&str> = read.iter().map(|boot| boot.saved.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "2026-10-23T08-00-00Z",
            "2026-10-22T08-00-00Z",
            "2026-10-21T08-00-00Z"
        ]
    );
    let len = u64::try_from(text.len()).unwrap();
    assert_eq!(
        read[0].saved.files,
        [("boot.json".to_owned(), len), ("kernel.log".to_owned(), 3)]
    );
    assert_eq!(read[0].record, Some(Ok(record("zroot/ROOT/void"))));
    // A boot saved from the shell has no record.
    assert_eq!(read[1].record, None);
    assert_eq!(fake.runner.lines().last().map(String::as_str), Some(MOUNT));
    assert_eq!(keep.lines(), ["umount /run/zbm-saved-logs-read"]);
}

#[test]
fn a_broken_or_oversized_or_newer_record_is_said_unread() {
    let fake = machine();
    let keep = FakeRunner::new();
    let dir = |day: u8| logs().join(format!("2026-10-2{day}T08-00-00Z"));
    let long = format!(
        "{{\"schema\":1,\"pad\":\"{}\"}}",
        "x".repeat(BootRecord::MAX_LEN)
    );
    fake.files
        .file(dir(1).join("boot.json"), "{\"schema\":1,\"ti")
        .file(dir(2).join("boot.json"), long.as_str())
        .file(dir(3).join("boot.json"), "{\"schema\":2}");

    let read = history(&fake.context(), &Destination::Esp, &keep).unwrap();

    let records: Vec<_> = read.iter().map(|boot| boot.record.clone()).collect();
    assert_eq!(
        records,
        [
            Some(Err(RecordError::Schema(2))),
            Some(Err(RecordError::TooLong {
                max: BootRecord::MAX_LEN
            })),
            Some(Err(RecordError::Invalid {
                line: 1,
                column: 15
            })),
        ]
    );
}

#[test]
fn a_record_whose_file_cannot_be_read_is_unread() {
    let fake = FakeSystem::new();
    let path = Path::new("/esp/boot.json");
    assert_eq!(super::record(&fake.files, path), Err(RecordError::Unread));
}

#[test]
fn names_taken_twice_order_by_their_number() {
    assert!(order("2026-10-23T08-00-00Z.10") > order("2026-10-23T08-00-00Z.9"));
    assert!(order("2026-10-23T08-00-00Z.2") > order("2026-10-23T08-00-00Z"));
    assert!(order("2026-10-24T08-00-00Z") > order("2026-10-23T08-00-00Z.10"));
}

#[test]
fn a_crowded_partition_is_read_only_so_far() {
    let fake = machine();
    let keep = FakeRunner::new();
    for day in 1..=(READ_BOOTS + 4) {
        saved(
            &fake,
            &format!("2026-01-{day:02}T00-00-00Z"),
            &["kernel.log"],
        );
    }
    let many: Vec<String> = (0..FILES_READ + 8)
        .map(|n| format!("crash-{n:03}.txt"))
        .collect();
    let many: Vec<&str> = many.iter().map(String::as_str).collect();
    saved(&fake, "2026-01-31T00-00-00Z", &many);

    let read = history(&fake.context(), &Destination::Esp, &keep).unwrap();

    assert_eq!(read.len(), READ_BOOTS);
    assert_eq!(read[0].saved.name, "2026-01-31T00-00-00Z");
    assert_eq!(read[0].saved.files.len(), FILES_READ);
    assert_eq!(read[READ_BOOTS - 1].saved.name, "2026-01-06T00-00-00Z");
}

#[test]
fn a_partition_that_does_not_mount_is_said_so_and_nothing_is_unmounted() {
    let fake = machine();
    let keep = FakeRunner::new();
    fake.runner.respond(MOUNT, 32, "");

    let error = history(&fake.context(), &Destination::Esp, &keep).unwrap_err();

    assert!(matches!(&error, SaveError::Mount(device) if device == "/dev/vda1"));
    assert!(keep.lines().is_empty(), "{:?}", keep.lines());
    assert!(!fake.files.exists(Path::new(READ_MOUNT_POINT)));
}

#[test]
fn a_mount_that_failed_but_mounted_is_unmounted() {
    let fake = machine();
    let keep = FakeRunner::new();
    let files = fake.files.clone();
    fake.runner.first(move |command| {
        if command.display() != MOUNT {
            return None;
        }
        files.file("/proc/self/mounts", LEFT);
        Some(Ok(Output::new(Status::exited(137), Vec::new())))
    });

    let read = history(&fake.context(), &Destination::Esp, &keep);

    assert!(read.is_err());
    assert_eq!(keep.lines(), ["umount /run/zbm-saved-logs-read"]);
}

#[test]
fn a_machine_without_an_esp_reads_nothing() {
    let fake = FakeSystem::new();
    let keep = FakeRunner::new();

    let error = history(&fake.context(), &Destination::Esp, &keep).unwrap_err();

    assert!(matches!(error, SaveError::NoEsp));
    assert!(
        fake.runner
            .lines()
            .iter()
            .all(|line| !line.starts_with("mount"))
    );
}

/// A runner that cancels `token` once the ESP is mounted, as a person
/// pressing Escape while the boots are read.
struct CancelAfterMount<'a> {
    inner: &'a dyn Runner,
    token: CancelToken,
}

impl Runner for CancelAfterMount<'_> {
    fn run(&self, command: &Command) -> io::Result<Output> {
        let output = self.inner.run(command);
        if command.display().starts_with("mount -t vfat") {
            self.token.cancel();
        }
        output
    }
}

#[test]
fn a_read_cancelled_once_mounted_is_still_unmounted() {
    let fake = machine();
    let keep = FakeRunner::new();
    saved(&fake, "2026-10-23T08-00-00Z", &["kernel.log"]);
    let token = CancelToken::new().unwrap();
    let cancelling = CancelAfterMount {
        inner: &fake.runner,
        token: token.clone(),
    };
    let runner = CancellingRunner::new(&cancelling, token.clone());
    let context = Context::new(fake.system().with_runner(&runner));

    let read = history(&context, &Destination::Esp, &keep).unwrap();

    assert!(token.is_cancelled());
    assert_eq!(read.len(), 1);
    assert_eq!(keep.lines(), ["umount /run/zbm-saved-logs-read"]);
    assert!(
        fake.runner
            .lines()
            .iter()
            .all(|line| !line.starts_with("umount"))
    );
}

#[test]
fn the_read_and_the_save_use_their_own_mount_points() {
    let fake = machine();
    let keep = FakeRunner::new();
    saved(&fake, "2026-10-23T08-00-00Z", &["kernel.log"]);
    fake.files.file(
        Path::new(MOUNT_POINT)
            .join(DIRECTORY)
            .join("2025-01-01T00-00-00Z/kernel.log"),
        "",
    );

    let read = history(&fake.context(), &Destination::Esp, &keep).unwrap();
    let (_, listed) = list(&fake.context(), &Destination::Esp).unwrap();

    assert_eq!(read[0].saved.name, "2026-10-23T08-00-00Z");
    assert_eq!(listed[0].name, "2025-01-01T00-00-00Z");
}

#[test]
fn a_mount_an_earlier_read_left_is_unmounted_before_the_read() {
    let fake = machine();
    let keep = FakeRunner::new();
    fake.files.file("/proc/self/mounts", LEFT);
    saved(&fake, "2026-10-23T08-00-00Z", &["kernel.log"]);

    let read = history(&fake.context(), &Destination::Esp, &keep).unwrap();

    assert_eq!(read.len(), 1);
    let umount = "umount /run/zbm-saved-logs-read";
    assert_eq!(keep.lines(), [umount, umount]);
}

#[test]
fn a_mount_left_that_stays_stops_the_read() {
    let fake = machine();
    let keep = FakeRunner::new();
    keep.respond("umount /run/zbm-saved-logs-read", 32, "");
    fake.files.file("/proc/self/mounts", LEFT);

    let error = history(&fake.context(), &Destination::Esp, &keep).unwrap_err();

    assert!(matches!(&error, SaveError::Mount(device) if device == "/dev/vda1"));
    assert!(!fake.runner.lines().iter().any(|line| line == MOUNT));
}

#[test]
fn a_save_unmounts_the_read_only_mount_a_read_left() {
    let fake = machine();
    fake.files.file("/proc/self/mounts", LEFT);

    let _ = list(&fake.context(), &Destination::Esp).unwrap();

    let lines = fake.runner.lines();
    let at = |line: &str| lines.iter().position(|ran| ran == line);
    let unmounted = at("umount /run/zbm-saved-logs-read").unwrap();
    let mounted = at("mount -t vfat -o rw,noatime /dev/vda1 /run/zbm-saved-logs").unwrap();
    assert!(unmounted < mounted, "{lines:?}");
}

/// Files that count how often what is below `below` is looked at.
struct Counted<'a> {
    inner: &'a dyn Files,
    below: std::path::PathBuf,
    looked: std::sync::atomic::AtomicUsize,
}

impl Counted<'_> {
    /// How often what is below `below` was looked at.
    fn looked(&self) -> usize {
        self.looked.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// Answers each call with `inner`'s.
macro_rules! passed_on {
    ($($name:ident($($arg:ident: $ty:ty),*) -> $out:ty;)*) => {
        $(fn $name(&self, $($arg: $ty),*) -> $out {
            self.inner.$name($($arg),*)
        })*
    };
}

impl Files for Counted<'_> {
    passed_on! {
        read(path: &Path) -> io::Result<Vec<u8>>;
        read_prefix(path: &Path, len: usize) -> io::Result<Vec<u8>>;
        write(path: &Path, contents: &[u8]) -> io::Result<()>;
        write_private(path: &Path, contents: &[u8]) -> io::Result<()>;
        lock(path: &Path, wait: zbm_platform::LockWait)
            -> io::Result<Option<zbm_platform::FileLock>>;
        is_locked(path: &Path) -> bool;
        append(path: &Path, contents: &[u8]) -> io::Result<()>;
        create_dir(path: &Path) -> io::Result<()>;
        create_dir_all(path: &Path) -> io::Result<()>;
        remove_file(path: &Path) -> io::Result<()>;
        remove_dir(path: &Path) -> io::Result<()>;
        rename(from: &Path, to: &Path) -> io::Result<()>;
        read_dir(path: &Path) -> io::Result<Vec<std::ffi::OsString>>;
        set_mode(path: &Path, mode: u32) -> io::Result<()>;
        canonicalize(path: &Path) -> io::Result<std::path::PathBuf>;
        read_link(path: &Path) -> io::Result<std::path::PathBuf>;
        terminal_size(path: &Path) -> io::Result<zbm_platform::Size>;
    }

    fn metadata(&self, path: &Path) -> io::Result<zbm_platform::Metadata> {
        if path.starts_with(&self.below) {
            self.looked
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        self.inner.metadata(path)
    }

    fn shared(&self) -> std::sync::Arc<dyn Files + Send + Sync> {
        self.inner.shared()
    }
}

#[test]
fn only_the_files_listed_of_a_crowded_boot_are_looked_at() {
    let fake = FakeSystem::new();
    let crowded = Path::new("/esp/zfsbootmenu/logs/2026-01-31T00-00-00Z");
    for n in 0..FILES_READ * 4 {
        fake.files
            .file(crowded.join(format!("crash-{n:03}.txt")), "");
    }
    let files = Counted {
        inner: &fake.files,
        below: crowded.to_path_buf(),
        looked: 0.into(),
    };

    let read = past_boots(&files, Path::new("/esp/zfsbootmenu/logs"));

    assert_eq!(read[0].saved.files.len(), FILES_READ);
    // The directory, each file listed, and the record, which it has not.
    assert_eq!(files.looked(), 1 + FILES_READ + 1);
}
