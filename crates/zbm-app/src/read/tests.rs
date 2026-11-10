use pretty_assertions::assert_eq;
use zbm_config::args::ZbmArgs;
use zbm_luks::unlocker::{Luks, Settings};
use zbm_platform::interaction::{
    Answer, Asked, Interaction, NoInteraction, PromptRequest, SecretRequest, Tell, Wait,
    WaitRequest,
};
use zbm_testkit::fakes::FakeSystem;
use zbm_testkit::zfs::FakeZfs;

use super::*;

/// A prompter for code that must not show a timed prompt, and asks nothing
/// else of anyone.
pub(crate) struct NoPrompts;

impl Interaction for NoPrompts {
    fn ask(&self, request: &PromptRequest) -> Answer {
        panic!("unexpected timed prompt {request:?}");
    }

    fn secret(&self, request: &SecretRequest<'_>) -> Asked {
        NoInteraction.secret(request)
    }

    fn wait(&self, request: &WaitRequest<'_>) -> Wait {
        NoInteraction.wait(request)
    }

    fn notify(&self, tell: Tell<'_>) {
        NoInteraction.notify(tell);
    }
}

#[test]
fn environments_carry_their_command_line_and_where_it_was_set() {
    let zfs = FakeZfs::new()
        .pool("zroot")
        .filesystem(
            "zroot/ROOT",
            &[(COMMAND_LINE, "quiet"), ("mountpoint", "none")],
        )
        .filesystem(
            "zroot/ROOT/void",
            &[("mountpoint", "/"), ("keystatus", "available")],
        )
        .filesystem(
            "zroot/ROOT/arch",
            &[("mountpoint", "/"), (COMMAND_LINE, "ro loglevel=4")],
        )
        .filesystem(
            "zroot/ROOT/old",
            &[("mountpoint", "/"), ("canmount", "off")],
        )
        .bootfs("zroot", "zroot/ROOT/void")
        .snapshot("zroot/ROOT/arch@old", 100)
        .snapshot("zroot/ROOT/arch@new", 200)
        .snapshot("zroot/ROOT@all", 150);

    let environments = environments(&zfs, None).unwrap();

    let names: Vec<&str> = environments.iter().map(|be| be.name.as_str()).collect();
    assert_eq!(names, ["zroot/ROOT/arch", "zroot/ROOT/void"]);
    let (arch, void) = (&environments[0], &environments[1]);
    assert!(void.is_default && !arch.is_default);
    assert_eq!(void.encryption, Encryption::Unlocked);
    assert_eq!(arch.encryption, Encryption::None);
    assert_eq!(arch.command_line.value.get(), "ro loglevel=4");
    assert_eq!(arch.command_line.origin, Origin::Local);
    assert_eq!(void.command_line.value.get(), "quiet");
    assert_eq!(
        void.command_line.origin,
        Origin::Inherited("zroot/ROOT".parse().unwrap())
    );
    assert!(void.kernels.is_empty() && void.snapshot_count == 0);
    assert!(void.newest_snapshot.is_none());
    assert_eq!(arch.snapshot_count, 2);
    let newest = arch.newest_snapshot.as_ref().unwrap();
    assert_eq!((newest.name.name(), newest.used), ("new", 4096));
    assert!(is_environment(&zfs, &"zroot/ROOT/arch".parse().unwrap()));
    assert!(!is_environment(&zfs, &"zroot/ROOT/old".parse().unwrap()));
    assert!(!is_environment(&zfs, &"zroot/ROOT".parse().unwrap()));
    assert!(!is_environment(&zfs, &"zroot/ROOT/gone".parse().unwrap()));
    let none = command_line(&Lines::new(&zfs), &"zroot/ROOT/gone".parse().unwrap());
    assert_eq!(
        (none.value.get().as_str(), none.origin),
        ("", Origin::Default)
    );
    assert_eq!(origin(&Source::Temporary), Origin::Local);
    assert_eq!(origin(&Source::Inherited(String::new())), Origin::Default);
}

