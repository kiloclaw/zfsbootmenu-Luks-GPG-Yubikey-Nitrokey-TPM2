//! A runner that can only look: for shadow mode, which must never change the
//! machine it checks.

use std::io;
use std::time::Duration;

use crate::Clock;
use crate::exec::{Command, Input, Output, Runner};

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
/// every command once its time budget is spent; a command still running
/// when it ends is killed.
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
    if crate::shell::is_shell(std::path::Path::new(name)) {
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
        let now = self.clock.now();
        if now >= self.deadline {
            return refuse("the time budget is spent");
        }
        // A command that hangs, such as `blkid` on a dead disk, is killed
        // when the budget ends: shadow mode must not hold up the boot.
        let mut bounded = command.clone().timeout(self.deadline.saturating_sub(now));
        if matches!(bounded.get_stdin(), Input::Inherit) {
            bounded = bounded.stdin(Input::Null);
        }
        self.inner.run(&bounded)
    }
}
