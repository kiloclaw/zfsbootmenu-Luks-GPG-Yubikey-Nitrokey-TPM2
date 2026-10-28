//! Probing block devices: `blkid` (with no cache, `-c /dev/null`, so it
//! always reads the devices), and their sizes and partition types from the
//! kernel (`/sys/class/block`) and udev (`/run/udev/data`), as `lsblk`
//! reads them, which runs only where those cannot be read (ROADMAP.md CD2).
//!
//! Each probe returns what the program printed without its final newlines,
//! as the shell's `$(...)` does, and an empty answer when the program fails.

#![allow(
    clippy::disallowed_methods,
    reason = "an adapter: these command lines are built here and nowhere else"
)]

use std::path::{Path, PathBuf};
use std::time::Duration;
use zbm_platform::{Command, Files, Input, Runner, Stream};

/// The longest a probe may run, for a device that stopped answering.
const TIMEOUT: Duration = Duration::from_secs(60);

/// The kernel's block devices.
const SYS_BLOCK: &str = "/sys/class/block";

/// udev's database of devices.
const UDEV_DATA: &str = "/run/udev/data";

/// The block devices: `blkid`, run through a [`Runner`], and what the
/// kernel and udev say of them, read through [`Files`].
#[derive(Clone, Copy)]
pub struct Devices<'a> {
    runner: &'a dyn Runner,
    files: &'a dyn Files,
}

/// The text without its final newlines, as `$(...)` captures it.
fn captured(text: &str) -> String {
    text.trim_end_matches('\n').to_owned()
}

