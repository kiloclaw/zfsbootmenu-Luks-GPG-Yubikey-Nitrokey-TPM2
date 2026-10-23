//! The settings of pool import, from `/etc/zfsbootmenu.conf`: the import
//! policy, the preferred pool and how much it is required, the preferred
//! boot environment and the delay between retries.

use std::fmt;

use zbm_config::conf::Conf;
use zbm_types::{DatasetName, ImportPolicy};

/// A pool as `zpool import` takes it: a name or a numeric pool ID, as the
/// user gave it (`zbm.prefer`).
///
/// It is not checked as a [`zbm_types::PoolName`] is, since `zpool import`
/// also takes pool IDs, and a name that no pool can have is passed on as the
/// legacy code passes it, so that `zpool` says why it fails.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct PoolRef(String);

impl PoolRef {
    /// `text` as a pool reference, or `None` when it is empty.
    pub fn new(text: &str) -> Option<PoolRef> {
        (!text.is_empty()).then(|| PoolRef(text.to_owned()))
    }

    /// The reference as `zpool` takes it.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PoolRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// How much the preferred pool is required (`zbm.prefer=pool!` and
/// `pool!!`, `zbm_require_pool`).
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum Requirement {
    /// The preferred pool is imported first; when it cannot be, any pool is.
    #[default]
    Optional,
    /// The import is retried until the preferred pool is imported, and then
    /// the other pools are imported too.
    Required,
    /// Only the preferred pool is imported.
    Only,
}

impl Requirement {
    /// The requirement `zbm_require_pool` gives: empty for
    /// [`Requirement::Optional`], `only` for [`Requirement::Only`], anything
    /// else (`yes`) for [`Requirement::Required`], as the legacy stage tests
    /// the variable.
    pub fn from_conf(value: &str) -> Requirement {
        match value {
            "" => Requirement::Optional,
            "only" => Requirement::Only,
            _ => Requirement::Required,
        }
    }
}

/// The boot environment to prefer (`zbm.prefer` naming a filesystem,
/// `zbm_prefer_bootfs`).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum BootfsPreference {
    /// None: the `bootfs` property of a pool decides.
    #[default]
    FromPool,
    /// This filesystem, if it is one.
    Dataset(DatasetName),
    /// Text that cannot name a filesystem; no boot environment is chosen, as
    /// the legacy stage chooses none after `zfs list` fails on it.
    Invalid(String),
}

impl BootfsPreference {
    /// The preference `zbm_prefer_bootfs` gives.
    pub fn from_conf(value: &str) -> BootfsPreference {
        if value.is_empty() {
            return BootfsPreference::FromPool;
        }
        value.parse().map_or_else(
            |_| BootfsPreference::Invalid(value.to_owned()),
            BootfsPreference::Dataset,
        )
    }
}

/// The seconds between import attempts (`zbm.retry_delay`,
/// `zbm_retry_delay`).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct RetryDelay(i64);

impl RetryDelay {
    /// The delay `zbm_retry_delay` gives: 5 seconds when it is empty, and 0
    /// when it is not a number, as the legacy prompt reads it.
    pub fn from_conf(value: &str) -> RetryDelay {
        RetryDelay(match value {
            "" => 5,
            delay => delay.trim().parse().unwrap_or(0),
        })
    }

    /// The delay in seconds.
    pub fn seconds(self) -> i64 {
        self.0
    }
}

impl Default for RetryDelay {
    fn default() -> RetryDelay {
        RetryDelay(5)
    }
}

/// Whether the image is a release build (`ZBM_RELEASE_BUILD`), which changes
/// what the user is told to do about a foreign host ID or an unsupported pool
/// feature.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum Build {
    /// Built by `generate-zbm` on the user's system.
    #[default]
    Local,
    /// A release image.
    Release,
}

/// The settings of pool import.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ImportSettings {
    /// `import_policy`.
    pub policy: ImportPolicy,
    /// `zbm_prefer_pool`: the pool to import first, if any.
    pub prefer_pool: Option<PoolRef>,
    /// `zbm_require_pool`.
    pub requirement: Requirement,
    /// `zbm_prefer_bootfs`.
    pub prefer_bootfs: BootfsPreference,
    /// `zbm_retry_delay`.
    pub retry_delay: RetryDelay,
    /// `ZBM_RELEASE_BUILD`.
    pub build: Build,
}

impl ImportSettings {
    /// The settings in `conf`, read as the legacy stage reads its variables.
    pub fn from_conf(conf: &Conf) -> ImportSettings {
        let get = |name: &str| conf.get(name).unwrap_or_default();
        ImportSettings {
            // The legacy stage tests for `force` and `hostid`; any other value,
            // or none, imports as `strict` does.
            policy: get("import_policy").parse().unwrap_or(ImportPolicy::Strict),
            prefer_pool: PoolRef::new(get("zbm_prefer_pool")),
            requirement: Requirement::from_conf(get("zbm_require_pool")),
            prefer_bootfs: BootfsPreference::from_conf(get("zbm_prefer_bootfs")),
            retry_delay: RetryDelay::from_conf(get("zbm_retry_delay")),
            build: if get("ZBM_RELEASE_BUILD").is_empty() {
                Build::Local
            } else {
                Build::Release
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn settings_come_from_the_configuration() {
        let conf = Conf::parse(
            "export ZBM_RELEASE_BUILD=\"1\"\n\
             export import_policy='hostid'\n\
             export zbm_prefer_pool='tank'\n\
             export zbm_require_pool='only'\n\
             export zbm_prefer_bootfs='tank/ROOT/a'\n\
             export zbm_retry_delay='7'\n",
        )
        .unwrap();
        let settings = ImportSettings::from_conf(&conf);
        assert_eq!(settings.policy, ImportPolicy::Hostid);
        assert_eq!(settings.prefer_pool, PoolRef::new("tank"));
        assert_eq!(settings.requirement, Requirement::Only);
        assert_eq!(
            settings.prefer_bootfs,
            BootfsPreference::Dataset("tank/ROOT/a".parse().unwrap())
        );
        assert_eq!(settings.retry_delay.seconds(), 7);
        assert_eq!(settings.build, Build::Release);

        let empty = ImportSettings::from_conf(&Conf::default());
        assert_eq!(empty.policy, ImportPolicy::Strict);
        assert_eq!(empty.prefer_pool, None);
        assert_eq!(empty.build, Build::Local);
        assert_eq!(empty.retry_delay, RetryDelay::default());
    }

    #[test]
    fn values_are_read_as_the_legacy_stage_tests_them() {
        assert_eq!(Requirement::from_conf("yes"), Requirement::Required);
        assert_eq!(Requirement::from_conf("anything"), Requirement::Required);
        assert_eq!(Requirement::from_conf(""), Requirement::Optional);
        assert_eq!(RetryDelay::from_conf("").seconds(), 5);
        assert_eq!(RetryDelay::from_conf(" 3 ").seconds(), 3);
        assert_eq!(RetryDelay::from_conf("x").seconds(), 0);
        assert_eq!(
            BootfsPreference::from_conf("tank@snap"),
            BootfsPreference::Invalid("tank@snap".to_owned())
        );
        assert_eq!(BootfsPreference::from_conf(""), BootfsPreference::FromPool);
        assert_eq!(PoolRef::new(""), None);
        assert_eq!(PoolRef::new("123").unwrap().to_string(), "123");
    }
}
