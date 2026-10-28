use pretty_assertions::assert_eq;
use zbm_platform::exec::{Input, Stream};
use zbm_testkit::FakeRunner;

use super::*;

fn dataset(name: &str) -> DatasetName {
    name.parse().unwrap()
}

fn pool(name: &str) -> PoolName {
    name.parse().unwrap()
}

#[test]
fn dataset_commands_are_those_of_the_legacy_runtime() {
    let runner = FakeRunner::new();
    runner.respond(
        "zfs get -H -o value org.zfsbootmenu:commandline pool/be",
        0,
        "quiet\n",
    );
    runner.respond("zfs get -H -o value mountpoint pool/be@snap", 1, "");
    runner.respond("zfs list -o name -H pool/missing", 1, "");
    let zfs = ZfsText::new(&runner);
    let be = dataset("pool/be");
    assert_eq!(
        zfs.value(&be, COMMANDLINE.name, Stderr::Console).as_deref(),
        Some("quiet")
    );
    let snapshot: FilesystemName = "pool/be@snap".parse().unwrap();
    assert_eq!(zfs.value(&snapshot, "mountpoint", Stderr::Hidden), None);
    assert!(zfs.exists(&be));
    assert!(!zfs.exists(&dataset("pool/missing")));
    assert!(zfs.is_filesystem(&be));
    assert!(zfs.set(&be, COMMANDLINE.name, "a b").unwrap());
    assert!(zfs.inherit(&be, COMMANDLINE.name).unwrap());
    assert_eq!(zfs.commandline("pool/be").as_deref(), Some("quiet"));
    assert_eq!(zfs.inherited_source("pool/be").as_deref(), Some(""));
    assert_eq!(
        runner.lines(),
        [
            "zfs get -H -o value org.zfsbootmenu:commandline pool/be",
            "zfs get -H -o value mountpoint pool/be@snap",
            "zfs list -o name -H pool/be",
            "zfs list -o name -H pool/missing",
            "zfs list -H -o name -t filesystem pool/be",
            "zfs set org.zfsbootmenu:commandline=a b pool/be",
            "zfs inherit org.zfsbootmenu:commandline pool/be",
            "zfs get -H -o value org.zfsbootmenu:commandline pool/be",
            "zfs get -H -o source -s inherited org.zfsbootmenu:commandline pool/be",
        ]
    );
    let calls = runner.calls();
    assert_eq!(calls[1].get_stderr(), Stream::Null);
}

#[test]
fn keys_are_loaded_on_the_console() {
    let runner = FakeRunner::new();
    runner.respond("zfs load-key -L prompt tank/ROOT", 1, "");
    let zfs = ZfsText::new(&runner);
    let root = dataset("tank/ROOT");

    assert!(!zfs.load_key(&root, "prompt").unwrap());
    assert!(zfs.load_key(&root, "file:///etc/zfs/k").unwrap());
    let calls = runner.calls();
    assert_eq!(
        calls[1].display(),
        "zfs load-key -L file:///etc/zfs/k tank/ROOT"
    );
    assert!(matches!(calls[0].get_stdin(), Input::Inherit));
    assert_eq!(calls[0].get_stdout(), Stream::Inherit);
}

#[test]
fn pool_commands_are_those_of_the_legacy_runtime() {
    let runner = FakeRunner::new();
    let tank = pool("tank");
    runner.respond("zpool get -H -o value readonly tank", 0, "off\n");
    runner.respond("zpool list -H -o name", 0, "rpool\ntank\n\n");
    runner.respond("zpool list -H -o name,health", 0, "tank\tONLINE\n");
    runner.respond("zpool list -H -o bootfs tank", 0, "tank/ROOT/be\n");
    runner.respond("zpool list -H -o bootfs --", 0, "-\n");
    runner.respond("zpool get all -H -o name,property", 0, "tank\tsize\n");
    runner.respond("zpool export tank", 1, "");
    runner.respond("zpool import", 0, "pool: tank\n");
    runner.respond("zpool status -P", 0, "pool: tank\n");
    let zpool = ZfsText::new(&runner);

    assert_eq!(
        zpool
            .pool_value(&tank, "readonly", Stderr::Hidden)
            .as_deref(),
        Some("off")
    );
    assert_eq!(
        zpool.imported(Stderr::Hidden),
        [pool("rpool"), tank.clone()]
    );
    assert_eq!(
        zpool.pools_list(&["name", "health"], Stderr::Console),
        "tank\tONLINE\n"
    );
    assert_eq!(zpool.pool_list(None, &["bootfs"], Stderr::Console), "-\n");
    assert_eq!(
        zpool.pool_list(Some(&tank), &["bootfs"], Stderr::Console),
        "tank/ROOT/be\n"
    );
    assert_eq!(zpool.pool_properties(Stderr::Console), "tank\tsize\n");
    assert_eq!(zpool.export(&tank), 1);
    assert_eq!(zpool.importable(), "pool: tank\n");
    assert_eq!(zpool.probe_import(&tank), "");
    assert_eq!(zpool.probe_import_id("1234"), "");
    assert_eq!(zpool.status(), "pool: tank\n");
    assert_eq!(
        runner.lines()[8..],
        [
            "zpool import -o readonly=on -N tank",
            "zpool import -N 1234",
            "zpool status -P",
        ]
    );
}

#[test]
fn an_import_reports_its_status_and_output() {
    let runner = FakeRunner::new();
    let tank = pool("tank");
    runner.respond("zpool import -N -o readonly=on tank", 1, "no such pool\n");
    let zpool = ZfsText::new(&runner);
    assert_eq!(
        zpool.import(Import::pool(&tank)),
        (1, "no such pool\n".to_owned())
    );
    assert_eq!(zpool.import(Import::all()), (0, String::new()));
    assert!(runner.calls()[0].merges_stderr());

    let missing = FakeRunner::new();
    missing.on(|_| Some(Err(io::Error::other("no zpool"))));
    let zpool = ZfsText::new(&missing);
    assert_eq!(zpool.import(Import::all()), (126, "no zpool".to_owned()));
    assert_eq!(zpool.export(&tank), 126);
    assert_eq!(zpool.status(), "");
}
