use pretty_assertions::assert_eq;
use zbm_types::Untrusted;

use super::*;
use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, FakeMachine};
use crate::menu::theme::Theme;
use crate::menu::world::{Action, DeviceClass, DeviceState, Event, Health};

/// The menu on the demo machine with `options`, in the workspace, on its
/// Pools tab.
fn workspace(options: &DemoOptions) -> Model {
    let machine = FakeMachine::new(options);
    let mut model = Model::new(machine.snapshot().clone(), Theme::new(Capabilities::FULL));
    model.map = Map::Workspace;
    model.screen = Screen::Pools;
    model.refresh();
    model
}

/// Each row of the tree of `model`: its depth, kind, name and health.
fn rows(model: &Model) -> Vec<(usize, Kind, String, Option<&'static str>)> {
    model
        .tree()
        .into_iter()
        .map(|node| (node.depth, node.kind, node.name, node.health))
        .collect()
}

/// The names of the selected row and those above it.
fn path(model: &Model) -> Vec<String> {
    model.path()
}

#[test]
fn the_tree_lists_each_pool_then_its_vdevs_devices_and_volumes_depth_first() {
    let mut model = workspace(&DemoOptions::default());
    let row = |depth, kind, name: &str, health| (depth, kind, name.to_owned(), health);
    let online = Some("ONLINE");

    assert_eq!(
        rows(&model),
        [
            row(0, Kind::Pool, "zroot", online),
            row(1, Kind::Vdev, "mirror-0", online),
            row(2, Kind::Device, "nvme0n1p2", online),
            row(2, Kind::Device, "nvme1n1p2", online),
            row(1, Kind::Class, "cache", None),
            row(2, Kind::Device, "nvme2n1", online),
            row(1, Kind::Volume, "luks-zroot-a", None),
            row(1, Kind::Volume, "luks-zroot-b", None),
            row(0, Kind::Pool, "tank", Some("DEGRADED")),
            row(1, Kind::Vdev, "raidz1-0", Some("DEGRADED")),
            row(2, Kind::Device, "sda2", online),
            row(2, Kind::Device, "sdb2", Some("UNAVAIL")),
            row(2, Kind::Device, "sdc2", Some("DEGRADED")),
            row(1, Kind::Volume, "luks-tank", None),
        ]
    );
    // Every row but the heading is a stop, in the order of the tree.
    let stops: Vec<usize> = model.pools.matches.iter().map(|r| r.index).collect();
    let wanted: Vec<usize> = (0..14).filter(|&index| index != 4).collect();
    assert_eq!(stops, wanted);
    assert_eq!(path(&model), ["zroot"]);
    model.pools.selected = 4;
    assert_eq!(path(&model), ["zroot", "cache", "nvme2n1"]);
}

#[test]
fn volumes_of_no_pool_listed_show_under_a_header_that_is_no_stop() {
    let mut model = workspace(&DemoOptions::default());
    let mut snapshot = model.snapshot.clone();
    snapshot.volumes[0].pool = None;
    snapshot.volumes[2].pool = Some("gone".parse().unwrap());
    model.replace_snapshot(snapshot);

    let tree = rows(&model);
    let tail: Vec<(usize, Kind, &str)> = tree[tree.len() - 3..]
        .iter()
        .map(|(depth, kind, name, _)| (*depth, *kind, name.as_str()))
        .collect();
    assert_eq!(
        tail,
        [
            (0, Kind::NoPool, "No pool"),
            (1, Kind::Volume, "luks-zroot-a"),
            (1, Kind::Volume, "luks-tank"),
        ]
    );
    // The header, as the heading of the cache, is a row the selection
    // skips.
    let header = tree.len() - 3;
    assert_eq!(model.pools.matches.len(), tree.len() - 2);
    assert!(model.pools.matches.iter().all(|r| r.index != header));
    model.pools.selected = model.pools.matches.len() - 1;
    assert_eq!(path(&model), ["No pool", "luks-tank"]);
}

