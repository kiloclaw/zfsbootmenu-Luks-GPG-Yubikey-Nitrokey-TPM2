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
            assert_eq!(change.to_string(), ran);
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
        format!("{:?}", dry.snapshot_space(&root)),
        format!("{:?}", cli.snapshot_space(&root)),
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
