//! `zbm report`: a bug report for an issue (ROADMAP.md #9), with no key
//! material ([`zbm_boot::report`]).
//!
//! - `zbm report` writes it on the partition `zbm.save_logs` names, or the
//!   EFI system partition, below `EFI/zfsbootmenu/reports`, mounting the
//!   partition only for the write;
//! - `zbm report DIR` writes it in the directory `DIR`, such as a mounted USB
//!   stick;
//! - `zbm report PARTITION` writes it on another partition, named as a
//!   device or as `LABEL=`, `UUID=`, `PARTLABEL=` or `PARTUUID=`.

use std::path::Path;

use zbm_boot::report;
use zbm_boot::saved_logs::Destination;
use zbm_platform::{NoConsole, RealSystem, System};

use crate::debug::BASE;
use crate::{Io, USAGE};

/// The usage text.
const USAGE_TEXT: &str = "\
Usage: zbm report              write a report on the EFI system partition
       zbm report DIR          write it in the directory DIR
       zbm report PARTITION    write it on PARTITION (a device, or LABEL=...)
";

/// `zbm report` on `system`, with the state directory `base`.
pub fn run_in(system: &System<'_>, base: &Path, args: &[String], io: &mut Io) -> i32 {
    let written = match args {
        [] => {
            let destination = crate::saved::destination(system.files, base);
            report::write_on(system, base, &destination.unwrap_or(Destination::Esp))
                .map(|(device, dir)| format!("{dir} on {device}"))
        }
        [dir] if system.files.is_dir(Path::new(dir)) => {
            report::write(system, base, Path::new(dir)).map(|dir| dir.display().to_string())
        }
        [partition] if !partition.starts_with('-') => {
            report::write_on(system, base, &Destination::Device(partition.clone()))
                .map(|(device, dir)| format!("{dir} on {device}"))
        }
        _ => {
            let _ = io.stderr.write_all(USAGE_TEXT.as_bytes());
            return USAGE;
        }
    };
    match written {
        Ok(place) => {
            let _ = writeln!(io.stdout, "report written in {place}");
            0
        }
        Err(error) => {
            let _ = writeln!(io.stderr, "zbm report: {}", zbm_types::code::report(&error));
            1
        }
    }
}

/// `zbm report`.
pub fn run(args: &[String], io: &mut Io) -> i32 {
    let real = RealSystem::new();
    run_in(&real.with_console(&NoConsole), Path::new(BASE), args, io)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use zbm_platform::Files as _;
    use zbm_testkit::fakes::FakeSystem;

    use super::*;

    /// A machine with one ESP and a journal.
    fn machine() -> FakeSystem {
        let fake = FakeSystem::new();
        fake.runner.respond(
            "lsblk -rnpo NAME,PARTTYPE",
            0,
            "/dev/vda1 c12a7328-f81f-11d2-ba4b-00a0c93ec93b\n",
        );
        fake.runner.respond(
            "blkid -c /dev/null -l -o device -t LABEL=USB",
            0,
            "/dev/sdb1\n",
        );
        fake.files
            .file("/zfsbootmenu/journal.jsonl", "{}\n")
            .dir("/media/usb");
        fake
    }

    fn call(fake: &FakeSystem, args: &[&str]) -> (i32, String, String) {
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let mut io = Io {
            stdout: &mut stdout,
            stderr: &mut stderr,
        };
        let code = run_in(&fake.system(), Path::new(BASE), &args, &mut io);
        let text = |bytes: Vec<u8>| String::from_utf8(bytes).unwrap();
        (code, text(stdout), text(stderr))
    }

    #[test]
    fn a_report_goes_to_the_esp_a_directory_or_a_partition() {
        let fake = machine();
        let name = "zbm-report-2026-01-01T00-00-00Z";

        let esp = call(&fake, &[]);
        let usb = call(&fake, &["/media/usb"]);
        let partition = call(&fake, &["LABEL=USB"]);

        let on = |device: &str| {
            let text = format!("report written in EFI/zfsbootmenu/reports/{name} on {device}\n");
            (0, text, String::new())
        };
        assert_eq!(esp, on("/dev/vda1"));
        assert_eq!(usb.1, format!("report written in /media/usb/{name}\n"));
        assert!(
            fake.files
                .exists(&Path::new("/media/usb").join(name).join("report.txt"))
        );
        assert_eq!(partition, on("/dev/sdb1"));
        let lines = fake.runner.lines();
        let mounted = "mount -t vfat -o rw,noatime /dev/sdb1 /run/zbm-saved-logs";
        assert!(lines.contains(&mounted.to_owned()), "{lines:?}");
    }

    #[test]
    fn wrong_usage_and_failures_are_said_so() {
        let fake = FakeSystem::new();

        assert_eq!(
            call(&fake, &["--help"]),
            (USAGE, String::new(), USAGE_TEXT.to_owned())
        );
        assert_eq!(call(&fake, &["a", "b"]).0, USAGE);
        assert_eq!(
            call(&fake, &[]),
            (
                1,
                String::new(),
                "zbm report: ZBM-E0621: no EFI system partition found\n".to_owned()
            )
        );
    }
}