impl<'a> Devices<'a> {
    /// The devices, with programs run through `runner` and files read
    /// through `files`.
    pub fn new(runner: &'a dyn Runner, files: &'a dyn Files) -> Devices<'a> {
        Devices { runner, files }
    }

    /// Standard output of `program` with `args`; when `only_success`, empty
    /// unless it succeeds.
    fn output(&self, program: &str, args: &[&str], only_success: bool) -> String {
        let command = Command::new(program)
            .stdin(Input::Null)
            .timeout(TIMEOUT)
            .args(args.iter().copied())
            .stdout(Stream::Capture)
            .stderr(Stream::Null);
        match self.runner.run(&command) {
            Ok(output) if !only_success || output.status.is_success() => {
                captured(&output.stdout_text())
            }
            _ => String::new(),
        }
    }

    /// The first device whose tag `key` is `value`
    /// (`blkid -c /dev/null -l -o device -t KEY=value`); empty when there is
    /// none.
    pub fn find(&self, key: &str, value: &str) -> String {
        let tag = format!("{key}={value}");
        self.output(
            "blkid",
            &["-c", "/dev/null", "-l", "-o", "device", "-t", &tag],
            false,
        )
    }

    /// Every device with a LUKS header
    /// (`blkid -c /dev/null -o device -t TYPE=crypto_LUKS`), one per line.
    pub fn luks_devices(&self) -> String {
        self.output(
            "blkid",
            &["-c", "/dev/null", "-o", "device", "-t", "TYPE=crypto_LUKS"],
            false,
        )
    }

    /// What `device` holds, probed from its contents
    /// (`blkid -c /dev/null -p -s TYPE -o value`), such as `zfs_member`;
    /// empty when it is not known.
    pub fn content_type(&self, device: &str) -> String {
        self.output(
            "blkid",
            &["-c", "/dev/null", "-p", "-s", "TYPE", "-o", "value", device],
            true,
        )
    }

    /// The tag `tag` of `device`, such as its `LABEL`
    /// (`blkid -c /dev/null -s TAG -o value`); empty when it has none.
    pub fn tag(&self, device: &str, tag: &str) -> String {
        self.output(
            "blkid",
            &["-c", "/dev/null", "-s", tag, "-o", "value", device],
            true,
        )
    }

    /// Whether a block device holds a file system or signature of type
    /// `kind` (`blkid -t TYPE=KIND`), such as `swsuspend`.
    pub fn has_type(&self, kind: &str) -> bool {
        let command = Command::new("blkid")
            .timeout(TIMEOUT)
            .args(["-t", &format!("TYPE={kind}")])
            .stdout(Stream::Null)
            .stderr(Stream::Null);
        self.runner
            .run(&command)
            .is_ok_and(|output| output.status.is_success())
    }

    /// The partitions whose GPT partition type is `kind`, a GUID such as
    /// the EFI system partition's, as paths, in the order of their names:
    /// the type udev read from the partition table (`ID_PART_ENTRY_TYPE`).
    /// Without udev's record of any partition, `lsblk -rnpo NAME,PARTTYPE`
    /// says.
    pub fn partitions_of_type(&self, kind: &str) -> Vec<String> {
        let mut recorded = false;
        let mut found = Vec::new();
        for name in self.block_devices() {
            let dir = Path::new(SYS_BLOCK).join(&name);
            if !self.files.exists(&dir.join("partition")) {
                continue;
            }
            let Some(kind_found) = self.udev_property(&dir, "ID_PART_ENTRY_TYPE") else {
                continue;
            };
            recorded = true;
            if kind_found.eq_ignore_ascii_case(kind) {
                found.push(format!("/dev/{name}"));
            }
        }
        if recorded {
            return found;
        }
        self.output("lsblk", &["-rnpo", "NAME,PARTTYPE"], true)
            .lines()
            .filter_map(|line| line.split_once(' '))
            .filter(|(_, found)| found.eq_ignore_ascii_case(kind))
            .map(|(name, _)| name.to_owned())
            .collect()
    }

    /// The names of the kernel's block devices, in byte order.
    fn block_devices(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .files
            .read_dir(Path::new(SYS_BLOCK))
            .unwrap_or_default()
            .into_iter()
            .map(|name| name.to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// The value of `key` in udev's record of the block device whose
    /// directory in `/sys/class/block` is `dir`.
    fn udev_property(&self, dir: &Path, key: &str) -> Option<String> {
        let dev = self.files.read_to_string(&dir.join("dev")).ok()?;
        let record = Path::new(UDEV_DATA).join(format!("b{}", dev.trim()));
        let text = self.files.read_to_string(&record).ok()?;
        let prefix = format!("E:{key}=");
        text.lines()
            .find_map(|line| line.strip_prefix(&prefix))
            .map(str::to_owned)
    }

    /// The size of `device` as `lsblk -dno SIZE` prints it, without spaces,
    /// such as `512M`, from the kernel's count of its sectors; `lsblk` says
    /// where that cannot be read. Empty when neither can.
    pub fn size(&self, device: &str) -> String {
        if let Some(bytes) = self.size_in_bytes(device) {
            return human_size(bytes);
        }
        self.output("lsblk", &["-dno", "SIZE", device], true)
            .replace(' ', "")
    }

    /// The size of `device` in bytes, which `/sys/class/block/NAME/size`
    /// counts in sectors of 512 bytes whatever the device's own.
    fn size_in_bytes(&self, device: &str) -> Option<u64> {
        let real = self.files.canonicalize(Path::new(device)).ok()?;
        let name: PathBuf = real.file_name()?.into();
        let size = Path::new(SYS_BLOCK).join(name).join("size");
        let sectors: u64 = self.files.read_to_string(&size).ok()?.trim().parse().ok()?;
        sectors.checked_mul(512)
    }
}

/// `bytes` as `lsblk` prints a size: in the largest unit of 1024 that it
/// reaches (`B`, `K`, `M`, `G`, `T`, `P`, `E`), with one decimal rounded half
/// up when it is not 0, such as `7.5G` or `256G` (util-linux's
/// `size_to_human_string`).
pub fn human_size(bytes: u64) -> String {
    const UNITS: [char; 7] = ['B', 'K', 'M', 'G', 'T', 'P', 'E'];
    let mut exponent = 0_u32;
    while exponent < 60 && bytes >= 1_u64 << (exponent + 10) {
        exponent += 10;
    }
    let unit = UNITS
        .get(usize::try_from(exponent / 10).unwrap_or(0))
        .copied()
        .unwrap_or('E');
    let mut whole = bytes >> exponent;
    let rest = bytes - (whole << exponent);
    // Thousandths of the unit, then tenths rounded half up.
    let thousandths = if exponent == 0 {
        0
    } else {
        u64::try_from((u128::from(rest) * 1000) >> exponent).unwrap_or(0)
    };
    let mut tenths = (thousandths + 50) / 100;
    if tenths == 10 {
        whole += 1;
        tenths = 0;
    }
    if tenths == 0 {
        format!("{whole}{unit}")
    } else {
        format!("{whole}.{tenths}{unit}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use zbm_testkit::FakeRunner;
    use zbm_testkit::fakes::FakeFiles;

    #[test]
    fn probes_run_blkid_without_its_cache() {
        let runner = FakeRunner::new();
        runner.respond(
            "blkid -c /dev/null -l -o device -t LABEL=key",
            0,
            "/dev/sdb1\n\n",
        );
        runner.respond(
            "blkid -c /dev/null -o device -t TYPE=crypto_LUKS",
            0,
            "/dev/sda2\n/dev/sdc\n",
        );
        runner.respond(
            "blkid -c /dev/null -p -s TYPE -o value /dev/mapper/a",
            0,
            "zfs_member\n",
        );
        runner.respond(
            "blkid -c /dev/null -s LABEL -o value /dev/sda2",
            2,
            "ignored\n",
        );
        runner.respond("lsblk -dno SIZE /dev/sda2", 0, " 7.5G\n");
        runner.respond(
            "lsblk -rnpo NAME,PARTTYPE",
            0,
            "/dev/sda \n/dev/sda1 C12A7328-F81F-11D2-BA4B-00A0C93EC93B\n/dev/sda2 6a85cf4d\n",
        );
        let files = FakeFiles::new();
        let devices = Devices::new(&runner, &files);

        assert_eq!(devices.find("LABEL", "key"), "/dev/sdb1");
        assert_eq!(devices.luks_devices(), "/dev/sda2\n/dev/sdc");
        assert_eq!(devices.content_type("/dev/mapper/a"), "zfs_member");
        assert_eq!(devices.tag("/dev/sda2", "LABEL"), "");
        assert_eq!(devices.size("/dev/sda2"), "7.5G");
        let esp = devices.partitions_of_type("c12a7328-f81f-11d2-ba4b-00a0c93ec93b");
        assert_eq!(esp, ["/dev/sda1"]);
        let failing = FakeRunner::new();
        failing.on(|_| Some(Err(std::io::Error::other("no blkid"))));
        assert_eq!(Devices::new(&failing, &files).find("UUID", "x"), "");
        runner.respond("blkid -t TYPE=suspend", 2, "");
        assert!(devices.has_type("swsuspend"));
        assert!(!devices.has_type("suspend"));
    }

    /// The kernel's and udev's record of a disk with an EFI system partition
    /// and a LUKS partition, and of a partition udev has no record of.
    fn sysfs() -> FakeFiles {
        let files = FakeFiles::new();
        for (name, dev, sectors) in [
            ("sda", "8:0", "1000215216"),
            ("sda1", "8:1", "1048576"),
            ("sda2", "8:2", "15728640"),
            ("sdb1", "8:17", "25568"),
        ] {
            files.file(format!("/sys/class/block/{name}/dev"), format!("{dev}\n"));
            files.file(
                format!("/sys/class/block/{name}/size"),
                format!("{sectors}\n"),
            );
            if name != "sda" {
                files.file(format!("/sys/class/block/{name}/partition"), "1\n");
            }
            files.block_device(format!("/dev/{name}"));
        }
        files.file(
            "/run/udev/data/b8:1",
            "S:disk/by-partlabel/EFI\nE:ID_PART_ENTRY_TYPE=C12A7328-F81F-11D2-BA4B-00A0C93EC93B\n",
        );
        files.file(
            "/run/udev/data/b8:2",
            "E:ID_PART_ENTRY_TYPE=ca7d7ccb-63ed-4c53-861c-1742536059cc\n",
        );
        files
    }

    #[test]
    fn sizes_and_partition_types_come_from_the_kernel_and_udev() {
        let runner = FakeRunner::new();
        let files = sysfs();
        let devices = Devices::new(&runner, &files);

        assert_eq!(devices.size("/dev/sda"), "476.9G");
        assert_eq!(devices.size("/dev/sda1"), "512M");
        assert_eq!(devices.size("/dev/sda2"), "7.5G");
        assert_eq!(devices.size("/dev/sdb1"), "12.5M");
        let esp = devices.partitions_of_type("c12a7328-f81f-11d2-ba4b-00a0c93ec93b");
        assert_eq!(esp, ["/dev/sda1"]);
        assert!(runner.calls().is_empty(), "{:?}", runner.lines());
    }

    #[test]
    fn sizes_are_rounded_as_lsblk_rounds_them() {
        for (bytes, shown) in [
            (0, "0B"),
            (512, "512B"),
            (1023, "1023B"),
            (1024, "1K"),
            (1536, "1.5K"),
            // 1023.96K rounds up to the next whole number of the unit.
            (1_048_535, "1024K"),
            (1_048_576, "1M"),
            (13_090_816, "12.5M"),
            (274_877_906_944, "256G"),
            (2_000_398_934_016, "1.8T"),
            (u64::MAX, "16E"),
        ] {
            assert_eq!(human_size(bytes), shown, "{bytes}");
        }
    }

    #[test]
    fn sizes_match_lsblk_on_this_machine() {
        // Every block device of the host, where `lsblk` is there to compare.
        if !zbm_testkit::have_program("lsblk") {
            return;
        }
        let runner = zbm_platform::SystemRunner;
        let files = zbm_platform::RealFiles;
        let devices = Devices::new(&runner, &files);
        for name in devices.block_devices() {
            let device = format!("/dev/{name}");
            let Some(bytes) = devices.size_in_bytes(&device) else {
                continue;
            };
            if bytes == 0 || !Path::new(&device).exists() {
                continue;
            }
            let listed = devices.output("lsblk", &["-dno", "SIZE", &device], true);
            assert_eq!(human_size(bytes), listed.replace(' ', ""), "{device}");
        }
    }
}
