//! Tests of loading keys: each step of the order on a fake machine whose
//! key status changes as keys load.

use std::path::{Path, PathBuf};
use zbm_testkit::Shared;

use zbm_platform::KernelLog;
use zbm_platform::exec::{Output, Status};
use zbm_state::State;
use zbm_testkit::fakes::FakeSystem;

use super::*;
use crate::testing::name;

const HOOKS: &str = "/libexec/hooks";
const BE: &str = "tank/ROOT/be";

/// Which roots are unlocked, the `zfs load-key` locations that unlock each
/// root, and the key location of the key source (`prompt` when empty).
#[derive(Default)]
struct Keys {
    unlocked: Vec<String>,
    loads: Vec<(String, String)>,
    source_location: String,
}

/// A pool `tank` with the boot environment `tank/ROOT/be`, encrypted below
/// `tank/ROOT`, and the encrypted key source `tank/keys`, whose key is asked
/// for.
struct Machine {
    fake: FakeSystem,
    keys: Shared<Keys>,
}

fn output(stdout: &str) -> Output {
    Output::new(Status::exited(0), stdout.as_bytes())
}

impl Machine {
    fn new(keylocation: &str, keyformat: &str, keysource: &str) -> Machine {
        let fake = FakeSystem::new();
        let keys = Shared::new(Keys::default());
        let shared = keys.clone();
        fake.runner.on(move |command| {
            let line = command.display();
            if let Some(root) = line.strip_prefix("zfs get -H -o value keystatus ") {
                let unlocked = shared.borrow().unlocked.iter().any(|done| done == root);
                return Some(Ok(output(if unlocked {
                    "available\n"
                } else {
                    "unavailable\n"
                })));
            }
            if line == "zfs get -H -o value keylocation tank/keys" {
                let location = shared.borrow().source_location.clone();
                let location = if location.is_empty() {
                    "prompt".into()
                } else {
                    location
                };
                return Some(Ok(output(&format!("{location}\n"))));
            }
            let rest = line.strip_prefix("zfs load-key -L ")?;
            let (location, root) = rest.split_once(' ')?;
            let mut keys = shared.borrow_mut();
            let loads = keys.loads.iter().any(|(l, r)| l == location && r == root);
            if loads {
                keys.unlocked.push(root.to_owned());
            }
            Some(Ok(Output::new(Status::exited(u8::from(!loads)), "")))
        });
        let runner = &fake.runner;
        runner.respond("zpool list -H -o feature@encryption tank", 0, "active\n");
        for (fs, root) in [(BE, "tank/ROOT"), ("tank/keys", "tank/keys")] {
            let line = format!("zfs get -H -o value encryptionroot {fs}");
            runner.respond(&line, 0, &format!("{root}\n"));
        }
        let property = |name: &str, fs: &str, value: &str| {
            let line = format!("zfs get -H -o value {name} {fs}");
            runner.respond(&line, 0, &format!("{value}\n"));
        };
        property("keylocation", "tank/ROOT", keylocation);
        property("keyformat", "tank/ROOT", keyformat);
        property("org.zfsbootmenu:keysource", BE, keysource);
        runner.respond("zfs list -o name -H tank/keys", 0, "tank/keys\n");
        fake.files.dir("/zfsbootmenu");
        Machine { fake, keys }
    }

    /// `zfs load-key -L <location> <root>` unlocks `root`.
    fn loads(&self, location: &str, root: &str) -> &Machine {
        let load = (location.to_owned(), root.to_owned());
        self.keys.borrow_mut().loads.push(load);
        self
    }

    fn load(&self, path: BootPath, clear: Clear) -> Result<(), BootError> {
        self.load_fs(BE, path, clear)
    }

    fn load_fs(&self, fs: &str, path: BootPath, clear: Clear) -> Result<(), BootError> {
        let system = self.fake.system();
        let state = State::new(&self.fake.files, "/zfsbootmenu");
        let _log = KernelLog::new(&self.fake.files, "7", Path::new("/zfsbootmenu"), 1).install();
        let how = KeyLoad {
            hooks: Path::new(HOOKS),
            path,
            clear,
        };
        load_key(&system, &state, &how, &name(fs))
    }

    fn ran(&self, line: &str) -> bool {
        self.fake.runner.lines().iter().any(|ran| ran == line)
    }

    /// Whether the screen was cleared.
    fn cleared(&self) -> bool {
        let clear = zbm_platform::console::Screen::Clear;
        self.fake.console.screen_count(clear) > 0
    }

    fn ran_program(&self, program: &str) -> bool {
        self.fake
            .runner
            .lines()
            .iter()
            .any(|ran| ran.starts_with(program))
    }

    /// Where `tank/keys` is mounted.
    fn source_mount(&self) -> PathBuf {
        State::new(&self.fake.files, "/zfsbootmenu").mount_point("tank/keys")
    }
}

fn not_loaded(root: &str) -> Result<(), BootError> {
    Err(BootError::KeyNotLoaded {
        root: root.to_owned(),
    })
}

