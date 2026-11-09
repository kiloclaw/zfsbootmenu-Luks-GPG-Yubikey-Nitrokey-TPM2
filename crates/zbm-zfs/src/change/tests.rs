use std::sync::Mutex;

use zbm_testkit::FakeRunner;

use super::*;
use crate::ZfsCli;

fn snapshot() -> SnapshotName {
    "tank/ROOT/be@new".parse().unwrap()
}

fn dataset(name: &str) -> DatasetName {
    name.parse().unwrap()
}

/// Every change, with properties where it takes them.
fn changes() -> Vec<ZfsChange> {
    let properties = vec![("canmount".to_owned(), "noauto".to_owned())];
    vec![
        ZfsChange::Export("tank".parse().unwrap()),
        ZfsChange::Set {
            dataset: dataset("tank/ROOT/be"),
            property: "org.zfsbootmenu:active".into(),
            value: "on".into(),
        },
        ZfsChange::Set {
            dataset: dataset("tank/ROOT/be"),
            property: "org.zfsbootmenu:commandline".into(),
            value: "quiet loglevel=4".into(),
        },
        ZfsChange::Inherit {
            dataset: dataset("tank/ROOT/be"),
            property: "org.zfsbootmenu:kernel".into(),
        },
        ZfsChange::SetPool {
            pool: "tank".parse().unwrap(),
            property: "bootfs".into(),
            value: "tank/ROOT/be".into(),
        },
        ZfsChange::Snapshot(snapshot()),
        ZfsChange::Rollback(snapshot()),
        ZfsChange::Destroy(vec![snapshot(), "tank/ROOT/be@old".parse().unwrap()]),
        ZfsChange::Clone {
            snapshot: snapshot(),
            target: dataset("tank/ROOT/copy"),
            properties: properties.clone(),
        },
        ZfsChange::Promote(dataset("tank/ROOT/copy")),
        ZfsChange::InheritKey(dataset("tank/ROOT/copy")),
        ZfsChange::SendReceive {
            snapshot: snapshot(),
            target: dataset("tank/ROOT/copy"),
            properties,
        },
    ]
}

/// Makes `change` through `zfs`.
fn make(zfs: &dyn Zfs, change: &ZfsChange) {
    let made = match change {
        ZfsChange::Export(pool) => zfs.export(pool),
        ZfsChange::Set {
            dataset,
            property,
            value,
        } => zfs.set(dataset, property, value),
        ZfsChange::Inherit { dataset, property } => zfs.inherit(dataset, property),
        ZfsChange::SetPool {
            pool,
            property,
            value,
        } => zfs.set_pool_property(pool, property, value),
        ZfsChange::Snapshot(snapshot) => zfs.create_snapshot(snapshot),
        ZfsChange::Rollback(snapshot) => zfs.rollback(snapshot),
        ZfsChange::Destroy(snapshots) => zfs.destroy_snapshots(snapshots),
        ZfsChange::Clone {
            snapshot,
            target,
            properties,
        } => zfs.clone_snapshot(snapshot, target, properties),
        ZfsChange::Promote(dataset) => zfs.promote(dataset),
        ZfsChange::SendReceive {
            snapshot,
            target,
            properties,
        } => zfs.send_receive(snapshot, target, properties),
        ZfsChange::InheritKey(dataset) => zfs.inherit_key(dataset),
    };
    made.unwrap();
}

#[test]
fn a_change_shows_the_command_the_cli_runs() {
    let runner = FakeRunner::new();
    let cli = ZfsCli::new(&runner);
    for change in changes() {
        make(&cli, &change);
        let ran = runner.lines().pop().unwrap();
        if let ZfsChange::SendReceive { .. } = change {
            // A pipeline in a shell: its receive's arguments are the same.
            assert!(ran.ends_with("tank/ROOT/be@new -u -o canmount=noauto tank/ROOT/copy"));
            assert_eq!(
                change.to_string(),
                "zfs send -p -w tank/ROOT/be@new | zfs recv -u -o canmount=noauto tank/ROOT/copy"
            );
        } else {
            // A quoted argument is the runner's once its quotes go: the
            // runner passes each argument as it is, with no shell.
            assert_eq!(change.to_string().replace('\'', ""), ran);
        }
    }
    assert_eq!(
        ZfsChange::Snapshot(snapshot()).to_string(),
        "zfs snapshot tank/ROOT/be@new"
    );
}

