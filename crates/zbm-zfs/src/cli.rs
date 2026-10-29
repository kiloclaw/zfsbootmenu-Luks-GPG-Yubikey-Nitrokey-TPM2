//! [`ZfsCli`]: the ZFS port answered by the `zfs` and `zpool` programs.
//!
//! The format is chosen once, on first use: JSON (`-j -p`) when `zfs version
//! -j` works, which it does from OpenZFS 2.3 on, and the `-H -p` text of
//! older versions otherwise. Both give the same types; the fixture tests
//! check that on output recorded from the same pool.

use std::cell::OnceCell;

use zbm_platform::exec::{Command, Input, Runner, Stream};
use zbm_types::{DatasetName, PoolName, SnapshotName};

use crate::change::ZfsChange;
use crate::model::{Dataset, Difference, Pool, Property, Snapshot, Version};
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

    /// Runs `program` with `args` and returns its standard output. A
    /// message that something does not exist is [`ZfsError::Missing`], for
    /// the last argument.
    fn run(&self, program: &str, args: &[&str]) -> Result<String, ZfsError> {
        self.run_command(
            Command::new(program).args(args.iter().copied()),
            args.last().copied(),
        )
    }

    /// Makes `change`, as [`ZfsCli::change`] does.
    fn make(&self, change: &ZfsChange) -> Result<(), ZfsError> {
        let (program, args) = change.command();
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        self.change(program, &args)
    }

    /// Runs `change` as [`ZfsCli::run`] does, its last argument the name a
    /// failure may say is missing.
    fn run_change(&self, change: &ZfsChange) -> Result<(), ZfsError> {
        let (program, args) = change.command();
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        self.run(program, &args).map(drop)
    }

    /// Runs a change: as [`ZfsCli::run`], but every failure is
    /// [`ZfsError::Failed`], as the missing name could be any argument.
    fn change(&self, program: &str, args: &[&str]) -> Result<(), ZfsError> {
        self.run_command(Command::new(program).args(args.iter().copied()), None)
            .map(drop)
    }

    /// Runs `command` with its output captured; a failure whose message says
    /// that something does not exist is [`ZfsError::Missing`] for `missing`.
    fn run_command(&self, command: Command, missing: Option<&str>) -> Result<String, ZfsError> {
        let command = command
            .stdin(Input::Null)
            .stdout(Stream::Capture)
            .stderr(Stream::Capture);
        let output = self
            .runner
            .run(&command)
            .map_err(|source| ZfsError::Start {
                program: command.program().to_string_lossy().into_owned(),
                source,
            })?;
        if output.status.is_success() {
            return Ok(output.stdout_text());
        }
        let message = output.stderr_text().trim().to_owned();
        if let Some(name) = missing
            && (message.contains("does not exist") || message.contains("no such pool"))
        {
            return Err(ZfsError::Missing(name.to_owned()));
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

/// The raw send of a snapshot (`$1`) into a receive (the other arguments),
/// through `mbuffer` when the image has it, as the legacy runtime
/// duplicates a snapshot. The names are arguments, never part of the
/// script; `pipefail` makes a failed send fail the copy.
const SEND_RECEIVE: &str = "set -o pipefail
snapshot=\"$1\"; shift
if command -v mbuffer >/dev/null 2>&1; then
  zfs send -p -w \"$snapshot\" | mbuffer -q | zfs recv \"$@\"
else
  zfs send -p -w \"$snapshot\" | zfs recv \"$@\"
fi";

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

    fn pool_names(&self) -> Result<Vec<PoolName>, ZfsError> {
        let output = self.run("zpool", &["list", "-H", "-o", "name"])?;
        text::pool_names(&output).map_err(|error| ZfsError::Output {
            command: "zpool list -H -o name".to_owned(),
            detail: error.to_string(),
        })
    }

    fn devices(&self, pool: &PoolName) -> Result<Vec<String>, ZfsError> {
        let output = self.run("zpool", &["status", "-L", "-P", pool.as_str()])?;
        Ok(text::status_devices(&output))
    }

    fn export(&self, pool: &PoolName) -> Result<(), ZfsError> {
        self.run_change(&ZfsChange::Export(pool.clone()))
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

    fn snapshot_space(&self, dataset: &DatasetName) -> Result<Vec<(SnapshotName, u64)>, ZfsError> {
        let args = [
            "list",
            "-H",
            "-p",
            "-t",
            "snapshot",
            "-o",
            "name,used",
            "-r",
            dataset.as_str(),
        ];
        let output = self.run("zfs", &args)?;
        text::snapshot_space(&output).map_err(|error| ZfsError::Output {
            command: format!("zfs {}", args.join(" ")),
            detail: error.to_string(),
        })
    }

    fn property(&self, dataset: &DatasetName, property: &str) -> Result<Property, ZfsError> {
        self.one_property("zfs", dataset.as_str(), property)
    }

    fn set(&self, dataset: &DatasetName, property: &str, value: &str) -> Result<(), ZfsError> {
        self.run_change(&ZfsChange::Set {
            dataset: dataset.clone(),
            property: property.to_owned(),
            value: value.to_owned(),
        })
    }

    fn inherit(&self, dataset: &DatasetName, property: &str) -> Result<(), ZfsError> {
        self.run_change(&ZfsChange::Inherit {
            dataset: dataset.clone(),
            property: property.to_owned(),
        })
    }

    fn set_pool_property(
        &self,
        pool: &PoolName,
        property: &str,
        value: &str,
    ) -> Result<(), ZfsError> {
        self.make(&ZfsChange::SetPool {
            pool: pool.clone(),
            property: property.to_owned(),
            value: value.to_owned(),
        })
    }

    fn snapshot_property(
        &self,
        snapshot: &SnapshotName,
        property: &str,
    ) -> Result<Property, ZfsError> {
        self.one_property("zfs", &snapshot.to_string(), property)
    }

    fn own_properties(&self, dataset: &DatasetName) -> Result<Vec<(String, Property)>, ZfsError> {
        let args = [
            "get",
            "-o",
            "name,property,value,source",
            "-s",
            "local,received",
            "all",
            dataset.as_str(),
        ];
        let rows = match self.format() {
            Format::Json => self.listing("zfs", &args, json::properties_of)?,
            Format::Text => self.listing("zfs", &args, text::properties)?,
        };
        Ok(rows
            .into_iter()
            .filter(|(name, _, _)| name == dataset.as_str())
            .map(|(_, property, value)| (property, value))
            .collect())
    }

    fn create_snapshot(&self, snapshot: &SnapshotName) -> Result<(), ZfsError> {
        self.make(&ZfsChange::Snapshot(snapshot.clone()))
    }

    fn rollback(&self, snapshot: &SnapshotName) -> Result<(), ZfsError> {
        self.make(&ZfsChange::Rollback(snapshot.clone()))
    }

    fn clone_snapshot(
        &self,
        snapshot: &SnapshotName,
        target: &DatasetName,
        properties: &[(String, String)],
    ) -> Result<(), ZfsError> {
        self.make(&ZfsChange::Clone {
            snapshot: snapshot.clone(),
            target: target.clone(),
            properties: properties.to_vec(),
        })
    }

    fn promote(&self, dataset: &DatasetName) -> Result<(), ZfsError> {
        self.make(&ZfsChange::Promote(dataset.clone()))
    }

    fn send_receive(
        &self,
        snapshot: &SnapshotName,
        target: &DatasetName,
        properties: &[(String, String)],
    ) -> Result<(), ZfsError> {
        // The shell the runtime chose (brush in Rust images), else Bash.
        let shell = zbm_platform::shell::chosen().unwrap_or_else(|| "bash".into());
        let change = ZfsChange::SendReceive {
            snapshot: snapshot.clone(),
            target: target.clone(),
            properties: properties.to_vec(),
        };
        let (_, receive) = change.command();
        let command = Command::new(shell)
            .args(["-c", SEND_RECEIVE, "zfs-send-receive"])
            .arg(snapshot.to_string())
            .args(receive);
        self.run_command(command, None).map(drop)
    }

    fn inherit_key(&self, dataset: &DatasetName) -> Result<(), ZfsError> {
        self.make(&ZfsChange::InheritKey(dataset.clone()))
    }

    fn diff(
        &self,
        from: &SnapshotName,
        to: Option<&SnapshotName>,
    ) -> Result<Vec<Difference>, ZfsError> {
        let from_text = from.to_string();
        let to_text = to.map_or_else(|| from.dataset().to_string(), ToString::to_string);
        let args = ["diff", "-F", "-H", from_text.as_str(), to_text.as_str()];
        let output = self.run_command(Command::new("zfs").args(args), None)?;
        text::differences(&output).map_err(|error| ZfsError::Output {
            command: format!("zfs {}", args.join(" ")),
            detail: error.to_string(),
        })
    }
}
