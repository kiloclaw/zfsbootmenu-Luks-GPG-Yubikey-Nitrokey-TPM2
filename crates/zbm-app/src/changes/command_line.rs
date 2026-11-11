//! The command line an environment boots with, saved for every boot (ADR
//! 0016, point 18): `zfs set org.zfsbootmenu:commandline=<line>`, the
//! line one argument with nothing in it expanded, or `zfs inherit` for an
//! empty line. A user property is set without the environment's key.
//!
//! The line is checked first, before the pool is imported read/write, so
//! that a menu over SSH is held to it too: a control character, which no
//! kernel argument holds and a terminal would act on, or more bytes than
//! ZFS keeps of a user property's value, refuses it.

use zbm_config::spec::properties::COMMANDLINE;
use zbm_types::{DatasetName, Untrusted};

use super::{Changes, Failure, done};
use crate::model::{CommandLine, Event};

/// Why `line` cannot be saved, if it cannot: it holds a control
/// character, or more than [`CommandLine::LONGEST`] bytes. The menu
/// refuses the same lines with the same two checks.
fn refusal(line: &str) -> Option<String> {
    if CommandLine::has_control(line) {
        return Some("the command line holds a control character".to_owned());
    }
    CommandLine::is_too_long(line).then(|| {
        format!(
            "the command line has {} bytes, more than the {} ZFS keeps",
            line.len(),
            CommandLine::LONGEST
        )
    })
}

impl Changes<'_, '_> {
    /// Saves `line` as the command line of `environment`, or lets it
    /// inherit its command line without one, or with an empty one.
    pub(super) fn command_line(
        &mut self,
        environment: &DatasetName,
        line: Option<&Untrusted<String>>,
    ) -> Result<Vec<Event>, Failure> {
        let line = line.filter(|line| !line.get().is_empty());
        let what = match line {
            Some(_) => format!("save the command line of {environment}"),
            None => format!("let {environment} inherit its command line"),
        };
        if let Some(why) = line.and_then(|line| refusal(line.get())) {
            return Err(Failure::new(format!("unable to {what}"), vec![why]));
        }
        self.writable(&environment.pool(), &what)?;
        let failed = |error| Failure::zfs(format!("unable to {what}"), &error);
        let Some(line) = line else {
            self.zfs
                .inherit(environment, COMMANDLINE.name)
                .map_err(failed)?;
            return Ok(done(format!("{environment} inherits its command line")));
        };
        self.zfs
            .set(environment, COMMANDLINE.name, line.get())
            .map_err(failed)?;
        Ok(done(format!("the command line of {environment} is saved")))
    }
}
