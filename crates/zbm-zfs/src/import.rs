//! [`Import`]: how `zpool import` imports pools, as a request whose command
//! line only this crate builds.

use zbm_types::{ImportPolicy, PoolName};

/// Which pools an import imports.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Target<'a> {
    /// Every pool that can be imported (`zpool import -a`).
    #[default]
    All,
    /// One pool.
    Pool(&'a PoolName),
}

/// Whether imported pools can be written.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Access {
    /// Read-only (`-o readonly=on`), as every import of the start-up is.
    #[default]
    ReadOnly,
    /// Read-write (`-o readonly=off`).
    ReadWrite,
}

/// Whether to rewind a pool to its checkpoint.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Rewind {
    /// No.
    #[default]
    No,
    /// Yes (`--rewind-to-checkpoint`); only for one pool.
    ToCheckpoint,
}

/// How `zpool import` imports, without mounting (`-N`).
///
/// ```
/// use zbm_types::{ImportPolicy, PoolName};
/// use zbm_zfs::import::{Access, Import};
///
/// let tank = "tank".parse::<PoolName>().unwrap();
/// let import = Import::pool(&tank)
///     .policy(ImportPolicy::Force)
///     .access(Access::ReadWrite);
/// let args = ["import", "-N", "-f", "-o", "readonly=off", "tank"];
/// assert_eq!(import.arguments(), args);
/// let all = ["import", "-N", "-o", "readonly=on", "-a"];
/// assert_eq!(Import::all().arguments(), all);
/// ```
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Import<'a> {
    target: Target<'a>,
    policy: ImportPolicy,
    access: Access,
    rewind: Rewind,
}

impl<'a> Import<'a> {
    /// An import of every pool, read-only, by this host's ID.
    pub fn all() -> Import<'a> {
        Import::default()
    }

    /// An import of `pool`, read-only, by this host's ID.
    pub fn pool(pool: &'a PoolName) -> Import<'a> {
        Import {
            target: Target::Pool(pool),
            ..Import::default()
        }
    }

    /// An import of `target`: one pool, or every pool when `None`.
    pub fn target(target: Option<&'a PoolName>) -> Import<'a> {
        target.map_or_else(Import::all, Import::pool)
    }

    /// The same import under `policy`: [`ImportPolicy::Force`] imports pools
    /// in use elsewhere (`-f`).
    #[must_use]
    pub fn policy(mut self, policy: ImportPolicy) -> Import<'a> {
        self.policy = policy;
        self
    }

    /// The same import with `access`.
    #[must_use]
    pub fn access(mut self, access: Access) -> Import<'a> {
        self.access = access;
        self
    }

    /// The same import with `rewind`.
    #[must_use]
    pub fn rewind(mut self, rewind: Rewind) -> Import<'a> {
        self.rewind = rewind;
        self
    }

    /// The pools it imports.
    pub fn imports(self) -> Target<'a> {
        self.target
    }

    /// Whether it rewinds to the checkpoint.
    pub fn rewinds(self) -> Rewind {
        self.rewind
    }

    /// The options of `zpool import`.
    fn options(self) -> Vec<&'static str> {
        let mut args = vec!["-N"];
        if self.policy == ImportPolicy::Force {
            args.push("-f");
        }
        args.extend(match self.access {
            Access::ReadWrite => ["-o", "readonly=off"],
            Access::ReadOnly => ["-o", "readonly=on"],
        });
        if self.rewind == Rewind::ToCheckpoint {
            args.push("--rewind-to-checkpoint");
        }
        args
    }

    /// Logs the options, one debug message each.
    pub fn log_options(self) {
        if self.policy == ImportPolicy::Force {
            zbm_trace::debug!("import_policy set: force");
        }
        match self.access {
            Access::ReadWrite => zbm_trace::debug!("read_write set: yes"),
            Access::ReadOnly => zbm_trace::debug!("read_write unset"),
        }
        if self.rewind == Rewind::ToCheckpoint {
            zbm_trace::debug!("rewind_to_checkpoint set: yes");
        }
    }

    /// The pool argument: the pool, or `-a`.
    pub fn pool_argument(self) -> &'a str {
        match self.target {
            Target::Pool(pool) => pool.as_str(),
            Target::All => "-a",
        }
    }

    /// The arguments of `zpool` for this import.
    pub fn arguments(self) -> Vec<&'a str> {
        let mut args = vec!["import"];
        args.extend(self.options());
        args.push(self.pool_argument());
        args
    }
}
