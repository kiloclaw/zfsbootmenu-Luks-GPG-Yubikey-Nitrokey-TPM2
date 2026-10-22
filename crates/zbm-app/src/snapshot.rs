//! A snapshot of the machine: what the menu shows.
//!
//! The pilot reads the boot environments with `zpool list` and `zfs list`
//! in `-H` form; R2 replaces this with the JSON adapter of `zbm-zfs`
//! (REFACTOR-ROADMAP.md, "The ZFS boundary").

use std::collections::BTreeSet;

use zbm_platform::{Command, Runner, Stream};
use zbm_types::DatasetName;

/// The machine, as far as a front end needs to know it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Snapshot {
    /// The boot environments, in the order of their names.
    pub boot_environments: Vec<BootEnvironment>,
}

/// A filesystem that can be booted: its `mountpoint` is `/`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BootEnvironment {
    /// The filesystem.
    pub name: DatasetName,
    /// Whether it is the `bootfs` of its pool.
    pub is_default: bool,
    /// Whether it is encrypted, and whether its key is loaded.
    pub encryption: Encryption,
}

/// The encryption of a boot environment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Encryption {
    /// Not encrypted.
    None,
    /// Encrypted, and the key is not loaded.
    Locked,
    /// Encrypted, and the key is loaded.
    Unlocked,
}

impl Snapshot {
    /// A machine with three boot environments in two pools, one of them
    /// locked, for demonstrations and tests of front ends.
    pub fn example() -> Snapshot {
        let environments = [
            ("tank/ROOT/arch", false, Encryption::None),
            ("zroot/ROOT/ubuntu", false, Encryption::Locked),
            ("zroot/ROOT/void", true, Encryption::Unlocked),
        ];
        Snapshot {
            boot_environments: environments
                .into_iter()
                .filter_map(|(name, is_default, encryption)| {
                    Some(BootEnvironment {
                        name: name.parse().ok()?,
                        is_default,
                        encryption,
                    })
                })
                .collect(),
        }
    }
}

/// Why the machine could not be read.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SnapshotError {
    /// A program could not be started.
    #[error("{program} could not be started")]
    Start {
        /// The program.
        program: &'static str,
        /// Why.
        #[source]
        source: std::io::Error,
    },
    /// A program failed.
    #[error("{program} failed with status {status}")]
    Failed {
        /// The program.
        program: &'static str,
        /// Its exit status, as a shell reports it.
        status: i32,
    },
    /// A program printed a line this code does not understand.
    #[error("{program} printed an unexpected line: {line:?}")]
    Parse {
        /// The program.
        program: &'static str,
        /// The line.
        line: String,
    },
}

/// Reads the boot environments of the imported pools.
pub(crate) fn read(runner: &dyn Runner) -> Result<Snapshot, SnapshotError> {
    let pools = run(runner, "zpool", &["list", "-H", "-o", "name,bootfs"])?;
    let defaults = default_environments(&pools)?;
    let datasets = run(
        runner,
        "zfs",
        &[
            "list",
            "-H",
            "-t",
            "filesystem",
            "-o",
            "name,mountpoint,canmount,keystatus",
        ],
    )?;
    let boot_environments = boot_environments(&datasets, &defaults)?;
    Ok(Snapshot { boot_environments })
}

/// Runs `program` with `args` and returns its standard output.
fn run(runner: &dyn Runner, program: &'static str, args: &[&str]) -> Result<String, SnapshotError> {
    let command = Command::new(program).args(args).stdout(Stream::Capture);
    let output = runner
        .run(&command)
        .map_err(|source| SnapshotError::Start { program, source })?;
    if !output.status.success() {
        return Err(SnapshotError::Failed {
            program,
            status: output.status.shell_code(),
        });
    }
    Ok(output.stdout_text())
}

