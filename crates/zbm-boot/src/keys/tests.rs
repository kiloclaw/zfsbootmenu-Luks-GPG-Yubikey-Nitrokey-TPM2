use zbm_testkit::fakes::FakeSystem;

use super::*;
use crate::testing::name;

/// A machine whose boot environment `tank/ROOT/be` is encrypted below
/// `tank/ROOT` with a passphrase, with the key location and key source given.
fn machine(keylocation: &str, keysource: &str) -> FakeSystem {
    with_format(keylocation, keysource, "passphrase")
}

/// The same, with the key format given.
fn with_format(keylocation: &str, keysource: &str, keyformat: &str) -> FakeSystem {
    let fake = FakeSystem::new();
    let runner = &fake.runner;
    runner.respond("zpool list -H -o feature@encryption tank", 0, "active\n");
    runner.respond(
        "zfs get -H -o value encryptionroot tank/ROOT/be",
        0,
        "tank/ROOT\n",
    );
    runner.respond(
        "zfs get -H -o value keylocation tank/ROOT",
        0,
        &format!("{keylocation}\n"),
    );
    runner.respond(
        "zfs get -H -o value keyformat tank/ROOT",
        0,
        &format!("{keyformat}\n"),
    );
    runner.respond(
        "zfs get -H -o value org.zfsbootmenu:keysource tank/ROOT/be",
        0,
        &format!("{keysource}\n"),
    );
    runner.respond("zfs list -o name -H tank/keys", 0, "tank/keys\n");
    runner.respond(
        "zfs get -H -o value encryptionroot tank/keys",
        0,
        "tank/keys\n",
    );
    runner.respond("zfs list -o name -H tank/missing", 1, "");
    runner.respond("zfs list -o name -H tank/plain", 0, "tank/plain\n");
    runner.respond("zfs get -H -o value encryptionroot tank/plain", 0, "-\n");
    fake
}

/// Where the key of `tank/ROOT/be` comes from on `fake`.
fn source(fake: &FakeSystem) -> Option<KeySource> {
    key_source(
        &fake.runner,
        &fake.files,
        &name("tank/ROOT/be"),
        BootPath::Menu,
    )
    .map(|(root, source)| {
        assert_eq!(root, name("tank/ROOT"));
        source
    })
}

#[test]
fn a_prompt_location_is_the_prompt() {
    assert_eq!(source(&machine("prompt", "-")), Some(KeySource::Prompt));
}

#[test]
fn a_key_file_in_the_image_comes_first() {
    let fake = machine("file:///etc/zfs/tank.key", "tank/keys");
    fake.files.file("/etc/zfs/tank.key", "key");

    assert_eq!(
        source(&fake),
        Some(KeySource::File("/etc/zfs/tank.key".into()))
    );
}

#[test]
fn a_missing_key_file_is_copied_from_an_encrypted_key_source_from_the_menu() {
    let fake = machine("file:///etc/zfs/tank.key", "tank/keys");

    assert_eq!(source(&fake), Some(KeySource::Keysource(name("tank/keys"))));
    let countdown = key_source(
        &fake.runner,
        &fake.files,
        &name("tank/ROOT/be"),
        BootPath::Countdown,
    );
    assert_eq!(countdown, Some((name("tank/ROOT"), KeySource::Prompt)));
    assert_eq!(BootPath::default(), BootPath::Countdown);
}

#[test]
fn a_key_source_that_is_missing_unencrypted_or_unset_is_ignored() {
    for keysource in ["tank/missing", "tank/plain", "-", ""] {
        let fake = machine("file:///etc/zfs/tank.key", keysource);
        assert_eq!(source(&fake), Some(KeySource::Prompt), "{keysource}");
    }
}

#[test]
fn without_a_file_only_a_passphrase_can_be_asked_for() {
    let remote = machine("https://keys.example/tank", "tank/keys");
    assert_eq!(source(&remote), Some(KeySource::Prompt));

    let raw = with_format("file:///etc/zfs/tank.key", "-", "raw");
    assert_eq!(source(&raw), Some(KeySource::Nothing));
}

#[test]
fn an_unencrypted_filesystem_has_no_key() {
    let fake = FakeSystem::new();
    fake.runner
        .respond("zpool list -H -o feature@encryption tank", 0, "disabled\n");

    assert_eq!(
        key_source(
            &fake.runner,
            &fake.files,
            &name("tank/ROOT/be"),
            BootPath::Menu
        ),
        None
    );
}

#[test]
fn key_files_and_sources_are_written_out() {
    assert_eq!(key_file("file:///etc/zfs/k"), Some("etc/zfs/k"));
    assert_eq!(key_file("file://k"), Some("k"));
    assert_eq!(key_file("file://"), None);
    assert_eq!(key_file("prompt"), None);
    let shown: Vec<String> = [
        KeySource::Prompt,
        KeySource::File("/k".into()),
        KeySource::Keysource(name("tank/keys")),
        KeySource::Nothing,
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    assert_eq!(
        shown,
        ["prompt", "file /k", "keysource tank/keys", "nothing"]
    );
}

#[test]
fn key_sources_say_what_they_are() {
    let shown: Vec<String> = [
        KeySource::Prompt,
        KeySource::File("/etc/zfs/key".to_owned()),
        KeySource::Keysource(name("tank/keys")),
        KeySource::Nothing,
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    assert_eq!(
        shown,
        [
            "prompt",
            "file /etc/zfs/key",
            "keysource tank/keys",
            "nothing"
        ]
    );
    let fake = machine("prompt", "tank/keys");
    assert_eq!(
        keysource_property(&fake.runner, &name("tank/ROOT/be")).as_deref(),
        Some("tank/keys")
    );
    let unset = machine("prompt", "-");
    assert_eq!(
        keysource_property(&unset.runner, &name("tank/ROOT/be")),
        None
    );
}
