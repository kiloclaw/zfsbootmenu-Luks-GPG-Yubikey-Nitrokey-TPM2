//! Listings read row by row: a row that does not parse, such as a name the
//! port refuses, is skipped with a warning instead of hiding every other
//! row.

use std::fmt::Display;

/// Collects the rows of a listing.
pub(crate) trait Readable<T, E>: Iterator<Item = Result<T, E>> + Sized {
    /// The rows that parsed, with a warning for each that did not; the
    /// first error when none parsed, as the output is then of another shape.
    fn readable(self) -> Result<Vec<T>, E>
    where
        E: Display,
    {
        let mut rows = Vec::new();
        let mut errors = Vec::new();
        for row in self {
            match row {
                Ok(row) => rows.push(row),
                Err(error) => errors.push(error),
            }
        }
        if rows.is_empty() && !errors.is_empty() {
            return Err(errors.swap_remove(0));
        }
        for error in errors {
            zbm_trace::warn!("ZFS listed something unreadable, left out: {error}"; %error);
        }
        Ok(rows)
    }
}

impl<T, E, I: Iterator<Item = Result<T, E>>> Readable<T, E> for I {}
