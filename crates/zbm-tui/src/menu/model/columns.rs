//! The columns of the workspace's Boot tab (ADR 0007, decision 2): the
//! boot environments grouped by pool, the kernels of one of them, and the
//! path of what they select, which shows above them, as above the
//! Snapshots tab's.

use zbm_types::PoolName;

use super::list::keep;
use super::{Model, Screen};
use crate::menu::keymap::Map;
use crate::menu::world::{Environment, Kernel};

impl Model {
    /// The indexes of the environments in the order of the main view:
    /// defaults first, then by name. In the workspace they are grouped by
    /// pool, each pool where its first environment is in that order, and
    /// within a pool the order stays: the default first.
    pub(super) fn environment_order(&self) -> Vec<usize> {
        let mut order: Vec<(usize, &Environment)> =
            self.snapshot.environments.iter().enumerate().collect();
        order.sort_by(|(_, a), (_, b)| b.is_default.cmp(&a.is_default).then(a.name.cmp(&b.name)));
        if self.map == Map::Workspace {
            let mut pools: Vec<PoolName> = Vec::new();
            for (_, environment) in &order {
                let pool = environment.name.pool();
                if !pools.contains(&pool) {
                    pools.push(pool);
                }
            }
            // A stable sort: each pool keeps the order of its own.
            order.sort_by_key(|(_, environment)| {
                let pool = environment.name.pool();
                pools.iter().position(|known| *known == pool)
            });
        }
        order.into_iter().map(|(index, _)| index).collect()
    }

    /// The pool whose header the workspace draws above the main view's
    /// match at `position`: its environment's pool, when the match above
    /// is of another. None with the current map, and while a filter ranks
    /// the list by how well each name matches instead.
    pub fn group_at(&self, position: usize) -> Option<PoolName> {
        if self.map != Map::Workspace || !self.environments.filter.is_empty() {
            return None;
        }
        let pool_of = |position: usize| {
            let ranked = self.environments.matches.get(position)?;
            let environment = self.snapshot.environments.get(ranked.index)?;
            Some(environment.name.pool())
        };
        let pool = pool_of(position)?;
        let above = position.checked_sub(1).and_then(pool_of);
        (above.as_ref() != Some(&pool)).then_some(pool)
    }

    /// The kernel the kernels view selects.
    pub fn selected_kernel(&self) -> Option<&Kernel> {
        self.focused()?.kernels.get(self.kernels.selected_item()?)
    }

    /// The path above the columns of the Boot and Snapshots tabs: the pool
    /// and the environment selected, then the kernel when its column is
    /// open, the kernel drawn printable, or the snapshot selected in the
    /// snapshots column; above the workspace's Pools tab, the pool, the
    /// vdev and the device selected. Empty without a selection, and in the
    /// other views.
    pub fn path(&self) -> Vec<String> {
        let (environment, last) = match self.screen {
            Screen::Main => (self.selected_environment(), None),
            Screen::Kernels => (
                self.focused(),
                self.selected_kernel()
                    .map(|kernel| kernel.version.printable().to_string()),
            ),
            Screen::Snapshots => (
                self.focused(),
                self.selected_snapshot()
                    .map(|snapshot| snapshot.name.name().to_owned()),
            ),
            Screen::Pools => return self.tree_path(),
            _ => return Vec::new(),
        };
        let Some(environment) = environment else {
            return Vec::new();
        };
        let mut path = vec![
            environment.name.pool().to_string(),
            environment.name.to_string(),
        ];
        path.extend(last);
        path
    }

    /// Selects the kernel `name` again after a refresh, in the workspace,
    /// where a column keeps its selection.
    pub(super) fn keep_kernel(&mut self, name: Option<String>) {
        if self.map != Map::Workspace {
            return;
        }
        let Some(focus) = self.focus.as_ref() else {
            return;
        };
        let kernels = self
            .snapshot
            .environments
            .iter()
            .find(|environment| &environment.name == focus)
            .map(|environment| environment.kernels.as_slice())
            .unwrap_or_default();
        keep(&mut self.kernels, kernels, name, |kernel| {
            kernel.version.get().clone()
        });
    }
}
