//! The pools view's rows as the menu reads them (ADR 0016, point 5): the
//! pools, each with its state and the host that used it last when it is
//! another system, then the volumes. A pool is its row: two pools that
//! could be imported may share a name, and are told apart by their
//! identifiers.

use zbm_types::{PoolName, Printable};

use super::Model;
use super::devices::Item;
use crate::menu::keymap::Map;
use crate::menu::world::{DeviceClass, Pool, PoolHost, Volume};
use crate::text::text;

/// The host that used a pool last, as the menu names it: `host otherhost
/// (deadbeef)`, the name drawn printable; whatever of it is known.
pub(crate) fn host_text(host: &PoolHost) -> String {
    let hostid = host.hostid.as_ref().map(ToString::to_string);
    let hostname = host
        .hostname
        .as_ref()
        .map(|name| name.printable().to_string());
    match (hostname, hostid) {
        (Some(hostname), Some(hostid)) => {
            text!("pool-host-named", hostname = hostname, hostid = hostid)
        }
        (Some(hostname), None) => text!("pool-host", hostid = hostname),
        (None, Some(hostid)) => text!("pool-host", hostid = hostid),
        (None, None) => text!("pool-host-unknown"),
    }
}

/// The last part of the path `path` of a device, drawn printable: `sdb2`
/// for `/dev/sdb2`.
pub(crate) fn disk_name(path: &str) -> String {
    let shown = Printable(path).to_string();
    shown.rsplit('/').next().unwrap_or(&shown).to_owned()
}

/// The name of `pool` as the menu shows it: with its identifier `id`
/// when another pool has its name.
pub(crate) fn pool_name(pool: &Pool, id: Option<u64>) -> String {
    match id {
        Some(id) => format!("{} ({})", pool.name, text!("pool-id", id = id.to_string())),
        None => pool.name.to_string(),
    }
}

/// Whether `pool` is imported, and how: not imported, read/write or
/// read-only.
pub(crate) fn pool_mode(pool: &Pool) -> String {
    match (pool.imported, pool.writable) {
        (false, _) => text!("pool-not-imported"),
        (true, true) => text!("pool-read-write"),
        (true, false) => text!("pool-read-only"),
    }
}

/// The row of `pool` as words: its name, with its identifier `id` when it
/// shares the name, its health or that it is not imported, its mode, and
/// the host that used it last when it is another.
fn pool_row(pool: &Pool, id: Option<u64>) -> String {
    let mut words = vec![pool_name(pool, id), pool.health.as_str().to_owned()];
    words.push(pool_mode(pool));
    words.extend(pool.host.as_ref().map(host_text));
    words.join(", ")
}

/// What a row of the pools view shows: a pool, by its name and
/// identifier, a vdev or a device of the workspace's tree, by its pool's
/// and its own name, or a volume, by its name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Row {
    /// A pool.
    Pool(PoolName, Option<u64>),
    /// A vdev or a device of the pool, of its class: a spare in use shows
    /// among the data's devices and among the spares.
    Device(PoolName, Option<u64>, DeviceClass, String),
    /// A volume.
    Volume(String),
}

impl Model {
    /// The rows of the pools view: each pool, with its state and host,
    /// then each volume.
    pub fn pool_rows(&self) -> Vec<String> {
        let pools = self.snapshot.pools.iter();
        let pools = pools.map(|pool| pool_row(pool, self.import_id(pool)));
        let volumes = self
            .snapshot
            .volumes
            .iter()
            .map(|volume| volume.name.get().clone());
        pools.chain(volumes).collect()
    }

    /// What each row of the pools view shows, as a refresh finds it again
    /// wherever the new rows put it.
    fn row_keys(&self) -> Vec<Row> {
        let pools = self.snapshot.pools.iter();
        let pools = pools.map(|pool| Row::Pool(pool.name.clone(), pool.id));
        let volumes = self.snapshot.volumes.iter();
        let volumes = volumes.map(|volume| Row::Volume(volume.name.get().clone()));
        pools.chain(volumes).collect()
    }

    /// What the selected row of the pools view shows.
    pub(super) fn selected_row(&self) -> Option<Row> {
        if self.map == Map::Workspace {
            return self.selected_tree_row();
        }
        let row = self.pools.selected_item()?;
        self.row_keys().get(row).cloned()
    }

    /// Matches the rows of the pools view against its filter again, and
    /// selects the row that shows `row` again, if one does; in the
    /// workspace, the rows of its tree.
    pub(super) fn refilter_pools(&mut self, row: Option<Row>) {
        if self.map == Map::Workspace {
            self.refilter_tree(row);
            return;
        }
        let pools = self.pool_rows();
        super::refilter(&mut self.pools, pools.iter().map(String::as_str));
        self.keep_row(row);
    }

    /// Selects the row of the pools view that shows `row` again, if one
    /// does.
    fn keep_row(&mut self, row: Option<Row>) {
        let Some(row) = row else {
            return;
        };
        let keys = self.row_keys();
        let found =
            (self.pools.matches.iter()).position(|ranked| keys.get(ranked.index) == Some(&row));
        if let Some(position) = found {
            self.pools.selected = position;
        }
    }

    /// The pool of the selected row of the pools view, if it is a pool; in
    /// the workspace's tree, the pool of its vdev or device too.
    pub fn selected_pool(&self) -> Option<&Pool> {
        if self.map == Map::Workspace {
            return match self.selected_node()?.item {
                Item::Pool(pool) | Item::Device(pool, _) => Some(pool),
                Item::Volume(_) | Item::Header => None,
            };
        }
        let row = self.pools.selected_item()?;
        self.snapshot.pools.get(row)
    }

    /// The volume of the selected row of the pools view.
    pub fn selected_volume(&self) -> Option<&Volume> {
        if self.map == Map::Workspace {
            return match self.selected_node()?.item {
                Item::Volume(volume) => Some(volume),
                _ => None,
            };
        }
        let row = self
            .pools
            .selected_item()?
            .checked_sub(self.snapshot.pools.len())?;
        self.snapshot.volumes.get(row)
    }

    /// The identifier an import of `pool` names it by: its own, when
    /// another pool of the snapshot has its name; `None`, its name does.
    pub fn import_id(&self, pool: &Pool) -> Option<u64> {
        let shared = self
            .snapshot
            .pools
            .iter()
            .filter(|other| other.name == pool.name)
            .count()
            > 1;
        pool.id.filter(|_| shared)
    }

    /// Whether `pool` is not imported, and another pool of its name is:
    /// the menu shows it, but does not offer to import it.
    pub fn is_shadowed(&self, pool: &Pool) -> bool {
        !pool.imported
            && self
                .snapshot
                .pools
                .iter()
                .any(|other| other.imported && other.name == pool.name && other.id != pool.id)
    }
}
