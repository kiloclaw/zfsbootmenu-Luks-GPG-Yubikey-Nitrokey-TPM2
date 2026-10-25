//! Helpers that reach the host: the repository, programs on `PATH`, and
//! scratch directories of real files.
#![allow(
    clippy::disallowed_methods,
    reason = "tests that run real programs need real files; \
              these helpers are the one place they are made"
)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use zbm_platform::exec::{Command, Runner as _};

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

/// Whether a test that needs `what` can run: `available` says whether it is
/// there. Without it the test is skipped with a message, unless
/// `ZBM_REQUIRE_TOOLS=1`, which `cargo xtask test` sets: there a missing
/// tool fails the test, so that it never passes by skipping.
///
/// # Panics
///
/// When `what` is missing and `ZBM_REQUIRE_TOOLS=1`.
pub fn can_run(available: bool, what: &str) -> bool {
    let required = std::env::var_os("ZBM_REQUIRE_TOOLS").is_some_and(|value| value == "1");
    can_run_if(available, what, required)
}

/// [`can_run`], where `required` says whether a missing tool fails.
fn can_run_if(available: bool, what: &str, required: bool) -> bool {
    if available {
        return true;
    }
    assert!(
        !required,
        "{what} is missing, and ZBM_REQUIRE_TOOLS=1 requires it"
    );
    #[allow(
        clippy::disallowed_macros,
        reason = "the test harness shows a skipped test's standard error"
    )]
    {
        eprintln!("skipped: {what} is missing");
    }
    false
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
        .stdin(zbm_platform::exec::Input::Bytes(contents.as_ref().to_vec()));
    let output = zbm_platform::exec::SystemRunner
        .run(&command)
        .expect("sh can be started");
    assert!(
        output.status.is_success(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_tool_skips_unless_required() {
        assert!(can_run_if(true, "perl", true));
        assert!(!can_run_if(false, "perl", false));
    }

    #[test]
    #[should_panic(expected = "perl is missing, and ZBM_REQUIRE_TOOLS=1 requires it")]
    fn a_required_tool_that_is_missing_fails() {
        can_run_if(false, "perl", true);
    }

    #[test]
    fn temp_dir_is_removed() {
        let path = {
            let dir = TempDir::new("zbm-testkit");
            let file = dir.write("a/b", "x");
            assert!(file.is_file());
            dir.path().to_owned()
        };
        assert!(!path.exists());
    }

    #[test]
    fn repo_root_holds_the_workspace() {
        assert!(repo_root().join("Cargo.lock").is_file());
        assert!(have_program("sh"));
        assert!(!have_program("zbm-no-such-program"));
    }
}
