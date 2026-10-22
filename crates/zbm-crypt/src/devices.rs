//! Probing block devices: `blkid` (with no cache, `-c /dev/null`, so it
//! always reads the devices) and `lsblk`.
//!
//! Each probe returns what the program printed without its final newlines,
//! as the shell's `$(...)` does, and an empty answer when the program fails.

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
        let devices = Devices::new(&runner);

        assert_eq!(devices.find("LABEL", "key"), "/dev/sdb1");
        assert_eq!(devices.luks_devices(), "/dev/sda2\n/dev/sdc");
        assert_eq!(devices.content_type("/dev/mapper/a"), "zfs_member");
        assert_eq!(devices.tag("/dev/sda2", "LABEL"), "");
        assert_eq!(devices.size("/dev/sda2"), "7.5G");
        let failing = FakeRunner::new();
        failing.on(|_| Some(Err(std::io::Error::other("no blkid"))));
        assert_eq!(Devices::new(&failing).find("UUID", "x"), "");
    }
}
