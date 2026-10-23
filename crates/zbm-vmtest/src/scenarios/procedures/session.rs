//! Running the commands of a documentation section in the recovery shell.

use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::expect::regex;
use crate::record::Test;
use crate::vm::{Sh, Vm};

use crate::scenarios::docs::commands;

/// The page of the vdev configurations.
pub(super) const VDEV: &str = "vdev-configurations.rst";

/// The page of the ZFS versions.
pub(super) const ZV: &str = "zfs-versions.rst";

/// The page of the administration of pools.
pub(super) const ADMIN: &str = "administration.rst";

/// The machine of the procedures and its test.
pub(super) struct Run<'t> {
    /// The test.
    pub test: &'t Test,
    /// The machine, in its recovery shell.
    pub vm: Vm<'t>,
    /// The directory of the documentation pages.
    docs: PathBuf,
}

impl<'t> Run<'t> {
    /// Runs on `vm` the sections of the pages in `docs`.
    pub fn new(vm: Vm<'t>, docs: &Path) -> Run<'t> {
        Run {
            test: vm.test(),
            vm,
            docs: docs.to_owned(),
        }
    }

    /// The commands of a section of `page`.
    ///
    /// # Errors
    ///
    /// When the page cannot be read or has no such section.
    pub fn commands(&self, page: &str, section: &str) -> Result<Vec<String>> {
        commands(&self.docs.join(page), section)
    }

    /// Runs the commands of a documentation section; returns their output.
    ///
    /// # Errors
    ///
    /// See [`Run::doc_with`].
    pub fn doc(&mut self, page: &str, section: &str) -> Result<String> {
        self.doc_with(page, section, &[], &[])
    }

    /// Runs the commands of a documentation section, but not those that
    /// match a pattern of `skip`, with the replacements of `subst`; returns
    /// their output. The first command that fails ends the section, with a
    /// failed check.
    ///
    /// # Errors
    ///
    /// When the section cannot be read, or a command does not end in time.
    pub fn doc_with(
        &mut self,
        page: &str,
        section: &str,
        skip: &[&str],
        subst: &[(&str, &str)],
    ) -> Result<String> {
        let commands = self.commands(page, section)?;
        let skip = skip
            .iter()
            .map(|pattern| regex(pattern))
            .collect::<Result<Vec<_>>>()?;
        self.test.step(&format!(
            "--- {page}: {section} ({} commands)",
            commands.len()
        ));
        let mut out = Vec::new();
        for command in commands {
            if skip.iter().any(|pattern| pattern.is_match(&command)) {
                let first = first_line(&command, 90);
                self.test.step(&format!("  (not run here: {first})"));
                continue;
            }
            let command = substitute(&command, subst);
            let (rc, output) = self.vm.sh_with(&command, Sh::default().timeout(3600))?;
            out.push(output);
            if rc != 0 {
                self.test.show(out.last().map_or("", String::as_str));
                let first = first_line(&command, 100);
                self.test
                    .check(false, &format!("{section}: '{first}' exited {rc}"));
                return Ok(out.join("\n"));
            }
        }
        self.test
            .check(true, &format!("{section}: all commands succeeded"));
        Ok(out.join("\n"))
    }

    /// The status of `pool`, with full paths.
    ///
    /// # Errors
    ///
    /// When the command does not end in time.
    pub fn status(&mut self, pool: &str) -> Result<String> {
        Ok(self.vm.sh(&format!("zpool status -P {pool}"))?.1)
    }

    /// Destroys `pool`, if given, and closes every LUKS mapping (between
    /// examples).
    ///
    /// # Errors
    ///
    /// When a command does not end in time.
    pub fn reset(&mut self, pool: Option<&str>) -> Result<()> {
        if let Some(pool) = pool {
            self.vm.sh(&format!("zpool destroy {pool}"))?;
        }
        self.vm.sh(
            "for m in /dev/mapper/luks-*; do [ -e \"$m\" ] && cryptsetup close \"${m##*/}\"; \
             done; true",
        )?;
        Ok(())
    }
}

/// The first line of `command`, at most `chars` characters of it.
pub(super) fn first_line(command: &str, chars: usize) -> String {
    command
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(chars)
        .collect()
}

/// `command` with each `(from, to)` of `subst` replaced, in order.
pub(super) fn substitute(command: &str, subst: &[(&str, &str)]) -> String {
    subst
        .iter()
        .fold(command.to_owned(), |command, (from, to)| {
            command.replace(from, to)
        })
}

/// The number of matches of `pattern` in `text`.
///
/// # Errors
///
/// When `pattern` is not a valid regular expression.
pub(super) fn count(pattern: &str, text: &str) -> Result<usize> {
    Ok(regex(pattern)?.find_iter(text).count())
}
