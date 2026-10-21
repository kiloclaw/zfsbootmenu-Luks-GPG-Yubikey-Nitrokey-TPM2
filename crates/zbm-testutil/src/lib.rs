//! Test helpers of the Rust port of ZFSBootMenu.
//!
//! Only tests use this crate (as a dev-dependency), so nothing here reaches a
//! binary or the boot image.
//!
//! - [`FakeRunner`] replaces the system's programs: it records every command
//!   and answers it.
//! - [`fakes`] replaces the other system interfaces of `zbm-system` (files, the
//!   clock, the console, processes) and bundles them as a `FakeSystem`.
//! - [`legacy`] runs the Bash and Perl code in `legacy/`, so parity tests can
//!   compare it with the Rust code.
//! - [`Rng`] generates test inputs from a seed, so a failing generated case can
//!   be reproduced.
//! - [`TempDir`] is a scratch directory that is removed afterwards.

use std::cell::RefCell;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use zbm_core::exec::{Command, Output, Runner, Status};

pub mod fake_zfs;
pub mod fakes;
pub mod legacy;

/// A response function: returns the output for commands it handles and `None`
/// for the others.
type Responder = Box<dyn Fn(&Command) -> Option<io::Result<Output>>>;

/// A [`Runner`] that records commands and answers them without running
/// anything.
///
/// Responders are tried in the order they were added; a command no responder
/// handles succeeds with no output.
#[derive(Default)]
pub struct FakeRunner {
    calls: RefCell<Vec<Command>>,
    responders: RefCell<Vec<Responder>>,
}

impl FakeRunner {
    /// A runner with no responders.
    pub fn new() -> FakeRunner {
        FakeRunner::default()
    }

    /// Adds a responder.
    pub fn on(&self, responder: impl Fn(&Command) -> Option<io::Result<Output>> + 'static) {
        self.responders.borrow_mut().push(Box::new(responder));
    }

    /// Answers commands whose program and arguments, joined by spaces, equal
    /// `line`.
    pub fn respond(&self, line: &str, status: u8, stdout: &str) {
        let line = line.to_owned();
        let output = Output::new(Status::exited(status), stdout.as_bytes());
        self.on(move |command| (command.display() == line).then(|| Ok(output.clone())));
    }

    /// Answers commands whose program is `program`, whatever their arguments.
    pub fn respond_program(&self, program: &str, status: u8, stdout: &str) {
        let program = program.to_owned();
        let output = Output::new(Status::exited(status), stdout.as_bytes());
        self.on(move |command| (command.program() == program.as_str()).then(|| Ok(output.clone())));
    }

    /// The commands run so far.
    pub fn calls(&self) -> Vec<Command> {
        self.calls.borrow().clone()
    }

    /// The commands run so far, each as its program and arguments joined by spaces.
    pub fn lines(&self) -> Vec<String> {
        self.calls.borrow().iter().map(Command::display).collect()
    }
}

impl Runner for FakeRunner {
    fn run(&self, command: &Command) -> io::Result<Output> {
        self.calls.borrow_mut().push(command.clone());
        self.responders
            .borrow()
            .iter()
            .find_map(|responder| responder(command))
            .unwrap_or_else(|| Ok(Output::new(Status::exited(0), Vec::new())))
    }
}

/// The root of the repository.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the repository root exists")
}

/// Whether `program` can be found in `PATH`, so tests that need it can skip
/// otherwise.
pub fn have_program(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|dir| {
            zbm_core::fs::is_executable(dir.join(program)) && dir.join(program).is_file()
        })
    })
}

/// Writes an executable file through a child process (`cat`), so that this
/// process never has it open for writing.
///
/// A test that writes a program and then runs it can otherwise fail with "Text
/// file busy": another test thread may fork a child at the moment the file is
/// open, and the child holds it open until it executes its own program.
pub fn write_executable(path: &Path, contents: impl AsRef<[u8]>) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("the parent directory can be created");
    }
    let command = Command::new("sh")
        .args(["-c", "cat > \"$1\" && chmod 755 \"$1\"", "sh"])
        .arg(path)
        .stdin(zbm_core::exec::Input::Bytes(contents.as_ref().to_vec()));
    let output = zbm_core::exec::SystemRunner
        .run(&command)
        .expect("sh can be started");
    assert!(
        output.status.success(),
        "{} could not be written",
        path.display()
    );
}