#[test]
fn an_inherited_line_is_read_with_its_parent_expanded() {
    let zfs = FakeZfs::new()
        .pool("zroot")
        .filesystem("zroot", &[(COMMAND_LINE, "spl_hostid=1")])
        .filesystem(
            "zroot/ROOT",
            &[(COMMAND_LINE, "%{parent} quiet"), ("mountpoint", "none")],
        )
        .filesystem("zroot/ROOT/void", &[("mountpoint", "/")])
        .filesystem("zroot/ROOT/ubuntu", &[("mountpoint", "/")])
        .filesystem(
            "zroot/ROOT/arch",
            &[("mountpoint", "/"), (COMMAND_LINE, "ro %{parent}")],
        )
        .filesystem(
            "zroot/ROOT/plain",
            &[("mountpoint", "/"), (COMMAND_LINE, "rw")],
        );

    let environments = environments(&zfs, None).unwrap();

    let line = |name: &str| {
        let found = environments.iter().find(|e| e.name.as_str() == name);
        let line = &found.unwrap().command_line;
        (line.value.get().as_str(), line.expanded.get().as_str())
    };
    // An inherited line is its ancestor's expansion: the parent's
    // arguments once, not `quiet` again.
    assert_eq!(
        line("zroot/ROOT/void"),
        ("%{parent} quiet", "spl_hostid=1 quiet")
    );
    assert_eq!(
        line("zroot/ROOT/ubuntu"),
        ("%{parent} quiet", "spl_hostid=1 quiet")
    );
    assert_eq!(
        line("zroot/ROOT/arch"),
        ("ro %{parent}", "ro spl_hostid=1 quiet")
    );
    assert_eq!(line("zroot/ROOT/plain"), ("rw", "rw"));
    // One property read per dataset, whichever environments expand it.
    let reads: Vec<String> = zfs
        .calls()
        .into_iter()
        .filter(|call| call.starts_with("property "))
        .collect();
    let datasets = [
        "zroot/ROOT/arch",
        "zroot/ROOT",
        "zroot",
        "zroot/ROOT/plain",
        "zroot/ROOT/ubuntu",
        "zroot/ROOT/void",
    ];
    let expected: Vec<String> = datasets
        .iter()
        .map(|dataset| format!("property {dataset} {COMMAND_LINE}"))
        .collect();
    assert_eq!(reads, expected);
}

#[test]
fn a_parent_whose_line_is_unset_expands_to_nothing() {
    let zfs = FakeZfs::new().pool("zroot").filesystem(
        "zroot/ROOT/void",
        &[("mountpoint", "/"), (COMMAND_LINE, "%{parent} quiet")],
    );

    let line = command_line(&Lines::new(&zfs), &"zroot/ROOT/void".parse().unwrap());

    // `%{parent}` stands for nothing, as when booting: the unset line of
    // zroot/ROOT (`-`) ends the expansion before zroot.
    assert_eq!(line.expanded.get(), " quiet");
    let read = |dataset: &str| format!("property {dataset} {COMMAND_LINE}");
    assert_eq!(zfs.calls(), [read("zroot/ROOT/void"), read("zroot/ROOT")]);
}

#[test]
fn a_name_that_is_no_dataset_s_has_no_command_line() {
    let zfs = FakeZfs::new().pool("zroot");

    assert_eq!(Lines::new(&zfs).commandline("zroot/ROOT/../x"), None);
    assert!(zfs.calls().is_empty());
}

/// LUKS2 metadata with a TPM2 token that needs a PIN and a Clevis binding.
const METADATA: &str = r#"{"keyslots": {}, "tokens": {
  "0": {"type": "systemd-tpm2", "keyslots": ["1"], "tpm2-pin": true},
  "1": {"type": "clevis", "keyslots": ["2"]},
  "2": {"type": "systemd-tpm2", "keyslots": ["3"]}
}}"#;

