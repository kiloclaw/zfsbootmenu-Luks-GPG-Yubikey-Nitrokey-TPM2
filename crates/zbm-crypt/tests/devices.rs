//! `zbm-crypt` against real LUKS volumes on loop devices.
//!
//! These tests need root, `losetup`, `cryptsetup` and a kernel with
//! device-mapper, so they run only when `ZBM_DEVICE_TESTS=1` is set and the
//! kernel has device-mapper, and say so otherwise:
//!
//! ```sh
//! sudo env ZBM_DEVICE_TESTS=1 cargo test -p zbm-crypt --test devices
//! ```
//!
//! Each test formats its own volume in a file below the target directory and
//! detaches its loop device when it ends, even when it fails.

#![allow(
    clippy::disallowed_macros,
    clippy::disallowed_methods,
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "device tests set up real loop devices and fail loudly when they cannot"
)]

use std::path::PathBuf;
use std::process::{Command as Process, Stdio};

use zbm_crypt::{Closed, CryptError, Cryptsetup, Key, LuksVolumes, Mapping};
use zbm_platform::{Secret, SystemRunner};

/// The passphrase of the test volumes.
const KEY: &[u8] = b"pilot passphrase";

/// Whether the device tests should run; prints why not when they should not.
fn enabled() -> bool {
    if std::env::var_os("ZBM_DEVICE_TESTS").is_none_or(|value| value != "1") {
        eprintln!("skipped: set ZBM_DEVICE_TESTS=1 (needs root, losetup, cryptsetup)");
        return false;
    }
    // The kernel lists device-mapper among its misc devices when it has it.
    let misc = std::fs::read_to_string("/proc/misc").unwrap_or_default();
    if !misc.contains("device-mapper") {
        eprintln!("skipped: this kernel has no device-mapper");
        return false;
    }
    true
}

/// A LUKS2 volume in a file, attached to a loop device until dropped.
struct LoopVolume {
    file: PathBuf,
    device: String,
}

impl LoopVolume {
    /// Formats a 32 MiB volume named after `test`, with [`KEY`].
    fn new(test: &str) -> LoopVolume {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        let file = dir.join(format!("{test}-{}.img", std::process::id()));
        std::fs::File::create(&file)
            .and_then(|f| f.set_len(32 << 20))
            .expect("create the volume file");
        let device = run("losetup", &["--find", "--show", path_str(&file)], None);
        let volume = LoopVolume {
            file,
            device: device.trim().to_owned(),
        };
        run(
            "cryptsetup",
            &[
                "luksFormat",
                "--batch-mode",
                "--type",
                "luks2",
                "--pbkdf",
                "pbkdf2",
                "--pbkdf-force-iterations",
                "1000",
                "--key-file=-",
                &volume.device,
            ],
            Some(KEY),
        );
        volume
    }

    /// The volume closed, to be opened as `name`.
    fn mapping(&self, name: &str) -> Mapping<Closed> {
        Mapping::new(self.device.parse().unwrap(), name.parse().unwrap())
    }
}

impl Drop for LoopVolume {
    fn drop(&mut self) {
        let _ = Process::new("losetup").args(["-d", &self.device]).status();
        let _ = std::fs::remove_file(&self.file);
    }
}

fn path_str(path: &std::path::Path) -> &str {
    path.to_str().expect("UTF-8 path")
}

/// Runs a set-up program, with `stdin` if given, and returns its output.
fn run(program: &str, args: &[&str], stdin: Option<&[u8]>) -> String {
    let mut child = Process::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("{program}: {error}"));
    if let Some(bytes) = stdin {
        std::io::Write::write_all(child.stdin.as_mut().unwrap(), bytes).unwrap();
    }
    drop(child.stdin.take());
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{program} {args:?} failed");
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn the_right_key_opens_the_volume_and_close_removes_the_mapping() {
    if !enabled() {
        return;
    }
    let volume = LoopVolume::new("open-close");
    let name = format!("zbm-pilot-{}", std::process::id());
    let cryptsetup = Cryptsetup::new(&SystemRunner);

    let open = cryptsetup
        .open(
            volume.mapping(&name),
            Key::Secret(&Secret::copy_from(KEY)),
            &[],
        )
        .expect("open with the right key");

    assert!(std::path::Path::new(&open.path()).exists());
    let closed = cryptsetup.close(open).expect("close");
    assert!(!std::path::Path::new(&closed.name().mapper_path()).exists());
}

#[test]
fn a_wrong_key_is_reported_as_such() {
    if !enabled() {
        return;
    }
    let volume = LoopVolume::new("wrong-key");
    let name = format!("zbm-pilot-wrong-{}", std::process::id());

    let failed = Cryptsetup::new(&SystemRunner)
        .open(
            volume.mapping(&name),
            Key::Secret(&Secret::copy_from(b"not it")),
            &[],
        )
        .expect_err("a wrong key must not open the volume");

    assert!(
        matches!(failed.error, CryptError::WrongKey { .. }),
        "{failed}"
    );
    assert!(!std::path::Path::new(&format!("/dev/mapper/{name}")).exists());
}
