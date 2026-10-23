//! Not ported yet.

use crate::error::{Error, Result};
use crate::record::Test;

/// Runs the procedures.
///
/// # Errors
///
/// Always, until the group is ported.
pub fn run(_test: &Test) -> Result<()> {
    Err(Error::Setup("procedures is not ported yet".into()))
}
