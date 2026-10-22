//! A runner that can only look: for shadow mode, which must never change the
//! machine it checks.

use std::io;
use std::time::Duration;

use zbm_core::exec::{Command, Input, Output, Runner};
use zbm_system::Clock;

/// The programs [`ReadOnlyRunner`] runs, each with the first arguments that
/// make it read-only. `blkid` only reads, whatever its arguments.
pub const ALLOWED: &[(&str, &[&str])] = &[
    ("zfs", &["list", "get"]),
    ("zpool", &["list", "get", "status"]),
    ("cryptsetup", &["luksDump", "status", "luksUUID", "isLuks"]),
    ("blkid", &[]),
];

/// The one script Bash may run: the regular expression match that
/// `org.zfsbootmenu:kernel` needs (`zbm_boot::kernels::bash_regex_match`).
pub const BASH_MATCH: &str = "[[ $1 =~ $2 ]]";

/// A [`Runner`] for code that must not act: it runs only the read-only
/// commands of [`ALLOWED`] (and Bash's regular expression match,
/// [`BASH_MATCH`]), never with a secret on standard input and never with the
/// console (it gets `/dev/null` instead), and refuses everything else, and
/// every command once its time budget is spent.
///
/// ```
/// use std::time::Duration;
/// use zbm_platform::{Command, ReadOnlyRunner, Runner};
/// use zbm_testkit::fakes::FakeSystem;
///
/// let fake = FakeSystem::new();
/// let budget = Duration::from_secs(5);
/// let runner = ReadOnlyRunner::new(&fake.runner, &fake.clock, budget);
///
/// assert!(runner.run(&Command::new("zpool").args(["list", "-H"])).is_ok());
/// assert!(runner.run(&Command::new("zpool").args(["import", "-a"])).is_err());
/// assert_eq!(fake.runner.lines(), ["zpool list -H"]);
/// ```
pub struct ReadOnlyRunner<'a> {
    /// The runner that runs the commands allowed.
    inner: &'a dyn Runner,
    /// The clock that measures the budget.
    clock: &'a dyn Clock,
    /// When the budget ends, on `clock`.
    deadline: Duration,
}

impl<'a> ReadOnlyRunner<'a> {
    /// A runner that runs read-only commands through `inner` for `budget`
    /// from now, as `clock` measures it.
    pub fn new(
        inner: &'a dyn Runner,
        clock: &'a dyn Clock,
        budget: Duration,
    ) -> ReadOnlyRunner<'a> {
        ReadOnlyRunner {
            inner,
            clock,
            deadline: clock.now().saturating_add(budget),
        }
    }
}

/// Whether `command` only reads.
pub fn is_read_only(command: &Command) -> bool {
    let program = command.program().to_string_lossy();
    let name = program.rsplit('/').next().unwrap_or_default();
    let args: Vec<String> = command
        .get_args()
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let first = args.first().map_or("", String::as_str);
    if name == "bash" {
        return args
            .get(..2)
            .is_some_and(|start| start == ["-c", BASH_MATCH]);
    }
    ALLOWED.iter().any(|(allowed, subcommands)| {
        *allowed == name && (subcommands.is_empty() || subcommands.contains(&first))
    })
}

impl Runner for ReadOnlyRunner<'_> {
    fn run(&self, command: &Command) -> io::Result<Output> {
        let refuse = |why: &str| {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("{why}: {}", command.display()),
            ))
        };
        if !is_read_only(command) {
            return refuse("not a read-only command");
        }
        if matches!(command.get_stdin(), Input::Secret(_)) {
            return refuse("a read-only command reads no secret");
        }
        if self.clock.now() >= self.deadline {
            return refuse("the time budget is spent");
        }
        if matches!(command.get_stdin(), Input::Inherit) {
            return self.inner.run(&command.clone().stdin(Input::Null));
        }
        self.inner.run(command)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use zbm_core::secret::Secret;
    use zbm_testkit::cases;
    use zbm_testkit::fakes::FakeSystem;

    #[test]
    fn the_allowed_commands_are_pinned() {
        // Adding a command here is a decision: shadow mode must never change
        // the machine it runs on.
        assert_eq!(
            ALLOWED,
            &[
                ("zfs", &["list", "get"][..]),
                ("zpool", &["list", "get", "status"][..]),
                (
                    "cryptsetup",
                    &["luksDump", "status", "luksUUID", "isLuks"][..]
                ),
                ("blkid", &[][..]),
            ]
        );
        assert_eq!(BASH_MATCH, "[[ $1 =~ $2 ]]");
    }

    #[test]
    fn only_reading_commands_are_read_only() {
        cases! {
            for (line, read_only) in {
                "zfs get" => ("zfs get -H -o value bootfs tank", true),
                "zfs list by path" => ("/usr/sbin/zfs list -H", true),
                "zpool status" => ("zpool status -P tank", true),
                "luksUUID" => ("cryptsetup luksUUID /dev/sda2", true),
                "blkid" => ("blkid -o device", true),
                "zpool import" => ("zpool import -N tank", false),
                "zpool export" => ("zpool export tank", false),
                "zfs set" => ("zfs set canmount=on tank", false),
                "zfs load-key" => ("zfs load-key tank", false),
                "cryptsetup open" => ("cryptsetup open /dev/sda2 v", false),
                "kexec" => ("kexec -e", false),
                "bare zpool" => ("zpool", false),
            } {
                let mut words = line.split(' ');
                let command = Command::new(words.next().unwrap()).args(words);
                assert_eq!(is_read_only(&command), read_only);
            }
        }
    }

    #[test]
    fn bash_runs_only_the_regular_expression_match() {
        let matching = Command::new("bash").args(["-c", BASH_MATCH, "bash", "x", "y"]);
        let other = Command::new("bash").args(["-c", "rm -rf /"]);

        assert!(is_read_only(&matching));
        assert!(!is_read_only(&other));
        assert!(!is_read_only(&Command::new("bash")));
    }

    #[test]
    fn secrets_and_a_spent_budget_are_refused_and_the_console_is_not_read() {
        let fake = FakeSystem::new();
        let runner = ReadOnlyRunner::new(&fake.runner, &fake.clock, Duration::from_secs(3));
        let status = Command::new("cryptsetup").args(["status", "v"]);

        let secret = runner.run(
            &status
                .clone()
                .stdin(Input::Secret(Secret::copy_from(b"pw"))),
        );
        let console = runner.run(&status.clone().stdin(Input::Inherit));
        fake.clock.advance(Duration::from_secs(3));
        let after = runner.run(&status.stdin(Input::Null));

        assert!(secret.is_err() && after.is_err());
        assert!(console.is_ok());
        let calls = fake.runner.calls();
        assert_eq!(fake.runner.lines(), ["cryptsetup status v"]);
        assert_eq!(calls[0].get_stdin(), &Input::Null);
        assert!(
            after
                .unwrap_err()
                .to_string()
                .starts_with("the time budget is spent")
        );
    }
}