#[test]
fn an_unlocked_root_and_an_unencrypted_filesystem_need_nothing() {
    let machine = Machine::new("prompt", "passphrase", "-");
    machine.keys.borrow_mut().unlocked.push("tank/ROOT".into());
    assert_eq!(machine.load(BootPath::Countdown, Clear::No), Ok(()));
    assert!(!machine.ran_program("zfs load-key"));

    let plain = FakeSystem::new();
    plain
        .runner
        .respond("zpool list -H -o feature@encryption tank", 0, "disabled\n");
    let state = State::new(&plain.files, "/zfsbootmenu");
    let how = KeyLoad {
        hooks: Path::new(HOOKS),
        path: BootPath::Countdown,
        clear: Clear::No,
    };
    assert_eq!(load_key(&plain.system(), &state, &how, &name(BE)), Ok(()));
}

#[test]
fn a_prompt_location_asks_for_the_passphrase() {
    let machine = Machine::new("prompt", "passphrase", "-");
    machine.loads("prompt", "tank/ROOT");
    assert_eq!(machine.load(BootPath::Countdown, Clear::Yes), Ok(()));
    assert!(machine.cleared());
    assert!(machine.ran("zfs load-key -L prompt tank/ROOT"));

    let wrong = Machine::new("prompt", "passphrase", "-");
    assert_eq!(
        wrong.load(BootPath::Countdown, Clear::No),
        not_loaded("tank/ROOT")
    );
    assert!(!wrong.cleared());
}

#[test]
fn load_key_hooks_come_first_and_may_unlock_the_root() {
    let machine = Machine::new("prompt", "passphrase", "-");
    let hook = "/libexec/hooks/load-key.d/10-key";
    machine.fake.files.file_with_mode(hook, "", 0o755);
    let keys = machine.keys.clone();
    machine.fake.runner.on(move |command| {
        (command.program() == hook).then(|| {
            let env = command.get_env();
            let has = |key: &str, value: &str| {
                env.iter()
                    .any(|(k, v)| k == key && v.as_deref() == Some(value.as_ref()))
            };
            if has("ZBM_LOCKED_FS", BE) && has("ZBM_ENCRYPTION_ROOT", "tank/ROOT") {
                keys.borrow_mut().unlocked.push("tank/ROOT".into());
            }
            Ok(Output::new(Status::exited(0), ""))
        })
    });
    assert_eq!(machine.load(BootPath::Countdown, Clear::No), Ok(()));
    assert!(machine.ran(hook));
    assert!(!machine.ran_program("zfs load-key"));
    assert!(machine.fake.kmsg().contains("unlocked by user hooks"));
}

#[test]
fn a_hook_that_leaves_the_root_locked_falls_back_to_the_prompt() {
    let machine = Machine::new("prompt", "passphrase", "-");
    let hook = "/libexec/hooks/load-key.d/10-key";
    machine.fake.files.file_with_mode(hook, "", 0o755);
    machine.loads("prompt", "tank/ROOT");
    assert_eq!(machine.load(BootPath::Countdown, Clear::No), Ok(()));
    assert!(machine.ran(hook));
    assert!(machine.ran("zfs load-key -L prompt tank/ROOT"));
}

#[test]
fn a_key_file_in_the_image_is_loaded() {
    let machine = Machine::new("file:///etc/zfs/tank.key", "raw", "-");
    machine.fake.files.file("/etc/zfs/tank.key", "key");
    machine.loads("file:///etc/zfs/tank.key", "tank/ROOT");
    assert_eq!(machine.load(BootPath::Countdown, Clear::No), Ok(()));
    assert!(!machine.ran_program("zfs load-key -L prompt"));
}

#[test]
fn without_a_key_file_only_a_passphrase_is_asked_for() {
    // The key file does not load: the passphrase prompt.
    let passphrase = Machine::new("file:///etc/zfs/tank.key", "passphrase", "-");
    passphrase.fake.files.file("/etc/zfs/tank.key", "wrong");
    passphrase.loads("prompt", "tank/ROOT");
    assert_eq!(passphrase.load(BootPath::Countdown, Clear::Yes), Ok(()));
    assert!(passphrase.ran("zfs load-key -L file:///etc/zfs/tank.key tank/ROOT"));
    assert!(passphrase.cleared());

    // A raw key without its file cannot be loaded.
    let raw = Machine::new("file:///etc/zfs/tank.key", "raw", "-");
    assert_eq!(
        raw.load(BootPath::Countdown, Clear::No),
        not_loaded("tank/ROOT")
    );
    assert!(!raw.ran_program("zfs load-key"));
    assert!(
        raw.fake
            .kmsg()
            .contains("unable to load key with format raw")
    );
}

