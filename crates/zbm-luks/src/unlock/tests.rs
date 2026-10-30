//! Tests of unlocking and closing, on a fake machine with LUKS volumes.

use crate::mappings::Origin;
use std::path::Path;

use zbm_platform::exec::{Command, Input, Output, Status};
use zbm_platform::files::Files;
use zbm_testkit::assert_no_secret_leaked;
use zbm_testkit::fakes::{ConsoleInput, FakeSystem};
use zbm_testkit::machine::{FakeMachine, HOLDERS, LuksVolume};

use crate::device::OpenCheck;
use crate::error::LuksError;
use crate::testing::{Prompts, log, warnings};
use zbm_types::{DevicePath, MappingName};

/// The mapping name `name`, or the default one when it is empty.
fn named(name: &str) -> Option<MappingName> {
    (!name.is_empty()).then(|| name.parse().unwrap())
}

/// The device at `path`.
fn device(path: &str) -> DevicePath {
    path.parse().unwrap()
}
use crate::unlock::{Method, Opened, UnlockSummary};
use crate::unlocker::Answer;
use crate::unlocker::{Luks, Settings, Unlocker};

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

/// Unlocks every volume, for [`with_session`].
fn unlock_all(session: &mut Unlocker<'_>) -> Result<UnlockSummary, LuksError> {
    session.unlock_all()
}

