//! The workspace's Pools tab as a tree (ADR 0007, decision 2; ADR 0016,
//! point 9): each pool, then its vdevs and their devices as `zpool status`
//! prints them, those of the data first, then those of each other class
//! under its heading (`logs`, `cache`, `spares`), which is no stop; then
//! the LUKS volumes that hold a device of it; then, under a header that is
//! no stop, the volumes of no pool listed. Each row knows its depth, which
//! the view indents. A device's name is the
//! last part of its path, or of the path it had ([`Device::path`]), drawn
//! printable, as its name comes from the pool's labels.
//!
//! The rows are the snapshot's, flattened depth first; a refresh selects
//! again the row that shows the same pool, device or volume, by its name
//! and a device's class too, as a spare in use shows twice, and the
//! device's pool when the device is gone.

use zbm_types::{PoolName, Printable};

use super::list::refilter;
use super::{Model, Screen, disk_name, pool_name, pools::Row};
use crate::menu::keymap::Map;
use crate::menu::world::{Device, DeviceClass, DeviceState, Pool, Volume};
use crate::text::text;

/// The classes of devices that show under a heading of their own, in the
/// order `zpool status` prints them.
const CLASSES: [DeviceClass; 5] = [
    DeviceClass::Dedup,
    DeviceClass::Special,
    DeviceClass::Log,
    DeviceClass::Cache,
    DeviceClass::Spare,
];

/// What a row of the tree shows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Kind {
    /// A pool.
    Pool,
    /// A group of devices, as `mirror-0` or `raidz1-0`.
    Vdev,
    /// A device nothing is below: a disk, a partition or a file.
    Device,
    /// A LUKS volume.
    Volume,
    /// The heading of the devices of a class other than the data's, as
    /// `logs`, which is no stop.
    Class,
    /// The header of the volumes of no pool listed, which is no stop.
    NoPool,
}

/// What a row stands for in the snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Item<'s> {
    /// A pool.
    Pool(&'s Pool),
    /// A vdev or a device of the pool.
    Device(&'s Pool, &'s Device),
    /// A volume.
    Volume(&'s Volume),
    /// A heading or a header: no stop.
    Header,
}

/// A row of the tree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Node<'s> {
    /// What it shows.
    pub(crate) kind: Kind,
    /// How deep it is: 0 for a pool and the header of the volumes of no
    /// pool, 1 for what is below a pool.
    pub(crate) depth: usize,
    /// Its name as the tree shows it, drawn printable.
    pub(crate) name: String,
    /// Its health, as `zpool` words it; none for a volume, a heading, a
    /// header, and a device `zpool` gives none.
    pub(crate) health: Option<&'static str>,
    /// What it stands for.
    pub(crate) item: Item<'s>,
}

impl Node<'_> {
    /// Whether the selection stops on it: on every row but the headings
    /// and the header.
    pub(crate) fn is_stop(&self) -> bool {
        self.item != Item::Header
    }

    /// What a refresh finds it again by.
    fn key(&self) -> Option<Row> {
        Some(match self.item {
            Item::Pool(pool) => Row::Pool(pool.name.clone(), pool.id),
            Item::Device(pool, device) => Row::Device(
                pool.name.clone(),
                pool.id,
                device.class,
                device.name.get().clone(),
            ),
            Item::Volume(volume) => Row::Volume(volume.name.get().clone()),
            Item::Header => return None,
        })
    }
}

/// Adds `device`, at `depth`, then the devices below it, to `nodes`.
fn add_device<'s>(nodes: &mut Vec<Node<'s>>, pool: &'s Pool, device: &'s Device, depth: usize) {
    let kind = if device.children.is_empty() {
        Kind::Device
    } else {
        Kind::Vdev
    };
    nodes.push(Node {
        kind,
        depth,
        name: disk_name(device.path()),
        health: device.state.map(DeviceState::as_str),
        item: Item::Device(pool, device),
    });
    for child in &device.children {
        add_device(nodes, pool, child, depth + 1);
    }
}

/// The heading of the devices of `class`, as `zpool status` prints it;
/// none for the data's, which have none.
pub(crate) fn class_heading(class: DeviceClass) -> Option<String> {
    Some(match class {
        DeviceClass::Data => return None,
        DeviceClass::Dedup => text!("pools-class-dedup"),
        DeviceClass::Special => text!("pools-class-special"),
        DeviceClass::Log => text!("pools-class-log"),
        DeviceClass::Cache => text!("pools-class-cache"),
        DeviceClass::Spare => text!("pools-class-spare"),
    })
}

/// Adds the devices of `pool`, the data's at depth 1, then those of each
/// other class at depth 2 below the heading of their class, to `nodes`.
fn add_devices<'s>(nodes: &mut Vec<Node<'s>>, pool: &'s Pool) {
    let of = |class| (pool.devices.iter()).filter(move |device| device.class == class);
    for device in of(DeviceClass::Data) {
        add_device(nodes, pool, device, 1);
    }
    for class in CLASSES {
        let mut devices = of(class).peekable();
        let Some(heading) = class_heading(class).filter(|_| devices.peek().is_some()) else {
            continue;
        };
        nodes.push(Node {
            kind: Kind::Class,
            depth: 1,
            name: heading,
            health: None,
            item: Item::Header,
        });
        for device in devices {
            add_device(nodes, pool, device, 2);
        }
    }
}

