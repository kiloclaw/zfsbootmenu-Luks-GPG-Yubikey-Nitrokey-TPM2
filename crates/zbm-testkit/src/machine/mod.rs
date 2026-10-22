//! A fake machine: disks, LUKS volumes and pools that the fake programs
//! answer for, built in one expression.
//!
//! ```
//! use zbm_testkit::machine::FakeMachine;
//! use zbm_testkit::{assert_no_secret_leaked, assert_ran};
//! use zbm_platform::exec::{Command, Input, Runner};
//! use zbm_platform::secret::Secret;
//!
//! let machine = FakeMachine::new()
//!     .with_luks_volume("/dev/sda2", "pw")
//!     .with_pool("tank", "/dev/mapper/cryptroot");
//!
//! let open = Command::new("cryptsetup")
//!     .args(["open", "--type", "luks", "--key-file=-"])
//!     .args(["/dev/sda2", "cryptroot"])
//!     .stdin(Input::Secret(Secret::copy_from(b"pw")));
//! assert!(machine.runner.run(&open).unwrap().status.is_success());
//!
//! assert_ran!(
//!     machine,
//!     "cryptsetup open --type luks --key-file=- /dev/sda2 cryptroot"
//! );
//! assert!(machine.is_open("cryptroot"));
//! assert_no_secret_leaked!(machine, b"pw");
//! ```
//!
//! What the programs do, by program:
//!
//! - `cryptsetup isLuks`, `luksUUID`, `luksDump --dump-json-metadata`, `open`
//!   (the key on standard input with `--key-file=-`, or from a key file of
//!   the machine) and `close`. An open volume is a block device below
//!   `/dev/mapper` and a marker in [`HOLDERS`] naming its mapping, which
//!   stands for the kernel's `holders` directory.
//! - `blkid`: the LUKS devices for `-t TYPE=crypto_LUKS`, and with `-p` what
//!   an open volume contains ([`LuksVolume::content`]).
//! - `lsblk`: every device is `1G`.
//! - `zpool list` names the pools; `zpool status POOL` shows the vdev.
//!
//! Every other command succeeds with no output, as [`FakeRunner`] answers
//! it; a test adds responders for others through `machine.runner`.

mod programs;

use std::cell::RefCell;
use std::ops::Deref;
use std::path::Path;
use std::rc::Rc;

use zbm_platform::files::Files as _;

use crate::FakeRunner;
use crate::fakes::FakeSystem;

/// Where an open volume leaves a marker named after its device, holding its
/// mapping name: what the kernel shows in `/sys/block/DEVICE/holders`.
pub const HOLDERS: &str = "/fake/holders";

/// Where an open volume's content type is kept, by mapping name.
const MAPS: &str = "/fake/maps";

/// Where a test marks a mapping busy, so that closing it fails.
pub const BUSY: &str = "/fake/busy";

/// The directory of the machine's programs, on `PATH`.
pub const BIN: &str = "/sbin";

/// A LUKS volume of a [`FakeMachine`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LuksVolume {
    /// The block device, such as `/dev/sda2`.
    pub device: String,
    /// The LUKS UUID.
    pub uuid: String,
    /// The passphrase that opens it.
    pub passphrase: Vec<u8>,
    /// What `blkid -p` finds in it once open: `zfs_member` unless set.
    pub content: String,
    /// What `cryptsetup luksDump --dump-json-metadata` prints; `None` makes
    /// it fail, as an old `cryptsetup` does.
    pub metadata: Option<String>,
}

impl LuksVolume {
    /// A volume on `device` that `passphrase` opens, with a UUID made from
    /// the device's name.
    pub fn new(device: &str, passphrase: impl AsRef<[u8]>) -> LuksVolume {
        LuksVolume {
            device: device.to_owned(),
            uuid: uuid_for(device),
            passphrase: passphrase.as_ref().to_vec(),
            content: "zfs_member".to_owned(),
            metadata: None,
        }
    }

    /// The same volume with another UUID.
    #[must_use]
    pub fn uuid(mut self, uuid: &str) -> LuksVolume {
        uuid.clone_into(&mut self.uuid);
        self
    }

    /// The same volume, holding `content` (a `blkid` type such as `ext4`).
    #[must_use]
    pub fn content(mut self, content: &str) -> LuksVolume {
        content.clone_into(&mut self.content);
        self
    }

    /// The same volume, whose header's JSON metadata is `json`.
    #[must_use]
    pub fn metadata(mut self, json: &str) -> LuksVolume {
        self.metadata = Some(json.to_owned());
        self
    }
}

/// A UUID that depends only on `device`, so tests can predict it.
fn uuid_for(device: &str) -> String {
    let hash = device
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3)
        });
    let hex = format!("{hash:016x}{:016x}", hash.rotate_left(29));
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// A pool of a [`FakeMachine`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Pool {
    /// The pool's name.
    pub name: String,
    /// The device its one vdev is on.
    pub vdev: String,
}

