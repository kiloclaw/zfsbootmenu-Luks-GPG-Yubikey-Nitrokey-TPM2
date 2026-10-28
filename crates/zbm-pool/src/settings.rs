//! The settings of pool import, from `/etc/zfsbootmenu.conf`: the import
//! policy, the preferred pool and how much it is required, the preferred
//! boot environment and the delay between retries. Their types are
//! `zbm-types`' ([`PoolRequirement`], [`BootfsPreference`], [`RetryDelay`]).

use zbm_config::conf::Conf;
use zbm_types::{BootfsPreference, ImportPolicy, PoolName, PoolRequirement, RetryDelay};

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
    /// `zbm_prefer_pool`: the pool to import first, if any. A name that no
    /// pool can have is none.
    pub prefer_pool: Option<PoolName>,
    /// `zbm_require_pool`.
    pub requirement: PoolRequirement,
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
            prefer_pool: get("zbm_prefer_pool").parse().ok(),
            requirement: PoolRequirement::from_conf(get("zbm_require_pool")),
            prefer_bootfs: BootfsPreference::from_conf(get("zbm_prefer_bootfs")),
            retry_delay: RetryDelay::or_default(get("zbm_retry_delay")),
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
        );
        let settings = ImportSettings::from_conf(&conf);
        assert_eq!(settings.policy, ImportPolicy::Hostid);
        assert_eq!(settings.prefer_pool, "tank".parse().ok());
        assert_eq!(settings.requirement, PoolRequirement::Only);
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
    fn a_preferred_pool_that_no_pool_can_be_is_none() {
        let conf = |pool: &str| Conf::parse(&format!("export zbm_prefer_pool='{pool}'\n"));
        assert_eq!(ImportSettings::from_conf(&conf("")).prefer_pool, None);
        // `zbm.prefer` takes names; a pool's numeric ID is not one.
        assert_eq!(ImportSettings::from_conf(&conf("123")).prefer_pool, None);
        let spinning = Conf::parse("export zbm_retry_delay='0'\n");
        assert_eq!(
            ImportSettings::from_conf(&spinning).retry_delay,
            RetryDelay::DEFAULT
        );
    }
}
