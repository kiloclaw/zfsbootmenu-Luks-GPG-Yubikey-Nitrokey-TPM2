use zbm_testkit::fakes::FakeSystem;

use super::*;
use crate::testing::name;

/// A machine whose boot environment `tank/ROOT/be` is encrypted below
/// `tank/ROOT` with a passphrase, with the key location and key source given.
fn machine(keylocation: &str, keysource: &str) -> FakeSystem {
    let keyformat = "passphrase";
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

#[test]
fn a_key_source_is_an_encrypted_filesystem_that_exists() {
    let fake = machine("file:///etc/zfs/tank.key", "tank/keys");
    assert_eq!(
        keysource(&fake.runner, &name("tank/ROOT/be")),
        Some(name("tank/keys"))
    );
    assert_eq!(BootPath::default(), BootPath::Countdown);
}

#[test]
fn a_key_source_that_is_missing_unencrypted_or_unset_is_ignored() {
    for keysource_name in ["tank/missing", "tank/plain", "-", ""] {
        let fake = machine("file:///etc/zfs/tank.key", keysource_name);
        assert_eq!(
            keysource(&fake.runner, &name("tank/ROOT/be")),
            None,
            "{keysource_name}"
        );
    }
    let unreadable = FakeSystem::new();
    unreadable.runner.respond(
        "zfs get -H -o value org.zfsbootmenu:keysource tank/ROOT/be",
        1,
        "",
    );
    assert_eq!(keysource(&unreadable.runner, &name("tank/ROOT/be")), None);
}
