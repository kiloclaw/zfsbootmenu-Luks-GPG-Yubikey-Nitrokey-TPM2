//! How pools are imported.

use std::fmt;
use std::str::FromStr;

use crate::error::{ParseError, Reason};

/// The names of [`ImportPolicy`], in the order of its variants.
const NAMES: &[&str] = &["hostid", "force", "strict"];

/// How ZFSBootMenu imports pools that another host used last
/// (`zbm.import_policy`).
///
/// ```
/// use zbm_types::ImportPolicy;
///
/// assert_eq!("force".parse::<ImportPolicy>()?, ImportPolicy::Force);
/// assert_eq!(ImportPolicy::default().to_string(), "hostid");
/// assert!("Force".parse::<ImportPolicy>().is_err());
/// # Ok::<(), zbm_types::ParseError>(())
/// ```
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum ImportPolicy {
    /// Adopt the host ID that a pool was last imported with, when no pool can
    /// be imported with this machine's.
    #[default]
    Hostid,
    /// Import pools that another host may be using (`zpool import -f`).
    Force,
    /// Import only pools that this machine's host ID owns.
    Strict,
}

impl ImportPolicy {
    /// The name, as `zbm.import_policy` gives it.
    pub fn as_str(self) -> &'static str {
        match self {
            ImportPolicy::Hostid => "hostid",
            ImportPolicy::Force => "force",
            ImportPolicy::Strict => "strict",
        }
    }
}

impl FromStr for ImportPolicy {
    type Err = ParseError;

    fn from_str(text: &str) -> Result<ImportPolicy, ParseError> {
        match text {
            "hostid" => Ok(ImportPolicy::Hostid),
            "force" => Ok(ImportPolicy::Force),
            "strict" => Ok(ImportPolicy::Strict),
            _ => Err(ParseError::new(
                "import policy",
                text,
                Reason::NotOneOf(NAMES),
            )),
        }
    }
}

impl fmt::Display for ImportPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn every_policy_round_trips() {
        for policy in [
            ImportPolicy::Hostid,
            ImportPolicy::Force,
            ImportPolicy::Strict,
        ] {
            assert_eq!(policy.to_string().parse::<ImportPolicy>(), Ok(policy));
        }
    }

    #[test]
    fn other_names_say_which_are_known() {
        let error = "hostids".parse::<ImportPolicy>().unwrap_err();

        assert_eq!(
            error.to_string(),
            "\"hostids\" is not a valid import policy: it is not one of hostid, force, strict"
        );
    }
}
