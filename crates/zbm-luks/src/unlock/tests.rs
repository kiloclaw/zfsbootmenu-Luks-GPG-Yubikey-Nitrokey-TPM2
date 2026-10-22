//! Tests of unlocking and closing, on a fake machine with LUKS volumes.

use std::path::Path;

use zbm_platform::exec::{Command, Input, Output, Status};
use zbm_platform::files::Files;
use zbm_testkit::assert_no_secret_leaked;
use zbm_testkit::fakes::{ConsoleInput, FakeSystem};
use zbm_testkit::machine::{FakeMachine, HOLDERS, LuksVolume};

use crate::device::OpenCheck;
use crate::error::LuksError;
use crate::session::Answer;
use crate::session::{Luks, Session, Settings};
use crate::testing::{Prompts, log, warnings};
use crate::unlock::{Method, Opened, UnlockSummary};

/// A LUKS volume of the fake machine.
#[derive(Clone, Copy)]
struct Volume {
    device: &'static str,
    uuid: &'static str,
    passphrase: &'static [u8],
    /// What `blkid -p` finds in the open volume.
    content: &'static str,
}

const A: Volume = Volume {
    device: "/dev/loop1",
    uuid: "0f2c8bb6-2d5e-4f87-9a6b-9a7c1fef8d1a",
    passphrase: b"secret1",
    content: "zfs_member",
};
const B: Volume = Volume {
    device: "/dev/loop2",
    uuid: "0f2c8bb6-2d5e-4f87-9a6b-9a7c1fef8d1b",
    passphrase: b"secret1",
    content: "zfs_member",
};
const C: Volume = Volume {
    device: "/dev/loop3",
    uuid: "0f2c8bb6-2d5e-4f87-9a6b-9a7c1fef8d1c",
    passphrase: b"other",
    content: "ext4",
};