#[test]
fn a_boot_chosen_in_the_menu_copies_the_key_from_its_key_source() {
    let machine = Machine::new("file:///etc/zfs/keys/tank.key", "raw", "tank/keys");
    let cached = "/zfsbootmenu/.keys/tank/keys/etc/zfs/keys/tank.key";
    machine
        .loads("prompt", "tank/keys")
        .loads(&format!("file://{cached}"), "tank/ROOT");
    machine.fake.runner.respond(
        "zfs get -H -o value mountpoint tank/keys",
        0,
        "/etc/zfs/keys\n",
    );
    let mnt = machine.source_mount();
    machine.fake.files.file(mnt.join("tank.key"), "secret");

    assert_eq!(machine.load(BootPath::Menu, Clear::No), Ok(()));
    assert_eq!(machine.fake.files.text(cached).as_deref(), Some("secret"));
    let mode = zbm_platform::Files::metadata(&machine.fake.files, std::path::Path::new(cached))
        .unwrap()
        .mode;
    assert_eq!(mode, 0o600, "the cached key is private");
    assert!(machine.ran(&format!("umount {}", mnt.display())));
    let mutexes = machine.fake.files.paths();
    assert!(
        !mutexes
            .iter()
            .any(|path| path.to_string_lossy().contains("cachemutex"))
    );
    assert!(
        machine
            .fake
            .kmsg()
            .contains("caching key from mount-relative path")
    );
}

#[test]
fn the_countdown_does_not_try_key_sources() {
    let machine = Machine::new("file:///etc/zfs/keys/tank.key", "passphrase", "tank/keys");
    machine.loads("prompt", "tank/ROOT");
    assert_eq!(machine.load(BootPath::Countdown, Clear::No), Ok(()));
    assert!(!machine.ran_program("mount"));
    assert!(!machine.ran("zfs load-key -L prompt tank/keys"));
}

#[test]
fn a_key_source_whose_key_is_in_itself_is_tried_once() {
    let machine = Machine::new("file:///etc/zfs/keys/tank.key", "passphrase", "tank/keys");
    machine.keys.borrow_mut().source_location = "file:///etc/zfs/keys/keys.key".into();
    let runner = &machine.fake.runner;
    runner.respond(
        "zfs get -H -o value org.zfsbootmenu:keysource tank/keys",
        0,
        "tank/keys\n",
    );
    runner.respond("zfs get -H -o value keyformat tank/keys", 0, "passphrase\n");
    machine.loads("prompt", "tank/keys");
    let mnt = machine.source_mount();
    machine
        .fake
        .files
        .file(mnt.join("etc/zfs/keys/tank.key"), "secret");
    machine.loads(
        "file:///zfsbootmenu/.keys/tank/keys/etc/zfs/keys/tank.key",
        "tank/ROOT",
    );

    assert_eq!(machine.load(BootPath::Menu, Clear::No), Ok(()));
    let kmsg = machine.fake.kmsg();
    let repeat = "will not repeat cache attempt for tank/keys:etc/zfs/keys/keys.key";
    assert!(kmsg.contains(repeat));
    assert!(kmsg.contains("caching key from absolute path"));
}

#[test]
fn a_key_source_that_fails_leaves_the_passphrase_prompt() {
    // Its key does not load.
    let locked = Machine::new("file:///etc/zfs/keys/tank.key", "passphrase", "tank/keys");
    locked.loads("prompt", "tank/ROOT");
    assert_eq!(locked.load(BootPath::Menu, Clear::No), Ok(()));
    assert!(
        locked
            .fake
            .kmsg()
            .contains("failed to load key for tank/keys")
    );
    assert!(!locked.ran_program("mount"));

    // The key file is not in it, or leads out of it.
    let missing = Machine::new("file:///etc/zfs/keys/tank.key", "passphrase", "tank/keys");
    missing
        .loads("prompt", "tank/keys")
        .loads("prompt", "tank/ROOT");
    let mnt = missing.source_mount();
    missing.fake.files.file("/etc/secret", "not a key");
    missing
        .fake
        .files
        .symlink(mnt.join("etc/zfs/keys/tank.key"), "/etc/secret");
    assert_eq!(missing.load(BootPath::Menu, Clear::No), Ok(()));
    assert!(
        missing
            .fake
            .kmsg()
            .contains("is not a proper child of tank/keys")
    );
    assert!(missing.ran(&format!("umount {}", mnt.display())));
    assert!(
        !missing
            .fake
            .files
            .paths()
            .iter()
            .any(|p| p.starts_with("/zfsbootmenu/.keys/tank/keys/etc"))
    );
}

#[test]
fn a_key_source_that_unlocks_the_root_itself_is_enough() {
    // Loading the key source's key unlocks the boot environment too, as
    // when they share an encryption root.
    let machine = Machine::new("file:///etc/zfs/keys/tank.key", "raw", "tank/keys");
    machine.loads("prompt", "tank/keys");
    let keys = machine.keys.clone();
    machine.fake.runner.on(move |command| {
        command.display().starts_with("mount ").then(|| {
            keys.borrow_mut().unlocked.push("tank/ROOT".into());
            Ok(Output::new(Status::exited(0), ""))
        })
    });
    assert_eq!(machine.load(BootPath::Menu, Clear::No), Ok(()));
    assert!(
        machine
            .fake
            .kmsg()
            .contains("cache attempt has unlocked tank/ROOT")
    );
    assert!(
        machine
            .load_fs("tank/keys", BootPath::Menu, Clear::No)
            .is_ok()
    );
}