/// The `bootfs` of each pool in `zpool list -H -o name,bootfs` output.
fn default_environments(text: &str) -> Result<BTreeSet<DatasetName>, SnapshotError> {
    let mut defaults = BTreeSet::new();
    for line in text.lines() {
        let parse = || SnapshotError::Parse {
            program: "zpool",
            line: line.to_owned(),
        };
        let (_pool, bootfs) = line.split_once('\t').ok_or_else(parse)?;
        if bootfs != "-" {
            defaults.insert(bootfs.parse().map_err(|_| parse())?);
        }
    }
    Ok(defaults)
}

/// The boot environments in
/// `zfs list -H -o name,mountpoint,canmount,keystatus` output.
fn boot_environments(
    text: &str,
    defaults: &BTreeSet<DatasetName>,
) -> Result<Vec<BootEnvironment>, SnapshotError> {
    let mut environments = Vec::new();
    for line in text.lines() {
        let parse = || SnapshotError::Parse {
            program: "zfs",
            line: line.to_owned(),
        };
        let fields: Vec<&str> = line.split('\t').collect();
        let [name, mountpoint, canmount, keystatus] = fields.as_slice() else {
            return Err(parse());
        };
        if *mountpoint != "/" || *canmount == "off" {
            continue;
        }
        let name: DatasetName = name.parse().map_err(|_| parse())?;
        let encryption = match *keystatus {
            "-" => Encryption::None,
            "available" => Encryption::Unlocked,
            _ => Encryption::Locked,
        };
        environments.push(BootEnvironment {
            is_default: defaults.contains(&name),
            name,
            encryption,
        });
    }
    Ok(environments)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use zbm_testutil::FakeRunner;

    /// A machine with the pools and datasets of `zpool` and `zfs` output.
    fn machine(zpool: &str, zfs: &str) -> FakeRunner {
        let runner = FakeRunner::new();
        runner.respond_program("zpool", 0, zpool);
        runner.respond_program("zfs", 0, zfs);
        runner
    }

    #[test]
    fn filesystems_mounted_at_root_are_boot_environments() {
        let runner = machine(
            "zroot\tzroot/ROOT/void\ntank\t-\n",
            "zroot\tnone\ton\t-\n\
             zroot/ROOT\tnone\toff\t-\n\
             zroot/ROOT/void\t/\tnoauto\tavailable\n\
             zroot/ROOT/old\t/\toff\t-\n\
             zroot/home\t/home\ton\t-\n\
             tank/ROOT/arch\t/\tnoauto\tunavailable\n",
        );

        let snapshot = read(&runner).unwrap();

        assert_eq!(
            snapshot.boot_environments,
            [
                BootEnvironment {
                    name: "zroot/ROOT/void".parse().unwrap(),
                    is_default: true,
                    encryption: Encryption::Unlocked,
                },
                BootEnvironment {
                    name: "tank/ROOT/arch".parse().unwrap(),
                    is_default: false,
                    encryption: Encryption::Locked,
                },
            ]
        );
        assert_eq!(
            runner.lines(),
            [
                "zpool list -H -o name,bootfs",
                "zfs list -H -t filesystem -o name,mountpoint,canmount,keystatus",
            ]
        );
    }

    #[test]
    fn no_pools_means_no_boot_environments() {
        let snapshot = read(&machine("", "")).unwrap();

        assert_eq!(snapshot, Snapshot::default());
    }

    #[test]
    fn a_failing_program_is_an_error_that_names_it() {
        let runner = FakeRunner::new();
        runner.respond_program("zpool", 1, "");

        let error = read(&runner).unwrap_err();

        assert_eq!(error.to_string(), "zpool failed with status 1");
    }

    #[test]
    fn an_unexpected_line_is_an_error_that_quotes_it() {
        let error = read(&machine("", "zroot/ROOT/void\t/\n")).unwrap_err();

        assert_eq!(
            error.to_string(),
            "zfs printed an unexpected line: \"zroot/ROOT/void\\t/\""
        );
    }

    #[test]
    fn the_example_has_a_default_and_a_locked_environment() {
        let example = Snapshot::example();

        assert_eq!(example.boot_environments.len(), 3);
        assert!(example.boot_environments.iter().any(|e| e.is_default));
        assert!(
            example
                .boot_environments
                .iter()
                .any(|e| e.encryption == Encryption::Locked)
        );
    }
}
