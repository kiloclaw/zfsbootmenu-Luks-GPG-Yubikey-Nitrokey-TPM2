//! ZFS in the Rust port of ZFSBootMenu.
//!
//! - [`Zfs`] and [`Zpool`] run the `zfs` and `zpool` commands, as the legacy code does, and parse their `-H` output.
//! - [`mounts`] reads the mount table and builds the mount and unmount commands of `zfsbootmenu-core.sh`.
//!
//! The commands go through a [`Runner`], so tests can check what would run without a pool.

use std::ffi::OsString;
use std::io;

use zbm_cmdline::parent::{COMMANDLINE_PROPERTY, CommandLineSource};
use zbm_core::exec::{Command, Runner, Stream};
use zbm_core::shell::command_substitution;

pub mod mounts;

/// Splits `-H` output into rows of tab-separated fields.
pub fn parse_table(output: &str) -> Vec<Vec<String>> {
    output
        .lines()
        .map(|line| line.split('\t').map(str::to_owned).collect())
        .collect()
}

/// Runs the `zfs` command.
pub struct Zfs<'a> {
    runner: &'a dyn Runner,
    program: OsString,
}

impl<'a> Zfs<'a> {
    /// Runs `zfs` from `PATH`.
    pub fn new(runner: &'a dyn Runner) -> Zfs<'a> {
        Zfs::with_program(runner, "zfs")
    }

    /// Runs the given program instead of `zfs`, for example a fake in tests.
    pub fn with_program(runner: &'a dyn Runner, program: impl Into<OsString>) -> Zfs<'a> {
        Zfs {
            runner,
            program: program.into(),
        }
    }

    fn command(&self) -> Command {
        Command::new(&self.program)
    }

    /// Whether `dataset` exists (`zfs list -o name -H <dataset>`, quietly).
    pub fn exists(&self, dataset: &str) -> bool {
        let command = self
            .command()
            .args(["list", "-o", "name", "-H", dataset])
            .stdout(Stream::Null)
            .stderr(Stream::Null);
        self.runner
            .run(&command)
            .is_ok_and(|output| output.status.success())
    }

    /// Whether `dataset` is a filesystem (`zfs list -H -o name -t filesystem <dataset>`, quietly).
    pub fn is_filesystem(&self, dataset: &str) -> bool {
        if dataset.is_empty() {
            return false;
        }
        let command = self
            .command()
            .args(["list", "-H", "-o", "name", "-t", "filesystem", dataset])
            .stdout(Stream::Null)
            .stderr(Stream::Null);
        self.runner
            .run(&command)
            .is_ok_and(|output| output.status.success())
    }

    /// The value of a property (`zfs get -H -o value <property> <dataset>`), with the trailing newline removed, or
    /// `None` when the command fails. Error messages of `zfs` go to standard error, as in the legacy code.
    pub fn get_value(&self, property: &str, dataset: &str) -> Option<String> {
        let command = self
            .command()
            .args(["get", "-H", "-o", "value", property, dataset]);
        self.captured(&command)
    }

    /// The source of an inherited property (`zfs get -H -o source -s inherited ...`, errors hidden): `inherited from
    /// <ancestor>`, or empty when the property is not inherited; `None` when the command fails.
    pub fn inherited_source(&self, property: &str, dataset: &str) -> Option<String> {
        let command = self
            .command()
            .args([
                "get",
                "-H",
                "-o",
                "source",
                "-s",
                "inherited",
                property,
                dataset,
            ])
            .stderr(Stream::Null);
        self.captured(&command)
    }

    fn captured(&self, command: &Command) -> Option<String> {
        let output = self.runner.run(command).ok()?;
        output
            .status
            .success()
            .then(|| command_substitution(&output.stdout_text()).to_owned())
    }

    /// Sets a property (`zfs set <property>=<value> <dataset>`); returns whether `zfs` succeeded.
    pub fn set(&self, property: &str, value: &str, dataset: &str) -> io::Result<bool> {
        let command = self
            .command()
            .arg("set")
            .arg(format!("{property}={value}"))
            .arg(dataset)
            .stdout(Stream::Inherit);
        Ok(self.runner.run(&command)?.status.success())
    }

    /// Removes a local property value (`zfs inherit <property> <dataset>`); returns whether `zfs` succeeded.
    pub fn inherit(&self, property: &str, dataset: &str) -> io::Result<bool> {
        let command = self
            .command()
            .args(["inherit", property, dataset])
            .stdout(Stream::Inherit);
        Ok(self.runner.run(&command)?.status.success())
    }
}

impl CommandLineSource for Zfs<'_> {
    fn commandline(&self, filesystem: &str) -> Option<String> {
        self.get_value(COMMANDLINE_PROPERTY, filesystem)
    }

    fn inherited_source(&self, filesystem: &str) -> Option<String> {
        Zfs::inherited_source(self, COMMANDLINE_PROPERTY, filesystem)
    }
}

/// Runs the `zpool` command.
pub struct Zpool<'a> {
    runner: &'a dyn Runner,
    program: OsString,
}

