//! The mount table, and the mount and unmount commands of
//! `zfsbootmenu-core.sh`.

use std::path::Path;

use zbm_platform::exec::{Command, Runner, Stream};

/// One line of the mount table, split as `read -r dev path fs opts` splits it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MountEntry {
    /// The mounted device or dataset.
    pub source: String,
    /// The mount point.
    pub target: String,
    /// The filesystem type.
    pub fstype: String,
    /// The rest of the line: options and dump and pass numbers.
    pub rest: String,
}

/// The mount table, as `/proc/self/mounts` lists it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MountTable {
    entries: Vec<MountEntry>,
}

/// The root filesystem, as `zbm-kcl` finds it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RootFilesystem {
    /// `/` is the given ZFS dataset.
    Zfs(String),
    /// `/` is mounted, but not from ZFS.
    NotZfs,
    /// No mount of `/` is listed.
    Missing,
}

impl MountTable {
    /// Parses the mount table. Fields are not unescaped (a space in a mount point
    /// stays `\040`), as the legacy code does not unescape them either.
    pub fn parse(text: &str) -> MountTable {
        let entries = text
            .lines()
            .map(|line| {
                let mut fields = line.splitn(4, [' ', '\t']).map(str::trim);
                let mut next = || fields.next().unwrap_or_default().to_owned();
                MountEntry {
                    source: next(),
                    target: next(),
                    fstype: next(),
                    rest: next(),
                }
            })
            .collect();
        MountTable { entries }
    }

    /// Reads a mount table file, such as `/proc/self/mounts`.
    pub fn read(path: impl AsRef<Path>) -> std::io::Result<MountTable> {
        Ok(MountTable::parse(&std::fs::read_to_string(path)?))
    }

    /// The entries, in order.
    pub fn entries(&self) -> &[MountEntry] {
        &self.entries
    }

    /// The root filesystem: the first entry for `/` decides.
    pub fn root_filesystem(&self) -> RootFilesystem {
        match self.entries.iter().find(|entry| entry.target == "/") {
            Some(entry) if entry.fstype == "zfs" => RootFilesystem::Zfs(entry.source.clone()),
            Some(_) => RootFilesystem::NotZfs,
            None => RootFilesystem::Missing,
        }
    }

    /// Where `source` is mounted: the target of the first entry for the source.
    pub fn mountpoint_of(&self, source: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|entry| entry.source == source)
            .map(|entry| entry.target.as_str())
    }

    /// The mount points [`unmount_recursive`] unmounts one by one when
    /// `umount -R` fails, in that order.
    ///
    /// These are the mount points that start with `mountpoint` read as the regular
    /// expression `^mountpoint`, so a `.` in it matches any character. They are
    /// ordered as `sort -n -k1 -r` orders `depth,mountpoint`: deepest first, and in
    /// reverse byte order among equals. When there are none, the list holds a
    /// single empty name, as `printf '%s\n'` of an empty array prints one empty
    /// line.
    pub fn unmount_order(&self, mountpoint: &str) -> Vec<String> {
        let mut lines: Vec<(usize, String)> = self
            .entries
            .iter()
            .filter(|entry| matches_prefix(mountpoint, &entry.target))
            .map(|entry| {
                let depth = entry.target.matches('/').count();
                (depth, format!("{depth},{}", entry.target))
            })
            .collect();
        if lines.is_empty() {
            return vec![String::new()];
        }
        lines.sort_by(|(a_depth, a_line), (b_depth, b_line)| {
            b_depth.cmp(a_depth).then_with(|| b_line.cmp(a_line))
        });
        lines
            .into_iter()
            .map(|(_, line)| {
                line.split_once(',')
                    .map(|(_, target)| target.to_owned())
                    .unwrap_or_default()
            })
            .collect()
    }
}

/// Whether `text` starts with `prefix` read as a regular expression without
/// special characters but `.`.
fn matches_prefix(prefix: &str, text: &str) -> bool {
    let mut text = text.chars();
    prefix.chars().all(|expected| match text.next() {
        Some(actual) => expected == '.' || expected == actual,
        None => false,
    })
}