fn with_session<T>(
    fake: &FakeSystem,
    settings: &Settings,
    prompts: &Prompts,
    body: impl FnOnce(&mut Unlocker<'_>) -> T,
) -> T {
    let _log = log(fake);
    let luks = Luks {
        context: fake.context(),
        settings,
        interaction: prompts,
    };
    body(&mut Unlocker::new(luks))
}

fn mappings(fake: &FakeSystem) -> String {
    fake.files
        .text("/zfsbootmenu/luks-mappings")
        .unwrap_or_default()
}

/// How each volume was opened: `luks-unlocked`.
fn unlocked(fake: &FakeSystem) -> String {
    fake.files
        .text("/zfsbootmenu/luks-unlocked")
        .unwrap_or_default()
}

fn infos(fake: &FakeSystem) -> Vec<String> {
    fake.kmsg()
        .lines()
        .filter_map(|line| line.strip_prefix("<6>ZFSBootMenu: "))
        .map(str::to_owned)
        .collect()
}

mod attended;
mod close;

#[test]
fn discovery_shares_passphrases_and_closes_volumes_without_zfs() {
    let fake = machine(&[A, B, C], &[]);
    fake.console.secret(b"secret1").secret(b"other");
    let prompts = Prompts::new(&fake);
    let summary = with_session(&fake, &settings(), &prompts, unlock_all);
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
    // Typed: a person opened them, the one closed since too.
    assert_eq!(
        unlocked(&fake),
        format!(
            "luks-{}\tattended\nluks-{}\tattended\nluks-{}\tattended\n",
            A.uuid, B.uuid, C.uuid
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
    let prompts = Prompts::new(&fake);
    assert_eq!(
        with_session(&fake, &settings(), &prompts, unlock_all),
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
        with_session(&fake, &retry, &prompts, unlock_all),
        Err(LuksError::NothingNew)
    );
    assert!(fake.console.remaining().is_empty());
    assert!(fake.kmsg().contains("no LUKS volumes found"));
}

#[test]
fn the_end_of_the_input_fails_gracefully() {
    let fake = machine(&[A], &[]);
    fake.console.push(ConsoleInput::End);
    let prompts = Prompts::new(&fake);
    assert_eq!(
        with_session(&fake, &settings(), &prompts, unlock_all),
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
    let prompts = Prompts::answering(&fake, &[Answer::Continue]);
    assert_eq!(
        with_session(&fake, &settings, &prompts, unlock_all),
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
        with_session(&fake, &once, &Prompts::new(&fake), unlock_all),
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
    fake.console.on_screen(move |_| {
        files.file("/fake/holders/loop1", "elsewhere\n");
    });
    let prompts = Prompts::new(&fake);
    let mut settings = settings();
    settings.mappings = "/zfsbootmenu/luks-mappings".into();
    assert_eq!(
        with_session(&fake, &settings, &prompts, |session| {
            session.open_device(
                named("v").as_ref(),
                &device("/dev/loop1"),
                "",
                "",
                Origin::Configured,
            )
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
    let prompts = Prompts::new(&fake);
    assert!(with_session(&fake, &settings(), &prompts, unlock_all).is_ok());
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
fn a_crypttab_name_that_is_no_mapping_name_is_not_opened() {
    // cryptsetup would read the name as an option.
    let fake = machine(&[A, B], &[]);
    fake.files.file(
        "/etc/zfsbootmenu/crypttab",
        "--test-passphrase /dev/loop1 none\npool-b /dev/loop2 none\n",
    );
    fake.console.secret(b"secret1");
    let prompts = Prompts::new(&fake);
    let summary = with_session(&fake, &settings(), &prompts, unlock_all).unwrap();
    assert_eq!((summary.unlocked, summary.failed), (1, 1));
    assert_eq!(mappings(&fake), "pool-b\t/dev/loop2\t0\n");
    assert!(fake.kmsg().contains("not unlocking a crypttab entry"));
}

#[test]
fn missing_devices_are_asked_about_and_duplicates_are_skipped() {
    let fake = machine(&[A], &[]);
    fake.files.file(
        "/etc/zfsbootmenu/crypttab",
        format!("a1 /dev/loop1\na2 UUID={}\nm LABEL=missing\n", A.uuid),
    );
    fake.console.secret(b"secret1");
    let prompts = Prompts::new(&fake);
    // One unlocked, one failed: still a success.
    assert!(with_session(&fake, &settings(), &prompts, unlock_all).is_ok());
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
    let prompts = Prompts::new(&fake);
    assert_eq!(
        with_session(&fake, &settings(), &prompts, |session| {
            session.open_device(
                named("ks-a").as_ref(),
                &device("/dev/loop1"),
                "",
                "keyscript=/etc/ks",
                Origin::Configured,
            )
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
    // It may ask on the console, which a front end hands it.
    assert_eq!(prompts.consoles(), 1);
    assert_eq!(mappings(&fake), "ks-a\t/dev/loop1\t0\n");
    // A key script needs nobody.
    assert_eq!(unlocked(&fake), "ks-a\tunattended\n");
    fake.assert_no_leak(b"secret1");
}

#[test]
fn volumes_in_use_and_taken_names_are_left_alone() {
    let fake = machine(&[A, C], &[]);
    fake.files.file("/fake/holders/loop1", "x\n");
    fake.files.block_device("/dev/mapper/taken");
    let prompts = Prompts::new(&fake);
    with_session(&fake, &settings(), &prompts, |session| {
        assert_eq!(
            session.open_device(
                named("").as_ref(),
                &device("/dev/loop1"),
                "",
                "",
                Origin::Configured
            ),
            Ok(Opened::AlreadyOpen)
        );
        assert_eq!(
            session.open_device(
                named("taken").as_ref(),
                &device("/dev/loop3"),
                "",
                "",
                Origin::Configured
            ),
            Err(LuksError::MappingExists {
                name: "taken".to_owned()
            })
        );
        assert_eq!(
            session.open_device(
                named("").as_ref(),
                &device("/dev/sdz"),
                "",
                "",
                Origin::Configured
            ),
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
    let prompts = Prompts::new(&fake);
    let unlock =
        |fake: &FakeSystem, settings: Settings| with_session(fake, &settings, &prompts, unlock_all);
    let mut retry = settings();
    retry.retry = true;
    assert_eq!(unlock(&fake, settings()), Ok(UnlockSummary::default()));
    assert_eq!(unlock(&fake, retry.clone()), Err(LuksError::NothingNew));
    let fake = FakeSystem::new();
    assert_eq!(unlock(&fake, settings()), Ok(UnlockSummary::default()));
    assert_eq!(unlock(&fake, retry), Err(LuksError::NothingNew));
}
