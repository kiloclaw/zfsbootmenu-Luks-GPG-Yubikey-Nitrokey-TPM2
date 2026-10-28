//! Environment variables set by the test.

use std::sync::Mutex;

use crate::sync::lock;
use std::collections::BTreeMap;
use std::ffi::OsString;

use zbm_platform::env::Environment;

/// An [`Environment`] holding only the variables the test sets.
#[derive(Default)]
pub struct FakeEnvironment {
    vars: Mutex<BTreeMap<String, OsString>>,
}

impl FakeEnvironment {
    /// An empty environment.
    pub fn new() -> FakeEnvironment {
        FakeEnvironment::default()
    }

    /// Sets a variable.
    pub fn set(&self, name: &str, value: impl Into<OsString>) -> &FakeEnvironment {
        lock(&self.vars).insert(name.to_owned(), value.into());
        self
    }

    /// Removes a variable.
    pub fn remove(&self, name: &str) -> &FakeEnvironment {
        lock(&self.vars).remove(name);
        self
    }
}

impl Environment for FakeEnvironment {
    fn var_os(&self, name: &str) -> Option<OsString> {
        lock(&self.vars).get(name).cloned()
    }

    fn vars_os(&self) -> Vec<(OsString, OsString)> {
        // A BTreeMap of strings is already sorted by their bytes.
        lock(&self.vars)
            .iter()
            .map(|(name, value)| (OsString::from(name), value.clone()))
            .collect()
    }
}
