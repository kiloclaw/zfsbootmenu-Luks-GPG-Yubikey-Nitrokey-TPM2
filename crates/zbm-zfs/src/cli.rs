//! [`ZfsCli`]: the ZFS port answered by the `zfs` and `zpool` programs.
//!
//! The format is chosen once, on first use: JSON (`-j -p`) when `zfs version
//! -j` works, which it does from OpenZFS 2.3 on, and the `-H -p` text of
//! older versions otherwise. Both give the same types; the fixture tests
//! check that on output recorded from the same pool.

use std::cell::OnceCell;

use zbm_platform::exec::{Command, Runner, Stream};
use zbm_types::{DatasetName, PoolName};

use crate::model::{Dataset, Pool, Property, Snapshot, Version};
use crate::port::{Zfs, ZfsError};
use crate::{json, text};

/// The output format of the programs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Format {
    /// `-j -p`, OpenZFS 2.3 and later.
    Json,
    /// `-H -p`.
    Text,
}

/// The ZFS port on the `zfs` and `zpool` programs, run through a [`Runner`].
pub struct ZfsCli<'a> {
    runner: &'a dyn Runner,
    format: OnceCell<Format>,
}

impl<'a> ZfsCli<'a> {
    /// The programs, in the format the installed version supports.
    pub fn new(runner: &'a dyn Runner) -> ZfsCli<'a> {
        ZfsCli {
            runner,
            format: OnceCell::new(),
        }
    }

    /// The programs, always in `format`.
    pub fn with_format(runner: &'a dyn Runner, format: Format) -> ZfsCli<'a> {
        ZfsCli {
            runner,
            format: OnceCell::from(format),
        }
    }

    /// The format in use, found out on first use.
    pub fn format(&self) -> Format {
        *self.format.get_or_init(|| {
            let probe = Command::new("zfs")
                .args(["version", "-j"])
                .stderr(Stream::Null);
            match self.runner.run(&probe) {
                Ok(output) if output.status.is_success() => Format::Json,
                _ => Format::Text,
            }
        })
    }

    /// Runs `program` with `args` and returns its standard output.
    fn run(&self, program: &str, args: &[&str]) -> Result<String, ZfsError> {
        let command = Command::new(program)
            .args(args.iter().copied())
            .stdout(Stream::Capture)
            .stderr(Stream::Capture);
        let output = self
            .runner
            .run(&command)
            .map_err(|source| ZfsError::Start {
                program: program.to_owned(),
                source,
            })?;
        if output.status.is_success() {
            return Ok(output.stdout_text());
        }
        let message = output.stderr_text().trim().to_owned();
        if message.contains("does not exist") || message.contains("no such pool") {
            return Err(ZfsError::Missing(
                args.last().copied().unwrap_or_default().to_owned(),
            ));
        }
        Err(ZfsError::Failed {
            command: command.display(),
            status: output.status.shell_code(),
            message,
        })
    }

    /// Runs a listing in the format in use and parses it with `parse`.
    fn listing<T, E: std::fmt::Display>(
        &self,
        program: &str,
        args: &[&str],
        parse: impl FnOnce(&str) -> Result<T, E>,
    ) -> Result<T, ZfsError> {
        let mut full = Vec::with_capacity(args.len() + 2);
        full.push(args.first().copied().unwrap_or_default());
        match self.format() {
            Format::Json => full.extend(["-j", "-p"]),
            Format::Text => full.extend(["-H", "-p"]),
        }
        full.extend(args.iter().skip(1).copied());
        let output = self.run(program, &full)?;
        parse(&output).map_err(|error| ZfsError::Output {
            command: format!("{program} {}", full.join(" ")),
            detail: error.to_string(),
        })
    }

    /// The property of one name from a `get` listing.
    fn one_property(
        &self,
        program: &str,
        name: &str,
        property: &str,
    ) -> Result<Property, ZfsError> {
        let args = ["get", "-o", "name,property,value,source", property, name];
        let rows = match self.format() {
            Format::Json => self.listing(program, &args, json::properties_of)?,
            Format::Text => self.listing(program, &args, text::properties)?,
        };
        rows.into_iter()
            .find(|(row_name, row_property, _)| row_name == name && row_property == property)
            .map(|(_, _, value)| value)
            .ok_or_else(|| ZfsError::Output {
                command: format!("{program} get {property} {name}"),
                detail: "the property is missing".to_owned(),
            })
    }
}

/// The pool properties [`Zfs::pools`] lists.
const POOL_COLUMNS: &str = "name,health,readonly,bootfs,guid";

impl Zfs for ZfsCli<'_> {
    fn version(&self) -> Result<Version, ZfsError> {
        match self.format() {
            Format::Json => {
                let output = self.run("zfs", &["version", "-j"])?;
                json::version(&output).map_err(|error| ZfsError::Output {
                    command: "zfs version -j".to_owned(),
                    detail: error.to_string(),
                })
            }
            Format::Text => {
                let output = self.run("zfs", &["version"])?;
                text::version(&output).map_err(|error| ZfsError::Output {
                    command: "zfs version".to_owned(),
                    detail: error.to_string(),
                })
            }
        }
    }

    fn pools(&self) -> Result<Vec<Pool>, ZfsError> {
        let args = ["list", "-o", POOL_COLUMNS];
        match self.format() {
            Format::Json => self.listing("zpool", &args, json::pools),
            Format::Text => self.listing("zpool", &args, text::pools),
        }
    }

    fn pool_property(&self, pool: &PoolName, property: &str) -> Result<Property, ZfsError> {
        self.one_property("zpool", pool.as_str(), property)
    }

    fn filesystems(
        &self,
        root: Option<&DatasetName>,
        properties: &[&str],
    ) -> Result<Vec<Dataset>, ZfsError> {
        let columns = std::iter::once("name")
            .chain(properties.iter().copied())
            .collect::<Vec<_>>()
            .join(",");
        let mut args = vec!["list", "-t", "filesystem", "-o", &columns];
        if let Some(root) = root {
            args.extend(["-r", root.as_str()]);
        }
        match self.format() {
            Format::Json => self.listing("zfs", &args, json::datasets),
            Format::Text => self.listing("zfs", &args, |output| text::datasets(output, properties)),
        }
    }

    fn snapshots(&self, dataset: &DatasetName) -> Result<Vec<Snapshot>, ZfsError> {
        let args = [
            "list",
            "-t",
            "snapshot",
            "-o",
            "name,creation",
            "-r",
            dataset.as_str(),
        ];
        match self.format() {
            Format::Json => self.listing("zfs", &args, json::snapshots),
            Format::Text => self.listing("zfs", &args, text::snapshots),
        }
    }

    fn property(&self, dataset: &DatasetName, property: &str) -> Result<Property, ZfsError> {
        self.one_property("zfs", dataset.as_str(), property)
    }

    fn set(&self, dataset: &DatasetName, property: &str, value: &str) -> Result<(), ZfsError> {
        let assignment = format!("{property}={value}");
        self.run("zfs", &["set", &assignment, dataset.as_str()])
            .map(drop)
    }

    fn inherit(&self, dataset: &DatasetName, property: &str) -> Result<(), ZfsError> {
        self.run("zfs", &["inherit", property, dataset.as_str()])
            .map(drop)
    }
}
