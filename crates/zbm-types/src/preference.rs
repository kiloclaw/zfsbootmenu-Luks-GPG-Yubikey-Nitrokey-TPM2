//! What `zbm.prefer` asks of pool import: a pool to import first, how much
//! it is required, and a boot environment to prefer.

use std::fmt;

use crate::DatasetName;

/// How much the preferred pool is required (`zbm.prefer=pool!` and
/// `pool!!`, `zbm_require_pool`).
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum PoolRequirement {
    /// The preferred pool is imported first; when it cannot be, any pool is.
    #[default]
    Optional,
    /// The import is retried until the preferred pool is imported, and then
    /// the other pools are imported too (`!`, `yes`).
    Required,
    /// Only the preferred pool is imported (`!!`, `only`).
    Only,
}

impl PoolRequirement {
    /// The requirement `zbm_require_pool` gives: empty for
    /// [`PoolRequirement::Optional`], `only` for [`PoolRequirement::Only`],
    /// anything else (`yes`) for [`PoolRequirement::Required`], as the legacy
    /// stage tests the variable.
    pub fn from_conf(value: &str) -> PoolRequirement {
        match value {
            "" => PoolRequirement::Optional,
            "only" => PoolRequirement::Only,
            _ => PoolRequirement::Required,
        }
    }

    /// The value of `zbm_require_pool`.
    pub fn as_str(self) -> &'static str {
        match self {
            PoolRequirement::Optional => "",
            PoolRequirement::Required => "yes",
            PoolRequirement::Only => "only",
        }
    }

    /// The suffix of `zbm.prefer` that asks for it: none, `!` or `!!`.
    pub fn suffix(self) -> &'static str {
        match self {
            PoolRequirement::Optional => "",
            PoolRequirement::Required => "!",
            PoolRequirement::Only => "!!",
        }
    }
}

impl fmt::Display for PoolRequirement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
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

impl fmt::Display for BootfsPreference {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BootfsPreference::FromPool => Ok(()),
            BootfsPreference::Dataset(dataset) => write!(formatter, "{dataset}"),
            BootfsPreference::Invalid(text) => formatter.write_str(text),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_round_trip_through_the_configuration() {
        for requirement in [
            PoolRequirement::Optional,
            PoolRequirement::Required,
            PoolRequirement::Only,
        ] {
            assert_eq!(
                PoolRequirement::from_conf(requirement.as_str()),
                requirement
            );
        }
        assert_eq!(PoolRequirement::from_conf("1"), PoolRequirement::Required);
        assert_eq!(PoolRequirement::Only.suffix(), "!!");
        for text in ["", "tank/ROOT/void", "tank//x"] {
            assert_eq!(BootfsPreference::from_conf(text).to_string(), text);
        }
        assert!(matches!(
            BootfsPreference::from_conf("tank//x"),
            BootfsPreference::Invalid(_)
        ));
    }
}