/// The command that mounts a dataset:
/// `mount -o [zfsutil,]ro|rw -t zfs <dataset> <target>`.
///
/// `zfsutil` is needed for datasets with a non-legacy mount point, and must be
/// left out for legacy mount points and for snapshots.
pub fn zfs_mount_command(dataset: &str, target: &str, read_write: bool, zfsutil: bool) -> Command {
    let mode = if read_write { "rw" } else { "ro" };
    let options = if zfsutil {
        format!("zfsutil,{mode}")
    } else {
        mode.to_owned()
    };
    Command::new("mount").args(["-o", &options, "-t", "zfs", dataset, target])
}

/// The command that mounts `efivarfs` read-only (`ro`) or read-write (`rw`)
/// at `location`, or remounts it when it is already mounted there.
pub fn efivarfs_mount_command(state: &str, location: &str, remount: bool) -> Command {
    let options = if remount {
        format!("remount,{state}")
    } else {
        state.to_owned()
    };
    Command::new("mount").args(["-t", "efivarfs", "efivarfs", location, "-o", &options])
}

/// Why [`unmount_recursive`] left something mounted.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum UnmountError {
    /// The mount point was empty.
    #[error("mountpoint undefined")]
    NoMountpoint,
    /// `umount -R` failed, and so did `umount` of these mount points.
    #[error("unable to unmount {}", .0.join(", "))]
    StillMounted(Vec<String>),
}