#[test]
fn volumes_say_their_mapping_state_and_methods() {
    let fake = FakeSystem::new();
    fake.runner.respond(
        "blkid -c /dev/null -o device -t TYPE=crypto_LUKS",
        0,
        "/dev/vda2\n/dev/vdb\n/dev/vdc\n",
    );
    fake.runner.respond(
        "cryptsetup luksDump --dump-json-metadata /dev/vda2",
        0,
        METADATA,
    );
    fake.files
        .block_device("/dev/vda2")
        .block_device("/dev/vdb")
        .block_device("/dev/vdc")
        .file("/sys/class/block/vda2/holders/dm-0", "")
        .file("/zfsbootmenu/luks-mappings", "zroot-a\t/dev/vda2\t\n")
        .file("/zfsbootmenu/luks-skipped", "/dev/vdc\n");
    let settings = Settings::new("/zfsbootmenu");
    let session = Unlocker::new(Luks {
        context: fake.context(),
        settings: &settings,
        interaction: &NoPrompts,
    });

    let volumes = volumes(&session);

    assert_eq!(volumes.len(), 3);
    let (vda2, vdb) = (&volumes[0], &volumes[1]);
    assert_eq!(volumes[2].state, VolumeState::Skipped);
    assert_eq!(
        (vda2.name.get().as_str(), vda2.device.get().as_str()),
        ("zroot-a", "/dev/vda2")
    );
    assert_eq!(vda2.state, VolumeState::Open);
    assert_eq!(
        vda2.methods,
        [
            Method::Tpm2Pin,
            Method::Clevis,
            Method::Tpm2,
            Method::Passphrase
        ]
    );
    assert_eq!(vdb.name.get(), "/dev/vdb");
    assert_eq!(vdb.state, VolumeState::Locked);
    assert_eq!(vdb.methods, [Method::Passphrase]);
    assert_eq!(token_method("systemd-fido2", false), Some(Method::Fido2));
    assert_eq!(token_method("systemd-recovery", false), None);
}

#[test]
fn the_countdown_follows_zbm_timeout() {
    let timeout_of = |kcl: &str| {
        let (options, _) = ZbmOptions::parse(&ZbmArgs::from_cmdline(kcl));
        timeout(&options)
    };
    assert_eq!(timeout_of("zbm.timeout=5"), Timeout::Seconds(5));
    assert_eq!(timeout_of("zbm.timeout=0"), Timeout::Skip);
    assert_eq!(timeout_of("zbm.timeout=-1"), Timeout::Wait);
    assert_eq!(timeout_of("zbm.show"), Timeout::Wait);
    let (mut options, _) = ZbmOptions::parse(&ZbmArgs::from_cmdline(""));
    options.menu_timeout = zbm_types::Timeout::Seconds(u64::MAX);
    assert_eq!(timeout(&options), Timeout::Wait);
}

#[test]
fn the_keyboard_puts_the_console_layout_first() {
    let fake = FakeSystem::new();
    fake.files
        .file("/layouts/layouts", "us\nde\n\nfr\n")
        .file("/zfsbootmenu/console/layout", "fr\n");
    let state = State::new(&fake.files, "/zfsbootmenu");

    let local = keyboard(&fake.files, Path::new("/layouts"), &state, "tty1");
    let serial = keyboard(&fake.files, Path::new("/layouts"), &state, "/dev/ttyS0");

    assert_eq!(local.layouts, ["fr", "us", "de"]);
    assert!(local.local && !serial.local);
    let none = keyboard(
        &fake.files,
        Path::new("/none"),
        &State::new(&fake.files, "/x"),
        "",
    );
    assert!(none.layouts.is_empty() && none.local);
}

#[test]
fn the_keyboard_has_each_layouts_positions_from_its_keymap() {
    // Compiled from the machine's XKB data (`xkb-data`), as the image build
    // compiles them.
    let compiled = |name: &str| {
        zbm_display::layout::compile::keymap(Path::new(zbm_display::layout::compile::XKB), name)
            .unwrap()
    };
    let fake = FakeSystem::new();
    fake.files
        .file("/layouts/layouts", "us\nde\nfr\n")
        .file("/layouts/us.xkb", compiled("us"))
        .file("/layouts/de.xkb", compiled("de"))
        .file("/layouts/fr.xkb", "not a keymap");
    let state = State::new(&fake.files, "/zfsbootmenu");

    let keyboard = keyboard(&fake.files, Path::new("/layouts"), &state, "tty1");

    let names: Vec<&str> = keyboard.positions.keys().map(String::as_str).collect();
    assert_eq!(names, ["de", "us"]);
    let typed: String = "Y[rich"
        .chars()
        .map(|c| keyboard.positions["us"].translate(&keyboard.positions["de"], c))
        .collect();
    assert_eq!(typed, "Zürich");
}