#[test]
fn a_volume_shows_under_the_imported_pool_of_its_name() {
    let options = DemoOptions {
        exported: true,
        ..DemoOptions::default()
    };
    let mut machine = FakeMachine::new(&options);
    let mut model = workspace(&options);
    for event in machine.apply(Action::ScanPools) {
        if let Event::PoolsScanned(pools) = event {
            model.replace_pools(pools);
        }
    }

    let names: Vec<String> = rows(&model).into_iter().map(|row| row.2).collect();
    let tank = names.iter().position(|name| name == "tank (id 2)");
    let volume = names.iter().position(|name| name == "luks-tank");
    let other = names.iter().position(|name| name == "tank (id 5)");
    assert!(tank < volume && volume < other, "{names:?}");
}

#[test]
fn a_rescan_keeps_the_device_selected_by_name_and_its_pool_once_it_is_gone() {
    let mut model = workspace(&DemoOptions::default());
    let select = |model: &mut Model, name: &str| {
        let nodes = model.tree();
        let at = nodes.iter().position(|node| node.name == name).unwrap();
        model.pools.selected = model
            .pools
            .matches
            .iter()
            .position(|r| r.index == at)
            .unwrap();
    };
    select(&mut model, "sdb2");
    assert_eq!(path(&model), ["tank", "raidz1-0", "sdb2"]);

    // The scan lists another pool first: the device stays selected.
    let mut pools = model.snapshot.pools.clone();
    let mut first = pools[0].clone();
    first.name = "aaa".parse().unwrap();
    first.id = Some(7);
    pools.insert(0, first);
    model.replace_pools(pools.clone());
    assert_eq!(path(&model), ["tank", "raidz1-0", "sdb2"]);

    // The device is gone: its pool is selected.
    let tank = &mut pools[2];
    tank.devices[0]
        .children
        .retain(|device| device.name.get() != "/dev/sdb2");
    model.replace_pools(pools.clone());
    assert_eq!(path(&model), ["tank"]);

    // A volume, by its name.
    select(&mut model, "luks-tank");
    pools.remove(0);
    model.replace_pools(pools);
    assert_eq!(path(&model), ["tank", "luks-tank"]);
    assert_eq!(
        model
            .selected_volume()
            .map(|volume| volume.name.get().as_str()),
        Some("luks-tank")
    );
    assert_eq!(model.selected_pool(), None);
}

#[test]
fn a_filter_ranks_the_stops_by_name_and_the_path_follows_the_tree() {
    let mut model = workspace(&DemoOptions::default());
    model.pools.filter = "sdc".to_owned();
    model.refresh();

    assert_eq!(model.pools.matches.len(), 1);
    assert_eq!(path(&model), ["tank", "raidz1-0", "sdc2"]);
    assert_eq!(
        model.selected_pool().map(|pool| pool.name.as_str()),
        Some("tank")
    );
}

#[test]
fn the_classic_menu_keeps_its_rows_of_pools_then_volumes() {
    let mut model = workspace(&DemoOptions::default());
    model.map = Map::Classic;
    model.refresh();

    assert_eq!(model.pools.matches.len(), 5);
    assert_eq!(model.path(), Vec::<String>::new());
    model.pools.selected = 3;
    assert_eq!(
        model
            .selected_volume()
            .map(|volume| volume.name.get().as_str()),
        Some("luks-zroot-b")
    );
}

#[test]
fn a_vdev_is_named_by_its_kind_and_a_lost_device_by_the_path_it_had() {
    let device = |name: &str, note: Option<&str>| Device {
        name: Untrusted::new(name.to_owned()),
        class: DeviceClass::Data,
        state: Some(DeviceState::Unavailable),
        errors: None,
        note: note.map(|note| Untrusted::new(note.to_owned())),
        children: Vec::new(),
    };

    assert_eq!(vdev_kind(&device("mirror-0", None)), "mirror");
    assert_eq!(vdev_kind(&device("raidz2-1", None)), "raidz2");
    assert_eq!(vdev_kind(&device("draid1:4d:6c:0s-0", None)), "draid1");
    assert_eq!(vdev_kind(&device("\u{1b}[2J-0", None)), "\\x1b[2J");
    let mut pool = Pool::imported("tank".parse().unwrap(), Health::Degraded);
    pool.devices = vec![device("1234", Some("was /dev/sdb1"))];
    let names: Vec<String> = flatten(std::slice::from_ref(&pool), &[], |_| None)
        .into_iter()
        .map(|node| node.name)
        .collect();
    assert_eq!(names, ["tank", "sdb1"]);
}