#[test]
fn a_dry_run_reads_the_machine_and_plans_its_changes() {
    let runner = FakeRunner::new();
    runner.respond("zfs get -H -o value -p available tank/ROOT", 0, "1024\n");
    let cli = ZfsCli::new(&runner);
    let planned = Mutex::new(Vec::new());
    let plan = |change: ZfsChange| planned.lock().unwrap().push(change);
    let dry = DryRun::new(&cli, &plan);

    for change in changes() {
        make(&dry, &change);
    }
    let _ = dry.property(&dataset("tank/ROOT"), "available");

    assert_eq!(*planned.lock().unwrap(), changes());
    // Only what was read ran, with the version it was read in.
    let read = |line: &String| line.starts_with("zfs get") || line.starts_with("zfs version");
    assert!(runner.lines().iter().all(read), "{:?}", runner.lines());
}

#[test]
fn a_dry_run_reads_what_the_real_port_reads() {
    let runner = FakeRunner::new();
    let cli = ZfsCli::new(&runner);
    let plan = |_: ZfsChange| panic!("nothing changes");
    let dry = DryRun::new(&cli, &plan);
    let pool: PoolName = "tank".parse().unwrap();
    let root = dataset("tank/ROOT");
    let same = |dry: String, real: String| assert_eq!(dry, real);

    same(
        format!("{:?}", dry.version()),
        format!("{:?}", cli.version()),
    );
    same(format!("{:?}", dry.pools()), format!("{:?}", cli.pools()));
    same(
        format!("{:?}", dry.pool_names()),
        format!("{:?}", cli.pool_names()),
    );
    same(
        format!("{:?}", dry.devices(&pool)),
        format!("{:?}", cli.devices(&pool)),
    );
    same(format!("{:?}", dry.status()), format!("{:?}", cli.status()));
    same(
        format!("{:?}", dry.importable()),
        format!("{:?}", cli.importable()),
    );
    same(
        format!("{:?}", dry.pool_host(7)),
        format!("{:?}", cli.pool_host(7)),
    );
    same(
        format!("{:?}", dry.pool_property(&pool, "bootfs")),
        format!("{:?}", cli.pool_property(&pool, "bootfs")),
    );
    same(
        format!("{:?}", dry.filesystems(Some(&root), &["name"])),
        format!("{:?}", cli.filesystems(Some(&root), &["name"])),
    );
    same(
        format!("{:?}", dry.snapshots(&root)),
        format!("{:?}", cli.snapshots(&root)),
    );
    same(
        format!("{:?}", dry.own_snapshots(&root)),
        format!("{:?}", cli.own_snapshots(&root)),
    );
    same(
        format!("{:?}", dry.snapshot_details(&root)),
        format!("{:?}", cli.snapshot_details(&root)),
    );
    same(
        format!("{:?}", dry.holds(&[snapshot()])),
        format!("{:?}", cli.holds(&[snapshot()])),
    );
    let set = SnapshotSet::Each(vec![snapshot()]);
    same(
        format!("{:?}", dry.destroy_space(&set)),
        format!("{:?}", cli.destroy_space(&set)),
    );
    same(
        format!("{:?}", dry.checkpoint(&pool)),
        format!("{:?}", cli.checkpoint(&pool)),
    );
    same(
        format!("{:?}", dry.snapshot_property(&snapshot(), "creation")),
        format!("{:?}", cli.snapshot_property(&snapshot(), "creation")),
    );
    same(
        format!("{:?}", dry.own_properties(&root)),
        format!("{:?}", cli.own_properties(&root)),
    );
    same(
        format!("{:?}", dry.diff(&snapshot(), None)),
        format!("{:?}", cli.diff(&snapshot(), None)),
    );
}