/// The row of `volume` at `depth`.
fn volume_node(volume: &Volume, depth: usize) -> Node<'_> {
    Node {
        kind: Kind::Volume,
        depth,
        name: volume.name.printable().to_string(),
        health: None,
        item: Item::Volume(volume),
    }
}

/// The tree of `pools` and `volumes`, depth first: each pool, named with
/// the identifier `id` gives it when another has its name, its devices,
/// the data's first and then those of each other class under its heading,
/// then the volumes of its name when it is imported, or the first of
/// its name none of which is; then the volumes of no pool listed, under
/// their header.
pub(crate) fn flatten<'s>(
    pools: &'s [Pool],
    volumes: &'s [Volume],
    id: impl Fn(&Pool) -> Option<u64>,
) -> Vec<Node<'s>> {
    // The pool a volume is listed under: the imported one of its name.
    let holder = |name: &PoolName| {
        let named = || pools.iter().filter(|pool| &pool.name == name);
        named()
            .find(|pool| pool.imported)
            .or_else(|| named().next())
    };
    let mut nodes = Vec::new();
    for pool in pools {
        nodes.push(Node {
            kind: Kind::Pool,
            depth: 0,
            name: pool_name(pool, id(pool)),
            health: Some(pool.health.as_str()),
            item: Item::Pool(pool),
        });
        add_devices(&mut nodes, pool);
        let held = volumes.iter().filter(|volume| {
            volume
                .pool
                .as_ref()
                .and_then(holder)
                .is_some_and(|held| std::ptr::eq(held, pool))
        });
        nodes.extend(held.map(|volume| volume_node(volume, 1)));
    }
    let mut free = volumes
        .iter()
        .filter(|volume| volume.pool.as_ref().and_then(holder).is_none())
        .peekable();
    if free.peek().is_some() {
        nodes.push(Node {
            kind: Kind::NoPool,
            depth: 0,
            name: text!("pools-no-pool"),
            health: None,
            item: Item::Header,
        });
        nodes.extend(free.map(|volume| volume_node(volume, 1)));
    }
    nodes
}

impl Model {
    /// The rows of the workspace's Pools tab.
    pub(crate) fn tree(&self) -> Vec<Node<'_>> {
        let snapshot = &self.snapshot;
        flatten(&snapshot.pools, &snapshot.volumes, |pool| {
            self.import_id(pool)
        })
    }

    /// The row of the tree the selection is on.
    pub(crate) fn selected_node(&self) -> Option<Node<'_>> {
        let at = self.pools.selected_item()?;
        self.tree().into_iter().nth(at)
    }

    /// The names of the selected row of the tree and of the rows above
    /// it: its pool, the heading of its class, its vdev and itself.
    pub(super) fn tree_path(&self) -> Vec<String> {
        if self.map != Map::Workspace || self.screen != Screen::Pools {
            return Vec::new();
        }
        let Some(at) = self.pools.selected_item() else {
            return Vec::new();
        };
        let nodes = self.tree();
        let Some(node) = nodes.get(at) else {
            return Vec::new();
        };
        let mut path = vec![node.name.clone()];
        let mut depth = node.depth;
        for above in nodes.iter().take(at).rev() {
            if depth == 0 {
                break;
            }
            if above.depth < depth {
                depth = above.depth;
                path.push(above.name.clone());
            }
        }
        path.reverse();
        path
    }

    /// Ranks the stops of the tree into the pools' list by the filter, as
    /// indexes of the tree's rows, and selects the row `row` names again:
    /// the pool of a device that is gone.
    pub(super) fn refilter_tree(&mut self, row: Option<Row>) {
        let (stops, labels): (Vec<usize>, Vec<String>) = self
            .tree()
            .into_iter()
            .enumerate()
            .filter(|(_, node)| node.is_stop())
            .map(|(index, node)| (index, node.name))
            .unzip();
        refilter(&mut self.pools, labels.iter().map(String::as_str));
        for ranked in &mut self.pools.matches {
            ranked.index = stops.get(ranked.index).copied().unwrap_or(ranked.index);
        }
        let Some(row) = row else {
            return;
        };
        let pool = match &row {
            Row::Device(name, id, ..) => Some(Row::Pool(name.clone(), *id)),
            _ => None,
        };
        let keys: Vec<Option<Row>> = self.tree().iter().map(Node::key).collect();
        let position = |wanted: &Row| {
            (self.pools.matches.iter())
                .position(|ranked| keys.get(ranked.index).and_then(Option::as_ref) == Some(wanted))
        };
        if let Some(found) = position(&row).or_else(|| pool.as_ref().and_then(position)) {
            self.pools.selected = found;
        }
    }

    /// What the selected row of the tree shows, as a refresh finds it again.
    pub(super) fn selected_tree_row(&self) -> Option<Row> {
        self.selected_node()?.key()
    }
}

/// The kind of the vdev `device` as its name says: `mirror` for
/// `mirror-0`, `raidz1` for `raidz1-0`, drawn printable.
pub(crate) fn vdev_kind(device: &Device) -> String {
    let name = device.name.get();
    let stem = name.split(['-', ':']).next().unwrap_or(name);
    Printable(stem).to_string()
}

#[cfg(test)]
mod tests;