/// A device of the class `class` named `name`, with `children`.
fn of_class(class: DeviceClass, name: &str, children: Vec<Device>) -> Device {
    Device {
        name: Untrusted::new(name.to_owned()),
        class,
        state: Some(DeviceState::Online),
        errors: None,
        note: None,
        children,
    }
}

/// `tank` as `zpool status` prints a pool with a log mirror, a cache, a
/// special vdev and a spare in use, which shows twice: below the spare
/// vdev that stands in for a disk, and among the spares, with no state.
fn classes() -> Pool {
    let disk = |class, name: &str| of_class(class, name, Vec::new());
    let spare = of_class(
        DeviceClass::Data,
        "spare-0",
        vec![
            disk(DeviceClass::Data, "/dev/sdb"),
            disk(DeviceClass::Data, "/dev/sdd"),
        ],
    );
    let data = of_class(
        DeviceClass::Data,
        "mirror-0",
        vec![disk(DeviceClass::Data, "/dev/sda"), spare],
    );
    let log = of_class(
        DeviceClass::Log,
        "mirror-1",
        vec![
            disk(DeviceClass::Log, "/dev/nvme0n1"),
            disk(DeviceClass::Log, "/dev/nvme1n1"),
        ],
    );
    let in_use = Device {
        state: None,
        ..disk(DeviceClass::Spare, "/dev/sdd")
    };
    let mut pool = Pool::imported("tank".parse().unwrap(), Health::Online);
    // In the order of the classes, which the tree keeps whatever the order
    // of the snapshot.
    pool.devices = vec![
        disk(DeviceClass::Cache, "/dev/nvme2n1"),
        in_use,
        log,
        data,
        disk(DeviceClass::Special, "/dev/nvme3n1"),
    ];
    pool
}

#[test]
fn the_devices_of_each_class_show_under_its_heading_as_zpool_prints_them() {
    let pool = classes();

    let rows: Vec<(usize, Kind, String)> = flatten(std::slice::from_ref(&pool), &[], |_| None)
        .into_iter()
        .map(|node| (node.depth, node.kind, node.name))
        .collect();
    let row = |depth, kind, name: &str| (depth, kind, name.to_owned());
    assert_eq!(
        rows,
        [
            row(0, Kind::Pool, "tank"),
            row(1, Kind::Vdev, "mirror-0"),
            row(2, Kind::Device, "sda"),
            row(2, Kind::Vdev, "spare-0"),
            row(3, Kind::Device, "sdb"),
            row(3, Kind::Device, "sdd"),
            row(1, Kind::Class, "special"),
            row(2, Kind::Device, "nvme3n1"),
            row(1, Kind::Class, "logs"),
            row(2, Kind::Vdev, "mirror-1"),
            row(3, Kind::Device, "nvme0n1"),
            row(3, Kind::Device, "nvme1n1"),
            row(1, Kind::Class, "cache"),
            row(2, Kind::Device, "nvme2n1"),
            row(1, Kind::Class, "spares"),
            row(2, Kind::Device, "sdd"),
        ]
    );
}

#[test]
fn a_spare_in_use_stays_selected_where_it_was_across_a_rescan() {
    let mut model = workspace(&DemoOptions::default());
    model.snapshot.volumes.clear();
    model.replace_pools(vec![classes()]);
    // The last stop: the spare among the spares.
    model.pools.selected = model.pools.matches.len() - 1;
    assert_eq!(path(&model), ["tank", "spares", "sdd"]);

    model.replace_pools(vec![classes()]);
    assert_eq!(path(&model), ["tank", "spares", "sdd"]);
    // And the copy below the spare vdev, too.
    model.pools.selected = 5;
    assert_eq!(path(&model), ["tank", "mirror-0", "spare-0", "sdd"]);
    model.replace_pools(vec![classes()]);
    assert_eq!(path(&model), ["tank", "mirror-0", "spare-0", "sdd"]);
}