#[test]
fn the_log_holds_the_warnings_and_errors_of_the_journal() {
    let fake = FakeSystem::new();
    fake.files.file(
        "/zfsbootmenu/journal.jsonl",
        "{\"level\":\"INFO\",\"message\":\"started\"}\n\
         {\"level\":\"WARN\",\"message\":\"slow \\\"device\\\"\"}\n\
         not json\n\
         {\"level\":\"ERROR\",\"message\":\"import failed\"}\n\
         {\"level\":\"ERROR\"}\n",
    );

    let log = log(&fake.files, Path::new("/zfsbootmenu"));

    let entries: Vec<(Level, &str)> = log
        .entries
        .iter()
        .map(|entry| (entry.level, entry.text.get().as_str()))
        .collect();
    assert_eq!(
        entries,
        [
            (Level::Warning, "slow \"device\""),
            (Level::Error, "import failed"),
            (Level::Error, ""),
        ]
    );
    assert_eq!(log.count(Level::Error), 2);
    assert!(log_is_empty(&fake, "/elsewhere"));
}

fn log_is_empty(fake: &FakeSystem, base: &str) -> bool {
    log(&fake.files, Path::new(base)).entries.is_empty()
}

#[test]
#[should_panic(expected = "unexpected timed prompt")]
fn no_prompt_may_be_shown_while_reading() {
    let request = PromptRequest {
        subject: zbm_platform::interaction::Subject::Missing(String::new()),
        delay: None,
        message: String::new(),
        on_return: String::new(),
        on_escape: String::new(),
    };
    let _ = NoPrompts.ask(&request);
}

#[test]
fn unset_and_default_properties_have_the_default_origin() {
    let zfs = FakeZfs::new().pool("tank").filesystem(
        "tank/ROOT/be",
        &[("mountpoint", "/"), ("keystatus", "unavailable")],
    );

    let environments = environments(&zfs, None).unwrap();

    assert_eq!(environments[0].encryption, Encryption::Locked);
    assert_eq!(environments[0].command_line.origin, Origin::Default);
    assert_eq!(origin(&Source::Default), Origin::Default);
}

#[test]
fn the_stages_are_done_once_the_state_directory_records_them() {
    let fake = FakeSystem::new();
    fake.files.dir("/zfsbootmenu");
    let state = State::new(&fake.files, "/zfsbootmenu");
    state.finish_stage("10-kmods").unwrap();

    let stages = startup(&state);

    assert_eq!(stages.len(), zbm_types::Stage::ALL.len());
    assert_eq!(stages[0].name, "Loading kernel modules");
    assert_eq!(stages[0].state, crate::model::StageState::Done);
    assert!(
        stages[1..]
            .iter()
            .all(|stage| stage.state == crate::model::StageState::Pending)
    );
    let names: std::collections::BTreeSet<&str> = zbm_types::Stage::ALL
        .iter()
        .map(|stage| stage_name(*stage))
        .collect();
    assert_eq!(names.len(), zbm_types::Stage::ALL.len());
}

#[test]
fn the_zfs_version_is_the_first_line_the_module_gives() {
    let fake = FakeSystem::new();
    fake.files
        .file("/sys/module/zfs/version", "2.2.6-1\x1b[2J\nsecond\n");

    let version = zfs_version(&fake.files).unwrap();

    assert_eq!(version.get(), "2.2.6-1\x1b[2J");
    assert_eq!(version.printable().to_string(), "2.2.6-1\\x1b[2J");
}

#[test]
fn without_the_module_s_version_file_the_zfs_version_is_unknown() {
    let fake = FakeSystem::new();
    assert_eq!(zfs_version(&fake.files), None);
    fake.files.file("/sys/module/zfs/version", " \n2.2.6\n");
    assert_eq!(zfs_version(&fake.files), None);
}

#[test]
fn the_host_is_the_host_id_zfs_uses() {
    let fake = FakeSystem::new();
    fake.files.dir("/zfsbootmenu");
    let state = State::new(&fake.files, "/zfsbootmenu");
    assert_eq!(host(&fake.files, &state), None);

    state.set_spl_hostid("00bab10c").unwrap();
    assert_eq!(host(&fake.files, &state), HostId::from_hex("00bab10c"));
    fake.files.file("/etc/hostid", [0xef, 0xbe, 0xad, 0xde]);
    assert_eq!(host(&fake.files, &state), HostId::from_hex("deadbeef"));
    fake.files
        .file("/sys/module/spl/parameters/spl_hostid", "3054399729\n");
    assert_eq!(host(&fake.files, &state), HostId::from_hex("b60e70f1"));
}