/// Unmounts `mountpoint` and everything below it: `umount -R` first, then,
/// when that fails, each mount point in [`MountTable::unmount_order`].
///
/// # Errors
///
/// [`UnmountError::NoMountpoint`] when `mountpoint` is empty, and
/// [`UnmountError::StillMounted`] with the mount points that `umount` could
/// not unmount.
pub fn unmount_recursive(
    runner: &dyn Runner,
    table: &MountTable,
    mountpoint: &str,
) -> Result<(), UnmountError> {
    if mountpoint.is_empty() {
        return Err(UnmountError::NoMountpoint);
    }
    let quiet = |command: Command| command.stdout(Stream::Null).stderr(Stream::Null);
    let succeeded = |command: Command| {
        runner
            .run(&quiet(command))
            .is_ok_and(|output| output.status.is_success())
    };
    if succeeded(Command::new("umount").args(["-R", mountpoint])) {
        return Ok(());
    }
    let mounted: Vec<String> = table
        .unmount_order(mountpoint)
        .into_iter()
        .filter(|target| !succeeded(Command::new("umount").arg(target)))
        .collect();
    if mounted.is_empty() {
        Ok(())
    } else {
        Err(UnmountError::StillMounted(mounted))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbm_testkit::FakeRunner;

    const MOUNTS: &str = "\
pool/ROOT/be / zfs rw,relatime,xattr 0 0
proc /proc proc rw,nosuid 0 0
/dev/sda1 /boot/efi vfat rw 0 0
pool/ROOT/be /zfsbootmenu/environments/pool/ROOT/be/mnt zfs ro 0 0
pool/ROOT/be/var /zfsbootmenu/environments/pool/ROOT/be/mnt/var zfs ro 0 0
pool/ROOT/be/var/log /zfsbootmenu/environments/pool/ROOT/be/mnt/var/log zfs ro 0 0
pool/ROOT/be/usr /zfsbootmenu/environments/pool/ROOT/be/mnt/usr zfs ro 0 0
";

    #[test]
    fn root_and_sources() {
        let table = MountTable::parse(MOUNTS);
        assert_eq!(
            table.root_filesystem(),
            RootFilesystem::Zfs("pool/ROOT/be".to_owned())
        );
        assert_eq!(table.mountpoint_of("/dev/sda1"), Some("/boot/efi"));
        assert_eq!(table.mountpoint_of("pool/ROOT/be"), Some("/"));
        assert_eq!(table.entries()[1].rest, "rw,nosuid 0 0");
        assert_eq!(
            MountTable::parse("tmpfs / tmpfs rw 0 0\n").root_filesystem(),
            RootFilesystem::NotZfs
        );
        assert_eq!(
            MountTable::parse("proc /proc proc rw 0 0\n").root_filesystem(),
            RootFilesystem::Missing
        );
    }

    #[test]
    fn unmount_order_is_deepest_first() {
        let table = MountTable::parse(MOUNTS);
        assert_eq!(
            table.unmount_order("/zfsbootmenu/environments/pool/ROOT/be/mnt"),
            [
                "/zfsbootmenu/environments/pool/ROOT/be/mnt/var/log",
                "/zfsbootmenu/environments/pool/ROOT/be/mnt/var",
                "/zfsbootmenu/environments/pool/ROOT/be/mnt/usr",
                "/zfsbootmenu/environments/pool/ROOT/be/mnt",
            ]
        );
        assert_eq!(table.unmount_order("/nothing"), [""]);
        // A dot in the mount point matches any character, as in the legacy regular
        // expression.
        assert_eq!(
            MountTable::parse("x /a/bxc ext4 rw\n").unmount_order("/a/b.c"),
            ["/a/bxc"]
        );
    }

    #[test]
    fn unmount_order_matches_sort() {
        let table = MountTable::parse(MOUNTS);
        let mountpoint = "/zfsbootmenu";
        let entries: String = table
            .entries()
            .iter()
            .filter(|entry| entry.target.starts_with(mountpoint))
            .map(|entry| format!("{},{}\n", entry.target.matches('/').count(), entry.target))
            .collect();
        let sorted = zbm_platform::exec::SystemRunner
            .run(
                &Command::new("sort")
                    .args(["-n", "-k1", "-r"])
                    .env("LC_ALL", "C")
                    .stdin(zbm_platform::exec::Input::Bytes(entries.into_bytes())),
            )
            .unwrap()
            .stdout_text();
        let expected: Vec<&str> = sorted
            .lines()
            .map(|line| line.split_once(',').unwrap().1)
            .collect();
        assert_eq!(table.unmount_order(mountpoint), expected);
    }

    #[test]
    fn commands() {
        assert_eq!(
            zfs_mount_command("pool/be", "/mnt", false, true).display(),
            "mount -o zfsutil,ro -t zfs pool/be /mnt"
        );
        assert_eq!(
            zfs_mount_command("pool/be@snap", "/mnt", true, false).display(),
            "mount -o rw -t zfs pool/be@snap /mnt"
        );
        assert_eq!(
            efivarfs_mount_command("rw", "/sys/firmware/efi/efivars", true).display(),
            "mount -t efivarfs efivarfs /sys/firmware/efi/efivars -o remount,rw"
        );
    }

    #[test]
    fn unmount_recursive_falls_back_to_single_mounts() {
        let table = MountTable::parse(MOUNTS);
        let runner = FakeRunner::new();
        runner.respond(
            "umount -R /zfsbootmenu/environments/pool/ROOT/be/mnt",
            32,
            "",
        );
        runner.respond(
            "umount /zfsbootmenu/environments/pool/ROOT/be/mnt/usr",
            32,
            "",
        );
        let result = unmount_recursive(
            &runner,
            &table,
            "/zfsbootmenu/environments/pool/ROOT/be/mnt",
        );
        assert_eq!(runner.lines().len(), 5);
        assert_eq!(
            result,
            Err(UnmountError::StillMounted(vec![
                "/zfsbootmenu/environments/pool/ROOT/be/mnt/usr".to_owned()
            ]))
        );

        let runner = FakeRunner::new();
        assert_eq!(unmount_recursive(&runner, &table, "/x"), Ok(()));
        assert_eq!(runner.lines(), ["umount -R /x"]);
        assert_eq!(
            unmount_recursive(&runner, &table, ""),
            Err(UnmountError::NoMountpoint)
        );
    }
}
