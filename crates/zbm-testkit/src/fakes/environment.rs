//! Environment variables set by the test.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ffi::OsString;

use zbm_platform::env::Environment;

/// An [`Environment`] holding only the variables the test sets.
#[derive(Default)]
pub struct FakeEnvironment {
    vars: RefCell<BTreeMap<String, OsString>>,
}

impl FakeEnvironment {
    /// An empty environment.
    pub fn new() -> FakeEnvironment {
        FakeEnvironment::default()
    }

    /// Sets a variable.
    pub fn set(&self, name: &str, value: impl Into<OsString>) -> &FakeEnvironment {
        self.vars.borrow_mut().insert(name.to_owned(), value.into());
        self
    }

    /// Removes a variable.
    pub fn remove(&self, name: &str) -> &FakeEnvironment {
        self.vars.borrow_mut().remove(name);
        self
    }
}

impl Environment for FakeEnvironment {
    fn var_os(&self, name: &str) -> Option<OsString> {
        self.vars.borrow().get(name).cloned()
    }

    fn vars_os(&self) -> Vec<(OsString, OsString)> {
        // A BTreeMap of strings is already sorted by their bytes.
        self.vars
            .borrow()
            .iter()
            .map(|(name, value)| (OsString::from(name), value.clone()))
            .collect()
    }
}
