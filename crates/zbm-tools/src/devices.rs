//! Probing block devices: `blkid` (with no cache, `-c /dev/null`, so it
//! always reads the devices) and `lsblk`.
//!
//! Each probe returns what the program printed without its final newlines,
//! as the shell's `$(...)` does, and an empty answer when the program fails.

#![allow(
    clippy::disallowed_methods,
    reason = "an adapter: these command lines are built here and nowhere else"
)]

use zbm_platform::{Command, Input, Runner, Stream};

/// `blkid` and `lsblk`, run through a [`Runner`].
#[derive(Clone, Copy)]
pub struct Devices<'a> {
    runner: &'a dyn Runner,
}

/// The text without its final newlines, as `$(...)` captures it.
fn captured(text: &str) -> String {
    text.trim_end_matches('\n').to_owned()
}

impl<'a> Devices<'a> {
    /// The programs, run through `runner`.
    pub fn new(runner: &'a dyn Runner) -> Devices<'a> {
        Devices { runner }
    }

    /// Standard output of `program` with `args`; when `only_success`, empty
    /// unless it succeeds.
    fn output(&self, program: &str, args: &[&str], only_success: bool) -> String {
        let command = Command::new(program)
            .stdin(Input::Null)
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
            .args(["-t", &format!("TYPE={kind}")])
            .stdout(Stream::Null)
            .stderr(Stream::Null);
        self.runner
            .run(&command)
            .is_ok_and(|output| output.status.is_success())
    }

    /// The partitions whose GPT partition type is `kind`, a GUID such as
    /// the EFI system partition's, as paths (`lsblk -rnpo NAME,PARTTYPE`).
    pub fn partitions_of_type(&self, kind: &str) -> Vec<String> {
        self.output("lsblk", &["-rnpo", "NAME,PARTTYPE"], true)
            .lines()
            .filter_map(|line| line.split_once(' '))
            .filter(|(_, found)| found.eq_ignore_ascii_case(kind))
            .map(|(name, _)| name.to_owned())
            .collect()
    }

    /// The size of `device` as `lsblk -dno SIZE` prints it, without spaces,
    /// such as `512M`; empty when it cannot be read.
    pub fn size(&self, device: &str) -> String {
        self.output("lsblk", &["-dno", "SIZE", device], true)
            .replace(' ', "")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use zbm_testkit::FakeRunner;

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
        let devices = Devices::new(&runner);

        assert_eq!(devices.find("LABEL", "key"), "/dev/sdb1");
        assert_eq!(devices.luks_devices(), "/dev/sda2\n/dev/sdc");
        assert_eq!(devices.content_type("/dev/mapper/a"), "zfs_member");
        assert_eq!(devices.tag("/dev/sda2", "LABEL"), "");
        assert_eq!(devices.size("/dev/sda2"), "7.5G");
        let esp = devices.partitions_of_type("c12a7328-f81f-11d2-ba4b-00a0c93ec93b");
        assert_eq!(esp, ["/dev/sda1"]);
        let failing = FakeRunner::new();
        failing.on(|_| Some(Err(std::io::Error::other("no blkid"))));
        assert_eq!(Devices::new(&failing).find("UUID", "x"), "");
        runner.respond("blkid -t TYPE=suspend", 2, "");
        assert!(devices.has_type("swsuspend"));
        assert!(!devices.has_type("suspend"));
    }
}
