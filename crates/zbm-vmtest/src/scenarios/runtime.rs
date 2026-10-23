//! Not ported yet.

use crate::error::{Error, Result};
use crate::record::Test;

/// Runs a scenario of the group.
///
/// # Errors
///
/// Always, until the group is ported.
pub fn run(_test: &Test, scenario: &str) -> Result<()> {
    Err(Error::Setup(format!(
        "runtime-{scenario} is not ported yet"
    )))
}