#[test]
fn a_dry_run_counts_the_space_a_destroy_frees_and_plans_the_destroy() {
    let runner = FakeRunner::new();
    let count = "zfs destroy -n -v -p tank/ROOT/be@new";
    runner.respond(count, 0, "destroy\ttank/ROOT/be@new\nreclaim\t4096\n");
    let cli = ZfsCli::new(&runner);
    let planned = Mutex::new(Vec::new());
    let plan = |change: ZfsChange| planned.lock().unwrap().push(change);
    let dry = DryRun::new(&cli, &plan);

    let space = dry.destroy_space(&SnapshotSet::Each(vec![snapshot()]));
    dry.destroy_snapshots(&[snapshot()]).unwrap();

    assert_eq!(space.unwrap(), 4096);
    assert_eq!(runner.lines(), [count]);
    assert_eq!(
        *planned.lock().unwrap(),
        [ZfsChange::Destroy(vec![snapshot()])]
    );
}

#[test]
fn a_destroy_names_the_snapshots_of_one_dataset_in_one_argument() {
    let name = |text: &str| -> SnapshotName { text.parse().unwrap() };
    let (a, b, c) = (
        name("tank/ROOT/be@a"),
        name("tank/ROOT/be@b"),
        name("tank/ROOT/be@c"),
    );
    let other = name("tank/home@a");

    let destroy = |snapshots: &[SnapshotName]| ZfsChange::Destroy(snapshots.to_vec()).command();

    assert_eq!(
        destroy(&[a.clone(), b.clone(), c.clone()]),
        (
            "zfs",
            vec!["destroy".to_owned(), "tank/ROOT/be@a,b,c".to_owned()]
        )
    );
    // Several datasets or none are more or fewer arguments, which zfs
    // refuses: nothing is destroyed.
    assert_eq!(
        destroy(&[a.clone(), other.clone(), b.clone()]).1,
        ["destroy", "tank/ROOT/be@a,b", "tank/home@a"]
    );
    assert_eq!(destroy(&[]).1, ["destroy"]);
    let range = SnapshotSet::Range {
        first: a.clone(),
        last: c,
    };
    assert_eq!(range.targets(), ["tank/ROOT/be@a%c"]);
    let across = SnapshotSet::Range {
        first: a,
        last: other,
    };
    assert_eq!(across.targets(), ["tank/ROOT/be@a", "tank/home@a"]);
    assert_eq!(
        ZfsChange::Destroy(vec![b]).to_string(),
        "zfs destroy tank/ROOT/be@b"
    );
}

#[test]
fn a_value_with_spaces_is_quoted_as_a_shell_would_take_it() {
    let set = |value: &str| ZfsChange::Set {
        dataset: dataset("tank/ROOT/be"),
        property: "org.zfsbootmenu:commandline".into(),
        value: value.into(),
    };

    assert_eq!(
        set("quiet loglevel=4").to_string(),
        "zfs set org.zfsbootmenu:commandline='quiet loglevel=4' tank/ROOT/be"
    );
    assert_eq!(
        set("it's $HOME; rm -rf *").to_string(),
        r"zfs set org.zfsbootmenu:commandline='it'\''s $HOME; rm -rf *' tank/ROOT/be"
    );
    // The command run is unchanged: one argument, unquoted.
    assert_eq!(
        set("quiet loglevel=4").command().1,
        [
            "set",
            "org.zfsbootmenu:commandline=quiet loglevel=4",
            "tank/ROOT/be"
        ]
    );
    let pool = ZfsChange::SetPool {
        pool: "tank".parse().unwrap(),
        property: "comment".into(),
        value: "a b".into(),
    };
    assert_eq!(pool.to_string(), "zpool set comment='a b' tank");
    let clone = ZfsChange::Clone {
        snapshot: snapshot(),
        target: dataset("tank/ROOT/copy"),
        properties: vec![(
            "org.zfsbootmenu:commandline".to_owned(),
            "ro quiet".to_owned(),
        )],
    };
    assert_eq!(
        clone.to_string(),
        "zfs clone -o org.zfsbootmenu:commandline='ro quiet' tank/ROOT/be@new tank/ROOT/copy"
    );
}

#[test]
fn only_an_argument_a_shell_would_read_otherwise_is_quoted() {
    // A pool named with a space is quoted; the other changes are not.
    assert_eq!(
        ZfsChange::Export("my pool".parse().unwrap()).to_string(),
        "zpool export 'my pool'"
    );
    let lines: Vec<String> = changes()
        .iter()
        .filter(|change| !matches!(change, ZfsChange::Set { value, .. } if value.contains(' ')))
        .map(ToString::to_string)
        .collect();
    assert!(lines.iter().all(|line| !line.contains('\'')), "{lines:?}");
}
