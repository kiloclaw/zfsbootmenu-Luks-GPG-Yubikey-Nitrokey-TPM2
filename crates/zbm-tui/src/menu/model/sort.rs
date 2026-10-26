//! How lists are sorted.

use crate::text::{Id, id};

/// How lists are sorted.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Sort {
    /// By name.
    #[default]
    Name,
    /// Newest first.
    Creation,
    /// Largest first.
    Size,
}

impl Sort {
    /// The next order.
    #[must_use]
    pub fn next(self) -> Sort {
        match self {
            Sort::Name => Sort::Creation,
            Sort::Creation => Sort::Size,
            Sort::Size => Sort::Name,
        }
    }

    /// Its name in the catalogue.
    pub fn id(self) -> Id {
        match self {
            Sort::Name => id!("sort-name"),
            Sort::Creation => id!("sort-creation"),
            Sort::Size => id!("sort-size"),
        }
    }
}