/// A scratch directory, removed with its contents when dropped.
#[derive(Debug)]
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// Creates an empty directory in the system's temporary directory.
    pub fn new(prefix: &str) -> TempDir {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!("{prefix}-{}-{unique}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("the scratch directory can be created");
        TempDir { path }
    }

    /// The directory.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// A path inside the directory.
    pub fn join(&self, name: impl AsRef<Path>) -> PathBuf {
        self.path.join(name)
    }

    /// Writes an executable file inside the directory (see [`write_executable`])
    /// and returns its path.
    pub fn write_executable(&self, name: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> PathBuf {
        let path = self.path.join(name);
        write_executable(&path, contents);
        path
    }

    /// Writes a file inside the directory, creating parent directories, and returns
    /// its path.
    pub fn write(&self, name: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> PathBuf {
        let path = self.path.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("the parent directory can be created");
        }
        std::fs::write(&path, contents).expect("the file can be written");
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// A small random number generator for generated test inputs (xorshift64*).
///
/// The seed comes from `ZBM_TEST_SEED` when set, so a failing case can be run
/// again; generated-input tests print the seed they used.
#[derive(Clone, Debug)]
pub struct Rng {
    state: u64,
}

impl Rng {
    /// A generator with the given seed.
    pub fn new(seed: u64) -> Rng {
        Rng { state: seed.max(1) }
    }

    /// A generator seeded from `ZBM_TEST_SEED`, or with `default` when it is not
    /// set.
    pub fn from_env(default: u64) -> Rng {
        let seed = std::env::var("ZBM_TEST_SEED")
            .ok()
            .and_then(|seed| seed.parse().ok())
            .unwrap_or(default);
        eprintln!("generated inputs use ZBM_TEST_SEED={seed}");
        Rng::new(seed)
    }

    /// The next random number.
    pub fn next_u64(&mut self) -> u64 {
        self.state ^= self.state >> 12;
        self.state ^= self.state << 25;
        self.state ^= self.state >> 27;
        self.state.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// A number below `bound`, which must not be 0.
    pub fn below(&mut self, bound: usize) -> usize {
        let bound = u64::try_from(bound).expect("the bound fits in 64 bits");
        usize::try_from(self.next_u64() % bound).expect("the result is below the bound")
    }

    /// One of the given items.
    pub fn choose<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }

    /// A string of up to `max_len` characters taken from `alphabet`.
    pub fn string(&mut self, alphabet: &[char], max_len: usize) -> String {
        let len = self.below(max_len + 1);
        (0..len).map(|_| *self.choose(alphabet)).collect()
    }

    /// A string made of up to `max_parts` pieces taken from `pieces`.
    pub fn join(&mut self, pieces: &[&str], max_parts: usize) -> String {
        let count = self.below(max_parts + 1);
        (0..count).map(|_| *self.choose(pieces)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_runner_records_and_answers() {
        let runner = FakeRunner::new();
        runner.respond("zfs list -H -o name pool", 0, "pool\n");
        runner.respond_program("objcopy", 1, "");
        let listed = runner
            .run(&Command::new("zfs").args(["list", "-H", "-o", "name", "pool"]))
            .unwrap();
        assert_eq!(listed.stdout_text(), "pool\n");
        let copied = runner.run(&Command::new("objcopy").arg("x")).unwrap();
        assert_eq!(copied.status.code(), Some(1));
        let other = runner.run(&Command::new("true")).unwrap();
        assert!(other.status.success());
        assert_eq!(
            runner.lines(),
            ["zfs list -H -o name pool", "objcopy x", "true"]
        );
    }

    #[test]
    fn generator_is_reproducible() {
        let mut first = Rng::new(42);
        let mut second = Rng::new(42);
        let a: Vec<u64> = (0..5).map(|_| first.next_u64()).collect();
        let b: Vec<u64> = (0..5).map(|_| second.next_u64()).collect();
        assert_eq!(a, b);
        assert!(Rng::new(7).below(3) < 3);
        let text = Rng::new(9).string(&['a', 'b'], 8);
        assert!(text.len() <= 8 && text.chars().all(|c| c == 'a' || c == 'b'));
    }

    #[test]
    fn temp_dir_is_removed() {
        let path = {
            let dir = TempDir::new("zbm-testutil");
            let file = dir.write("a/b", "x");
            assert!(file.is_file());
            dir.path().to_owned()
        };
        assert!(!path.exists());
    }

    #[test]
    fn repo_root_holds_the_legacy_code() {
        assert!(repo_root().join("legacy/zfsbootmenu/zbm-release").is_file());
        assert!(have_program("sh"));
        assert!(!have_program("zbm-no-such-program"));
    }
}