/// What the fake programs know about the machine.
#[derive(Debug, Default)]
struct Model {
    /// The LUKS volumes.
    volumes: Vec<LuksVolume>,
    /// The pools.
    pools: Vec<Pool>,
}

/// A [`FakeSystem`] with a model of the machine's disks that its programs
/// answer for. It dereferences to the [`FakeSystem`], so `machine.files`,
/// `machine.console` and `machine.system()` work as on one.
pub struct FakeMachine {
    /// The fakes.
    system: FakeSystem,
    /// The disks, shared with the responders.
    model: Rc<RefCell<Model>>,
}

impl Default for FakeMachine {
    fn default() -> FakeMachine {
        FakeMachine::new()
    }
}

impl Deref for FakeMachine {
    type Target = FakeSystem;

    fn deref(&self) -> &FakeSystem {
        &self.system
    }
}

impl FakeMachine {
    /// A machine with no disks: `PATH` is [`BIN`], which holds `cryptsetup`,
    /// `blkid`, `zpool` and `zfs`, and `/dev/mapper`, [`HOLDERS`] and the state
    /// directory `/zfsbootmenu` exist.
    pub fn new() -> FakeMachine {
        let system = FakeSystem::new();
        system.environment.set("PATH", BIN);
        for program in ["cryptsetup", "blkid", "zpool", "zfs"] {
            system
                .files
                .file_with_mode(format!("{BIN}/{program}"), "", 0o755);
        }
        system
            .files
            .dir("/dev/mapper")
            .dir(HOLDERS)
            .dir(MAPS)
            .dir("/zfsbootmenu");
        let model = Rc::new(RefCell::new(Model::default()));
        programs::install(&system.runner, &system.files, &model);
        FakeMachine { system, model }
    }

    /// The same machine with a LUKS volume on `device` that `passphrase`
    /// opens.
    #[must_use]
    pub fn with_luks_volume(self, device: &str, passphrase: impl AsRef<[u8]>) -> FakeMachine {
        self.with_luks(LuksVolume::new(device, passphrase))
    }

    /// The same machine with `volume`: its device, and a link to it in
    /// `/dev/disk/by-uuid`.
    #[must_use]
    pub fn with_luks(self, volume: LuksVolume) -> FakeMachine {
        self.files.block_device(&volume.device);
        self.files.symlink(
            format!("/dev/disk/by-uuid/{}", volume.uuid),
            format!("../../{}", base(&volume.device)),
        );
        self.model.borrow_mut().volumes.push(volume);
        self
    }

    /// The same machine with the pool `name` on the device `vdev`.
    #[must_use]
    pub fn with_pool(self, name: &str, vdev: &str) -> FakeMachine {
        self.model.borrow_mut().pools.push(Pool {
            name: name.to_owned(),
            vdev: vdev.to_owned(),
        });
        self
    }

    /// The same machine with a file at `path`, such as a key file.
    #[must_use]
    pub fn with_file(self, path: &str, contents: impl AsRef<[u8]>) -> FakeMachine {
        self.files.file(path, contents);
        self
    }

    /// The same machine, booted with the kernel command line `line`.
    #[must_use]
    pub fn with_kernel_cmdline(self, line: &str) -> FakeMachine {
        self.files.file("/proc/cmdline", format!("{line}\n"));
        self
    }

    /// The same machine, with someone typing `secret` and Enter at the next
    /// prompt that hides its input.
    #[must_use]
    pub fn typing(self, secret: impl AsRef<[u8]>) -> FakeMachine {
        self.console.secret(secret.as_ref());
        self
    }

    /// The LUKS volume on `device`.
    ///
    /// # Panics
    ///
    /// If the machine has none; this is a mistake in the test.
    pub fn volume(&self, device: &str) -> LuksVolume {
        let model = self.model.borrow();
        let volume = model.volumes.iter().find(|volume| volume.device == device);
        volume
            .cloned()
            .unwrap_or_else(|| panic!("the fake machine has no LUKS volume on {device}"))
    }

    /// Whether a volume is open as `name`.
    pub fn is_open(&self, name: &str) -> bool {
        self.files.exists(Path::new(&format!("/dev/mapper/{name}")))
    }

    /// Marks the mapping `name` busy, so that closing it fails.
    pub fn hold_open(&self, name: &str) {
        self.files.file(format!("{BUSY}/{name}"), "");
    }

    /// Opens `device` as `name` behind the code's back, as `zluks` over SSH
    /// would.
    pub fn open_elsewhere(&self, device: &str, name: &str) {
        programs::open(&self.files, &self.volume(device), name);
    }

    /// The fake programs' runner.
    pub fn runner(&self) -> &FakeRunner {
        &self.system.runner
    }
}

/// The last component of `path`.
fn base(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or_default()
}

#[cfg(test)]
mod tests;