impl<'a> Zpool<'a> {
    /// Runs `zpool` from `PATH`.
    pub fn new(runner: &'a dyn Runner) -> Zpool<'a> {
        Zpool::with_program(runner, "zpool")
    }

    /// Runs the given program instead of `zpool`.
    pub fn with_program(runner: &'a dyn Runner, program: impl Into<OsString>) -> Zpool<'a> {
        Zpool {
            runner,
            program: program.into(),
        }
    }

    /// The names of the imported pools (`zpool list -H -o name`).
    pub fn imported(&self) -> Vec<String> {
        let command = Command::new(&self.program).args(["list", "-H", "-o", "name"]);
        self.runner
            .run(&command)
            .ok()
            .filter(|output| output.status.success())
            .map(|output| {
                parse_table(&output.stdout_text())
                    .into_iter()
                    .filter_map(|row| row.into_iter().next())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The value of a pool property (`zpool get -H -o value <property> <pool>`), or `None` when it fails.
    pub fn get_value(&self, property: &str, pool: &str) -> Option<String> {
        let command =
            Command::new(&self.program).args(["get", "-H", "-o", "value", property, pool]);
        let output = self.runner.run(&command).ok()?;
        output
            .status
            .success()
            .then(|| command_substitution(&output.stdout_text()).to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbm_cmdline::parent::{read_kcl_prop, read_kcl_value};
    use zbm_core::exec::SystemRunner;
    use zbm_testutil::fake_zfs::FakeZfs;
    use zbm_testutil::legacy::{Bash, LOG_STUBS, log_lines};
    use zbm_testutil::{FakeRunner, Rng, TempDir};

    #[test]
    fn commands_match_the_legacy_ones() {
        let runner = FakeRunner::new();
        runner.respond(
            "zfs get -H -o value org.zfsbootmenu:commandline pool/be",
            0,
            "quiet\n",
        );
        runner.respond("zfs list -o name -H missing", 1, "");
        let zfs = Zfs::new(&runner);
        assert_eq!(
            zfs.get_value(COMMANDLINE_PROPERTY, "pool/be").as_deref(),
            Some("quiet")
        );
        assert!(zfs.exists("pool/be"));
        assert!(!zfs.exists("missing"));
        assert!(!zfs.is_filesystem(""));
        assert!(zfs.set(COMMANDLINE_PROPERTY, "a b", "pool/be").unwrap());
        assert!(zfs.inherit(COMMANDLINE_PROPERTY, "pool/be").unwrap());
        assert_eq!(
            runner.lines(),
            [
                "zfs get -H -o value org.zfsbootmenu:commandline pool/be",
                "zfs list -o name -H pool/be",
                "zfs list -o name -H missing",
                "zfs set org.zfsbootmenu:commandline=a b pool/be",
                "zfs inherit org.zfsbootmenu:commandline pool/be",
            ]
        );
    }

    #[test]
    fn zpool_lists_pools() {
        let runner = FakeRunner::new();
        runner.respond("zpool list -H -o name", 0, "rpool\ntank\n");
        runner.respond("zpool get -H -o value readonly tank", 0, "off\n");
        let zpool = Zpool::new(&runner);
        assert_eq!(zpool.imported(), ["rpool", "tank"]);
        assert_eq!(zpool.get_value("readonly", "tank").as_deref(), Some("off"));
        assert_eq!(
            parse_table("a\tb\nc\t\t\n"),
            [vec!["a", "b"], vec!["c", "", ""]]
        );
    }

    const VALUES: &[&str] = &[
        "quiet",
        "%{parent} quiet",
        "%{parent}",
        "a&b %{parent}",
        "x %{parent} y %{parent}",
        "-n",
        "loglevel=7 \\x",
        "spl_hostid=1 a&b\\&c",
    ];

    #[test]
    fn parent_expansion_matches_read_kcl_prop() {
        let mut rng = Rng::from_env(5);
        let datasets = [
            "pool",
            "pool/ROOT",
            "pool/ROOT/a",
            "pool/ROOT/a/b",
            "pool/other",
        ];
        for _ in 0..40 {
            let fake = FakeZfs::new(&datasets);
            for dataset in datasets {
                if rng.below(3) > 0 {
                    let value = *rng.choose(VALUES);
                    fake.set(dataset, COMMANDLINE_PROPERTY, value);
                }
            }
            let dataset = *rng.choose(&datasets[..]);
            let dir = TempDir::new("zbm-zfs-kclfile");
            let kclfile = dir.write("kcl", format!("{}\n", *rng.choose(VALUES)));
            let missing = dir.join("missing");
            let lib = Bash::new()
                .env("PATH", &fake.path_env())
                .code(LOG_STUBS)
                .source("zfsbootmenu/lib/zfsbootmenu-kcl.sh");
            let zfs = Zfs::with_program(&SystemRunner, fake.program());

            let legacy = lib.call("read_kcl_prop", &[dataset], b"");
            let mut log = Vec::new();
            let rust = read_kcl_prop(&zfs, dataset, &mut log);
            assert_eq!(
                rust,
                legacy
                    .status
                    .success()
                    .then(|| legacy.stdout_text().trim_end_matches('\n').to_owned()),
                "dataset {dataset}"
            );
            let messages: String = log
                .iter()
                .map(|m| format!("{}\n", m.stderr_line()))
                .collect();
            assert_eq!(
                messages,
                log_lines(&legacy.stderr_text()),
                "dataset {dataset}"
            );

            for file in [&kclfile, &missing] {
                let file_arg = file.to_string_lossy();
                let legacy = lib.call("read_kcl_value", &[dataset, &file_arg], b"");
                let rust = read_kcl_value(&zfs, dataset, file, &mut Vec::new());
                assert_eq!(
                    rust.as_deref(),
                    Some(legacy.stdout_text().trim_end_matches('\n')),
                    "dataset {dataset}, file {file_arg}"
                );
            }
        }
    }
}
