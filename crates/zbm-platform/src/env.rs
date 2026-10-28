//! Environment variables.
//!
//! The runtime reads variables such as `SSH_TTY`, and the ones the legacy code
//! sets for a single call (`read_write=yes import_pool tank`), through
//! [`Environment`], so tests set them without touching the test process's own
//! environment. Nothing in the runtime changes its own environment: since the
//! 2024 edition, `std::env::set_var` is `unsafe`, because other threads may
//! read the environment at the same time. Variables for child processes are
//! set on each `Command` instead.

#![allow(
    clippy::disallowed_methods,
    reason = "ProcessEnvironment is where the environment is read"
)]

use std::ffi::{OsStr, OsString};

/// Reads environment variables.
pub trait Environment: Send + Sync {
    /// The value of `name`, if it is set.
    fn var_os(&self, name: &str) -> Option<OsString>;

    /// Every variable, sorted by name.
    fn vars_os(&self) -> Vec<(OsString, OsString)>;

    /// The value of `name` as text, with invalid UTF-8 replaced, if it is set.
    fn var(&self, name: &str) -> Option<String> {
        self.var_os(name)
            .map(|value| value.to_string_lossy().into_owned())
    }

    /// The value of `name` if it is set and not empty, as the shell's
    /// `[ -n "${name}" ]` tests it.
    fn non_empty(&self, name: &str) -> Option<String> {
        self.var(name).filter(|value| !value.is_empty())
    }
}

/// The environment of this process.
#[derive(Clone, Copy, Debug, Default)]
pub struct ProcessEnvironment;

impl Environment for ProcessEnvironment {
    fn var_os(&self, name: &str) -> Option<OsString> {
        // `var_os` panics on names with `=` or NUL, which no variable has.
        if name.is_empty() || name.contains(['=', '\0']) {
            return None;
        }
        std::env::var_os(name)
    }

    fn vars_os(&self) -> Vec<(OsString, OsString)> {
        let mut vars: Vec<(OsString, OsString)> = std::env::vars_os().collect();
        vars.sort_by(|a, b| sort_key(&a.0).cmp(sort_key(&b.0)));
        vars
    }
}

fn sort_key(name: &OsStr) -> &[u8] {
    name.as_encoded_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_process_environment() {
        let environment = ProcessEnvironment;
        // PATH is set wherever the tests run.
        assert_eq!(environment.var_os("PATH"), std::env::var_os("PATH"));
        assert!(environment.var("PATH").is_some());
        assert_eq!(environment.var("ZBM_SURELY_UNSET_VARIABLE"), None);
        assert_eq!(environment.non_empty("ZBM_SURELY_UNSET_VARIABLE"), None);
        assert_eq!(environment.var_os(""), None);
        assert_eq!(environment.var_os("A=B"), None);
        assert_eq!(environment.var_os("A\0B"), None);

        let vars = environment.vars_os();
        assert!(vars.iter().any(|(name, _)| name == "PATH"));
        assert!(
            vars.windows(2)
                .all(|pair| sort_key(&pair[0].0) <= sort_key(&pair[1].0))
        );
    }
}