fn args(command: &Command) -> Vec<String> {
    command
        .get_args()
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

/// A machine with `volumes` and pools on mapping paths.
fn machine(volumes: &[Volume], pools: &[(&str, &str)]) -> FakeMachine {
    let mut machine = FakeMachine::new().with_file("/usr/lib/udev/rules.d/95-dm-notify.rules", "");
    for volume in volumes {
        machine = machine.with_luks(
            LuksVolume::new(volume.device, volume.passphrase)
                .uuid(volume.uuid)
                .content(volume.content),
        );
    }
    for (pool, vdev) in pools {
        machine = machine.with_pool(pool, vdev);
    }
    machine
}

fn settings() -> Settings {
    let mut settings = Settings::new("/zfsbootmenu");
    settings.open_check = OpenCheck::Markers(HOLDERS.into());
    settings.colors = false;
    settings
}

fn with_session<T>(
    fake: &FakeSystem,
    settings: Settings,
    prompts: &Prompts,
    body: impl FnOnce(&mut Session<'_>) -> T,
) -> T {
    let _log = log(fake);
    let luks = Luks {
        system: fake.system(),
        settings: &settings,
        prompter: prompts,
    };
    body(&mut Session::new(luks))
}

fn mappings(fake: &FakeSystem) -> String {
    fake.files
        .text("/zfsbootmenu/luks-mappings")
        .unwrap_or_default()
}

fn infos(fake: &FakeSystem) -> Vec<String> {
    fake.kmsg()
        .lines()
        .filter_map(|line| line.strip_prefix("<6>ZFSBootMenu: "))
        .map(str::to_owned)
        .collect()
}

#[test]
fn discovery_shares_passphrases_and_closes_volumes_without_zfs() {
    let fake = machine(&[A, B, C], &[]);
    fake.console.secret(b"secret1").secret(b"other");
    let prompts = Prompts::default();
    let summary = with_session(&fake, settings(), &prompts, |session| session.unlock_all());
    assert_eq!(
        summary,
        Ok(UnlockSummary {
            unlocked: 2,
            failed: 0
        })
    );
    assert!(
        fake.files
            .exists(Path::new(&format!("/dev/mapper/luks-{}", A.uuid)))
    );
    assert!(
        fake.files
            .exists(Path::new(&format!("/dev/mapper/luks-{}", B.uuid)))
    );
    assert!(
        !fake
            .files
            .exists(Path::new(&format!("/dev/mapper/luks-{}", C.uuid)))
    );
    assert_eq!(
        mappings(&fake),
        format!(
            "luks-{}\t/dev/loop1\t1\nluks-{}\t/dev/loop2\t1\n",
            A.uuid, B.uuid
        )
    );
    let output = fake.console.output_text();
    assert_eq!(
        output.matches("Passphrase (leave empty to skip): ").count(),
        2
    );
    assert!(output.contains("Unlock LUKS volume /dev/loop1 (1G)"));
    let infos = infos(&fake);
    assert!(
        infos.contains(&"discovered LUKS volumes: /dev/loop1 /dev/loop2 /dev/loop3".to_owned())
    );
    assert!(infos.contains(&format!(
        "unlocked /dev/loop2 as luks-{} with a previously entered passphrase",
        B.uuid
    )));
    assert!(infos.contains(&format!(
        "closing luks-{}: /dev/loop3 does not contain a ZFS vdev",
        C.uuid
    )));
    assert_eq!(
        infos.last().map(String::as_str),
        Some("LUKS: 2 volume(s) available, 0 not unlocked")
    );
    assert_no_secret_leaked!(fake, b"secret1", b"other");
}

#[test]
fn empty_passphrases_skip_and_retries_leave_skipped_volumes_alone() {
    let fake = machine(&[A], &[]);
    fake.console.secret(b"");
    let prompts = Prompts::default();
    assert_eq!(
        with_session(&fake, settings(), &prompts, |session| session.unlock_all()),
        Err(LuksError::NoneUnlocked { failed: 1 })
    );
    assert_eq!(
        fake.files.text("/zfsbootmenu/luks-skipped").as_deref(),
        Some("/dev/loop1\n")
    );
    assert!(infos(&fake).contains(&"skipping LUKS volume /dev/loop1 at user request".to_owned()));

    // The retry of the import loop finds nothing new.
    let mut retry = settings();
    retry.retry = true;
    assert_eq!(
        with_session(&fake, retry, &prompts, |session| session.unlock_all()),
        Err(LuksError::NothingNew)
    );
    assert!(fake.console.remaining().is_empty());
    assert!(fake.kmsg().contains("no LUKS volumes found"));
}

#[test]
fn the_end_of_the_input_fails_gracefully() {
    let fake = machine(&[A], &[]);
    fake.console.push(ConsoleInput::End);
    let prompts = Prompts::default();
    assert_eq!(
        with_session(&fake, settings(), &prompts, |session| session.unlock_all()),
        Err(LuksError::NoneUnlocked { failed: 1 })
    );
    assert_eq!(
        warnings(&fake),
        ["no console available to unlock /dev/loop1"]
    );
    // Echo comes back on.
    assert_eq!(fake.console.echo_changes(), [false, true]);
}

#[test]
fn wrong_passphrases_end_in_a_retry_prompt() {
    let fake = machine(&[A], &[]);
    fake.console
        .secret(b"bad1")
        .secret(b"bad2")
        .secret(b"secret1");
    let mut settings = settings();
    settings.default_tries = 2;
    let prompts = Prompts::answering(&[Answer::Continue]);
    assert_eq!(
        with_session(&fake, settings, &prompts, |session| session.unlock_all()),
        Ok(UnlockSummary {
            unlocked: 1,
            failed: 0
        })
    );
    assert_eq!(
        prompts.messages(),
        ["Unable to unlock LUKS volume /dev/loop1 (1G)"]
    );
    let output = fake.console.output_text();
    assert_eq!(
        output
            .matches("No key available with this passphrase.")
            .count(),
        1
    );
    assert_eq!(
        warnings(&fake),
        [
            "failed to unlock /dev/loop1 (attempt 1)",
            "failed to unlock /dev/loop1 (attempt 2)"
        ]
    );

    // ESCAPE gives up and marks the volume skipped.
    let fake = machine(&[A], &[]);
    fake.console.secret(b"bad1");
    let mut once = self::settings();
    once.default_tries = 1;
    assert_eq!(
        with_session(&fake, once, &Prompts::default(), |session| session
            .unlock_all()),
        Err(LuksError::NoneUnlocked { failed: 1 })
    );
    assert!(warnings(&fake).contains(&"giving up on LUKS volume /dev/loop1".to_owned()));
    assert!(fake.files.exists(Path::new("/zfsbootmenu/luks-skipped")));
}

#[test]
fn volumes_unlocked_elsewhere_end_the_prompt() {
    let fake = machine(&[A], &[]);
    fake.console.timeout();
    // The second poll finds the volume open, as zluks over SSH would leave it.
    let files = fake.files.clone();
    fake.runner.on(move |command| {
        (command.program() == "tput").then(|| {
            files.file("/fake/holders/loop1", "elsewhere\n");
            Ok(Output::new(Status::exited(0), ""))
        })
    });
    let prompts = Prompts::default();
    let mut settings = settings();
    settings.mappings = "/zfsbootmenu/luks-mappings".into();
    assert_eq!(
        with_session(&fake, settings, &prompts, |session| {
            session.open_device("v", "/dev/loop1", "", "", false)
        }),
        Ok(Opened::Elsewhere)
    );
    assert!(infos(&fake).contains(&"/dev/loop1 was unlocked elsewhere".to_owned()));
    assert_eq!(mappings(&fake), "");
}

#[test]
fn crypttab_entries_use_key_files_options_and_names() {
    let fake = machine(&[A, B, C], &[]);
    fake.files.file("/etc/plainkey", "secret1");
    fake.files.file(
        "/etc/zfsbootmenu/crypttab",
        format!(
            "# name device key options\n\
             pool-a UUID={} /etc/plainkey discard,no-read-workqueue\n\
             pool-b /dev/loop2 none tries=2\n\
             ext-c /dev/loop3 - noauto\n\
             gone PARTLABEL=nothere none nofail\n",
            A.uuid
        ),
    );
    fake.console.secret(b"wrong").secret(b"secret1");
    let prompts = Prompts::default();
    assert!(with_session(&fake, settings(), &prompts, |session| session.unlock_all()).is_ok());
    assert_eq!(
        mappings(&fake),
        "pool-a\t/dev/loop1\t0\npool-b\t/dev/loop2\t0\n"
    );
    let opens: Vec<String> = fake
        .runner
        .lines()
        .into_iter()
        .filter(|line| line.starts_with("cryptsetup open"))
        .collect();
    assert_eq!(
        opens[0],
        "cryptsetup open --type luks --key-file=/etc/plainkey --allow-discards \
         --perf-no_read_workqueue /dev/loop1 pool-a"
    );
    // The key file's passphrase is not kept, so pool-b asks.
    assert_eq!(
        fake.console
            .output_text()
            .matches("Passphrase (leave empty")
            .count(),
        2
    );
    assert!(prompts.messages().is_empty());
    assert!(
        infos(&fake)
            .contains(&"optional LUKS device PARTLABEL=nothere not found, skipping".to_owned())
    );
    assert!(fake.kmsg().contains("not unlocking /dev/loop3 (noauto)"));
}

#[test]
fn missing_devices_are_asked_about_and_duplicates_are_skipped() {
    let fake = machine(&[A], &[]);
    fake.files.file(
        "/etc/zfsbootmenu/crypttab",
        format!("a1 /dev/loop1\na2 UUID={}\nm LABEL=missing\n", A.uuid),
    );
    fake.console.secret(b"secret1");
    let prompts = Prompts::default();
    // One unlocked, one failed: still a success.
    assert!(with_session(&fake, settings(), &prompts, |session| session.unlock_all()).is_ok());
    assert_eq!(prompts.messages(), ["LUKS device LABEL=missing is missing"]);
    assert!(
        warnings(&fake).contains(&"LUKS device /dev/loop1 is configured more than once".to_owned())
    );
    assert!(fake.kmsg().contains("LUKS device LABEL=missing not found"));
    assert!(infos(&fake).contains(&"LUKS: 1 volume(s) available, 1 not unlocked".to_owned()));
}

#[test]
fn key_scripts_come_first() {
    let fake = machine(&[A], &[]);
    fake.files.file_with_mode("/etc/ks", "", 0o755);
    fake.runner.on(|command| {
        (command.program() == "/etc/ks").then(|| {
            Ok(Output::with_secret(
                Status::exited(0),
                zbm_platform::secret::Secret::copy_from(b"secret1"),
            ))
        })
    });
    let prompts = Prompts::default();
    assert_eq!(
        with_session(&fake, settings(), &prompts, |session| {
            session.open_device("ks-a", "/dev/loop1", "", "keyscript=/etc/ks", false)
        }),
        Ok(Opened::Unlocked(Method::KeyScript))
    );
    let script = fake
        .runner
        .calls()
        .into_iter()
        .find(|call| call.program() == "/etc/ks")
        .unwrap();
    assert_eq!(args(&script), ["none"]);
    let env: Vec<String> = script
        .get_env()
        .iter()
        .map(|(key, value)| {
            format!(
                "{}={}",
                key.to_string_lossy(),
                value.as_deref().unwrap_or_default().to_string_lossy()
            )
        })
        .collect();
    assert_eq!(
        env,
        [
            "CRYPTTAB_NAME=ks-a",
            "CRYPTTAB_SOURCE=/dev/loop1",
            "CRYPTTAB_KEY=none",
            "CRYPTTAB_OPTIONS=keyscript=/etc/ks",
            "CRYPTTAB_TRIED=0",
        ]
    );
    assert_eq!(*script.get_stdin(), Input::Inherit);
    assert_eq!(mappings(&fake), "ks-a\t/dev/loop1\t0\n");
    fake.assert_no_leak(b"secret1");
}

#[test]
fn volumes_in_use_and_taken_names_are_left_alone() {
    let fake = machine(&[A, C], &[]);
    fake.files.file("/fake/holders/loop1", "x\n");
    fake.files.block_device("/dev/mapper/taken");
    let prompts = Prompts::default();
    with_session(&fake, settings(), &prompts, |session| {
        assert_eq!(
            session.open_device("", "/dev/loop1", "", "", false),
            Ok(Opened::AlreadyOpen)
        );
        assert_eq!(
            session.open_device("taken", "/dev/loop3", "", "", false),
            Err(LuksError::MappingExists {
                name: "taken".to_owned()
            })
        );
        assert_eq!(
            session.open_device("", "/dev/sdz", "", "", false),
            Err(LuksError::NotLuks {
                header: "/dev/sdz".to_owned()
            })
        );
    });
    assert!(
        infos(&fake).contains(&"LUKS device /dev/loop1 is already in use, skipping".to_owned())
    );
    assert!(
        fake.kmsg()
            .contains("cannot unlock /dev/loop3: mapping taken already exists")
    );
    assert!(
        warnings(&fake)
            .contains(&"/dev/sdz is not a LUKS volume (header), skipping /dev/sdz".to_owned())
    );
}

#[test]
fn disabled_or_unavailable_unlocking_fails_only_retries() {
    let fake = machine(&[A], &[]);
    fake.files.file("/zfsbootmenu/cmdline.d/zbm.luks", "off\n");
    let prompts = Prompts::default();
    let unlock = |fake: &FakeSystem, settings: Settings| {
        with_session(fake, settings, &prompts, |session| session.unlock_all())
    };
    let mut retry = settings();
    retry.retry = true;
    assert_eq!(unlock(&fake, settings()), Ok(UnlockSummary::default()));
    assert_eq!(unlock(&fake, retry.clone()), Err(LuksError::NothingNew));
    let fake = FakeSystem::new();
    assert_eq!(unlock(&fake, settings()), Ok(UnlockSummary::default()));
    assert_eq!(unlock(&fake, retry), Err(LuksError::NothingNew));
}

#[test]
fn closing_exports_pools_first_and_keeps_busy_mappings() {
    let fake = machine(
        &[A, B],
        &[("zroot", "/dev/mapper/a1"), ("other", "/dev/vda")],
    );
    fake.console.secret(b"secret1");
    fake.files.file(
        "/etc/zfsbootmenu/crypttab",
        "a1 /dev/loop1 none\nb1 /dev/loop2 none\n",
    );
    let prompts = Prompts::default();
    assert!(with_session(&fake, settings(), &prompts, |session| session.unlock_all()).is_ok());
    fake.files.file("/fake/busy/b1", "");
    assert_eq!(
        with_session(&fake, settings(), &prompts, |session| session
            .close_all(None)),
        Err(LuksError::NotClosed {
            names: vec!["b1".to_owned()]
        })
    );
    let lines = fake.runner.lines();
    let export = lines
        .iter()
        .position(|line| line == "zpool export zroot")
        .unwrap();
    let close = lines
        .iter()
        .position(|line| line == "cryptsetup close a1")
        .unwrap();
    assert!(export < close);
    assert!(!lines.contains(&"zpool export other".to_owned()));
    assert_eq!(mappings(&fake), "b1\t/dev/loop2\t0\n");
    assert!(infos(&fake).contains(&"closed LUKS mapping a1".to_owned()));
    assert!(warnings(&fake).contains(&"unable to close LUKS mapping b1".to_owned()));

    // Only the named mapping is closed.
    let _ = fake.files.remove_file(Path::new("/fake/busy/b1"));
    let close = |only| {
        with_session(&fake, settings(), &prompts, |session| {
            session.close_all(only)
        })
    };
    assert_eq!(close(Some("a1")), Ok(()));
    assert_eq!(mappings(&fake), "b1\t/dev/loop2\t0\n");
    assert_eq!(close(Some("b1")), Ok(()));
    assert_eq!(mappings(&fake), "");
}
