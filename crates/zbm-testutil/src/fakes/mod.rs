//! Fakes of the system interfaces of `zbm-system`.
//!
//! [`FakeSystem`] holds one fake of each interface and lends them out as a
//! [`System`]. Each fake records what the code did: the commands it ran
//! ([`FakeRunner`]), the files it changed ([`FakeFiles`]), how long it slept
//! ([`FakeClock`]), what it showed and read ([`FakeConsole`]) and the signals
//! it sent ([`FakeProcesses`]). [`FakeSystem::assert_no_leak`] checks all of
//! them for a secret.

mod clock;
mod console;
mod files;
mod processes;

use std::ffi::OsStr;
use std::path::Path;

pub use clock::{FakeClock, START};
pub use console::{ConsoleInput, FakeConsole};
pub use files::{FakeFiles, FileOperation};
pub use processes::FakeProcesses;

use zbm_core::exec::Input;
use zbm_system::System;

use crate::FakeRunner;

/// One fake of each system interface.
pub struct FakeSystem {
    /// External programs.
    pub runner: FakeRunner,
    /// Files: an empty root, `/dev/kmsg`, and nothing else.
    pub files: FakeFiles,
    /// The clock, shared with the console.
    pub clock: FakeClock,
    /// The console, whose timed-out key presses advance the clock.
    pub console: FakeConsole,
    /// Processes; this one has the ID 4242.
    pub processes: FakeProcesses,
}

impl Default for FakeSystem {
    fn default() -> FakeSystem {
        FakeSystem::new()
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

impl FakeSystem {
    /// A new fake system.
    pub fn new() -> FakeSystem {
        let clock = FakeClock::new();
        let files = FakeFiles::new();
        files.device("/dev/kmsg");
        FakeSystem {
            runner: FakeRunner::new(),
            files,
            console: FakeConsole::new().with_clock(clock.clone()),
            clock,
            processes: FakeProcesses::default(),
        }
    }

    /// The fakes as a [`System`].
    pub fn system(&self) -> System<'_> {
        System {
            runner: &self.runner,
            files: &self.files,
            clock: &self.clock,
            console: &self.console,
            processes: &self.processes,
        }
    }

    /// The records written to `/dev/kmsg`.
    pub fn kmsg(&self) -> String {
        self.files.text("/dev/kmsg").unwrap_or_default()
    }

    /// Fails the test if `secret` appears where secrets must never go: in the
    /// program, arguments or environment of a command, in a command's input
    /// given as plain bytes rather than as a secret, in a file written or a
    /// path created, or on the console.
    pub fn assert_no_leak(&self, secret: &[u8]) {
        assert!(!secret.is_empty(), "an empty secret cannot be searched for");
        let in_os = |text: &OsStr| contains(text.as_encoded_bytes(), secret);
        for command in self.runner.calls() {
            let line = command.display();
            assert!(
                !in_os(command.program()) && !command.get_args().iter().any(|arg| in_os(arg)),
                "a secret is in the arguments of `{line}`"
            );
            for (key, value) in command.get_env() {
                assert!(
                    !in_os(key) && !value.as_deref().is_some_and(in_os),
                    "a secret is in the environment of `{line}`"
                );
            }
            if let Input::Bytes(bytes) = command.get_stdin() {
                assert!(
                    !contains(bytes, secret),
                    "a secret is passed to `{line}` as plain bytes instead of Input::Secret"
                );
            }
        }
        let in_path = |path: &Path| in_os(path.as_os_str());
        for operation in self.files.operations() {
            let leaked = match &operation {
                FileOperation::Write(path, contents) | FileOperation::Append(path, contents) => {
                    in_path(path) || contains(contents, secret)
                }
                FileOperation::CreateDir(path)
                | FileOperation::Remove(path)
                | FileOperation::SetMode(path, _) => in_path(path),
                FileOperation::Rename(from, to) => in_path(from) || in_path(to),
            };
            assert!(!leaked, "a secret is in a file operation: {operation:?}");
        }
        assert!(
            !contains(&self.console.output(), secret),
            "a secret was written to the console"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};
    use zbm_core::exec::{Command, Runner, Stream};
    use zbm_core::secret::Secret;
    use zbm_system::{Clock, Console, FileKind, Files, Processes, Signal};

    fn path(text: &str) -> PathBuf {
        PathBuf::from(text)
    }

    #[test]
    fn files_behave_like_a_file_system() {
        let files = FakeFiles::new();
        files.file("/zfsbootmenu/bootfs", "tank/ROOT/one\n");
        assert_eq!(
            files.read_to_string(&path("/zfsbootmenu/bootfs")).unwrap(),
            "tank/ROOT/one\n"
        );
        assert!(files.is_dir(&path("/zfsbootmenu")));
        assert_eq!(
            files.read(&path("/missing")).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert_eq!(
            files.write(&path("/missing/file"), b"").unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert_eq!(
            files
                .write(&path("/zfsbootmenu/bootfs/x"), b"")
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotADirectory
        );
        assert_eq!(
            files.read(&path("/zfsbootmenu")).unwrap_err().kind(),
            io::ErrorKind::IsADirectory
        );
        assert_eq!(
            files.read(&path("relative")).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );

        files
            .append(&path("/zfsbootmenu/degraded"), b"tank\n")
            .unwrap();
        files
            .append(&path("/zfsbootmenu/degraded"), b"data\n")
            .unwrap();
        files
            .write(&path("/zfsbootmenu/bootfs"), b"tank/ROOT/two\n")
            .unwrap();
        assert_eq!(files.text("/zfsbootmenu/degraded").unwrap(), "tank\ndata\n");
        assert_eq!(
            files.text("/zfsbootmenu/bootfs").unwrap(),
            "tank/ROOT/two\n"
        );
        assert_eq!(
            files.operations(),
            [
                FileOperation::Append(path("/zfsbootmenu/degraded"), b"tank\n".to_vec()),
                FileOperation::Append(path("/zfsbootmenu/degraded"), b"data\n".to_vec()),
                FileOperation::Write(path("/zfsbootmenu/bootfs"), b"tank/ROOT/two\n".to_vec()),
            ]
        );
    }

    #[test]
    fn directories_list_sort_and_lock() {
        let files = FakeFiles::new();
        for name in ["b", "B", "10-x", "9-y"] {
            files.file(format!("/hooks/{name}"), "");
        }
        files.dir("/hooks/sub");
        files.file("/hooks/sub/deeper", "");
        assert_eq!(
            files.read_dir(&path("/hooks")).unwrap(),
            ["10-x", "9-y", "B", "b", "sub"]
        );
        assert_eq!(
            files.read_dir(&path("/hooks/b")).unwrap_err().kind(),
            io::ErrorKind::NotADirectory
        );

        files.create_dir(&path("/hooks/lock")).unwrap();
        assert_eq!(
            files.create_dir(&path("/hooks/lock")).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(
            files.remove_dir(&path("/hooks/sub")).unwrap_err().kind(),
            io::ErrorKind::DirectoryNotEmpty
        );
        files.remove_dir(&path("/hooks/lock")).unwrap();
        assert_eq!(
            files.remove_dir(&path("/hooks/b")).unwrap_err().kind(),
            io::ErrorKind::NotADirectory
        );
        assert_eq!(
            files.remove_file(&path("/hooks/sub")).unwrap_err().kind(),
            io::ErrorKind::IsADirectory
        );
        files.remove_file(&path("/hooks/b")).unwrap();
        assert!(!files.exists(&path("/hooks/b")));

        files.create_dir_all(&path("/a/b/c")).unwrap();
        files.create_dir_all(&path("/a/b/c")).unwrap();
        assert!(files.is_dir(&path("/a/b/c")));
        assert_eq!(
            files
                .create_dir_all(&path("/hooks/B/x"))
                .unwrap_err()
                .kind(),
            io::ErrorKind::AlreadyExists
        );
    }

    #[test]
    fn links_resolve_like_readlink() {
        let files = FakeFiles::new();
        files.file("/pool/keys/key.bin", "key");
        files.symlink("/pool/link", "keys/key.bin");
        files.symlink("/pool/escape", "../etc/shadow");
        files.symlink("/pool/dirlink", "/pool/keys");
        files.symlink("/loop", "/loop");
        files.file("/etc/shadow", "");
        assert_eq!(files.read(&path("/pool/link")).unwrap(), b"key");
        assert_eq!(
            files
                .canonicalize(&path("/pool/./dirlink/../keys/key.bin"))
                .unwrap(),
            path("/pool/keys/key.bin")
        );
        assert_eq!(
            files.canonicalize(&path("/pool/escape")).unwrap(),
            path("/etc/shadow")
        );
        assert_eq!(
            files
                .canonicalize(&path("/pool/missing"))
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
        assert!(files.read(&path("/loop")).is_err());
        // Removing a link removes the link, not its target.
        files.remove_file(&path("/pool/link")).unwrap();
        assert!(files.exists(&path("/pool/keys/key.bin")));
    }

    #[test]
    fn metadata_modes_and_times() {
        let files = FakeFiles::new();
        files.file_with_mode("/hooks/10-run", "#!/bin/sh\n", 0o755);
        files.file("/hooks/20-skip", "");
        let metadata = files.metadata(&path("/hooks/10-run")).unwrap();
        assert_eq!(metadata.kind, FileKind::File);
        assert_eq!(metadata.len, 10);
        assert!(metadata.is_executable());
        assert!(
            !files
                .metadata(&path("/hooks/20-skip"))
                .unwrap()
                .is_executable()
        );
        files.set_mode(&path("/hooks/10-run"), 0).unwrap();
        assert!(
            !files
                .metadata(&path("/hooks/10-run"))
                .unwrap()
                .is_executable()
        );
        assert_eq!(
            files.metadata(&path("/dev/kmsg")).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        files.device("/dev/kmsg");
        assert_eq!(
            files.metadata(&path("/dev/kmsg")).unwrap().kind,
            FileKind::Other
        );

        // Every change is one second later than the one before.
        files.write(&path("/older"), b"").unwrap();
        files.write(&path("/newer"), b"").unwrap();
        let older = files.metadata(&path("/older")).unwrap().modified;
        let newer = files.metadata(&path("/newer")).unwrap().modified;
        assert_eq!(newer.duration_since(older).unwrap(), Duration::from_secs(1));
        files.set_modified("/older", SystemTime::UNIX_EPOCH);
        assert_eq!(
            files.metadata(&path("/older")).unwrap().modified,
            SystemTime::UNIX_EPOCH
        );
    }

    #[test]
    fn renames_move_whole_trees_and_replace_files() {
        let files = FakeFiles::new();
        files.file("/a/dir/file", "x");
        files.rename(&path("/a/dir"), &path("/a/moved")).unwrap();
        assert_eq!(files.text("/a/moved/file").unwrap(), "x");
        assert!(!files.exists(&path("/a/dir")));

        files.file("/state/kernels", "old");
        files.replace(&path("/state/kernels"), b"new").unwrap();
        assert_eq!(files.text("/state/kernels").unwrap(), "new");
        assert_eq!(files.read_dir(&path("/state")).unwrap(), ["kernels"]);
        assert_eq!(
            files
                .rename(&path("/missing"), &path("/a/x"))
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
        assert_eq!(
            files
                .rename(&path("/state/kernels"), &path("/a"))
                .unwrap_err()
                .kind(),
            io::ErrorKind::IsADirectory
        );
        files
            .copy(&path("/state/kernels"), &path("/state/copy"))
            .unwrap();
        assert_eq!(files.text("/state/copy").unwrap(), "new");
    }

    #[test]
    fn injected_failures() {
        let files = FakeFiles::new();
        files.dir("/zfsbootmenu");
        files.fail("/zfsbootmenu/bootfs", io::ErrorKind::StorageFull);
        assert_eq!(
            files
                .write(&path("/zfsbootmenu/bootfs"), b"x")
                .unwrap_err()
                .kind(),
            io::ErrorKind::StorageFull
        );
        assert!(files.operations().is_empty());
        files.fail("/zfsbootmenu", io::ErrorKind::PermissionDenied);
        assert!(files.read_dir(&path("/zfsbootmenu")).is_err());
        assert!(files.paths().contains(&path("/zfsbootmenu")));
    }

    #[test]
    fn the_clock_moves_only_when_asked() {
        let clock = FakeClock::new();
        assert_eq!(clock.now(), Duration::ZERO);
        clock.sleep(Duration::from_secs(5));
        clock.advance(Duration::from_millis(500));
        let other = clock.clone();
        assert_eq!(other.now(), Duration::from_millis(5500));
        assert_eq!(clock.sleeps(), [Duration::from_secs(5)]);
        assert_eq!(
            clock.wall(),
            SystemTime::UNIX_EPOCH + START + Duration::from_millis(5500)
        );
    }

    #[test]
    fn the_console_plays_its_script() {
        let clock = FakeClock::new();
        let console = FakeConsole::new().with_clock(clock.clone());
        console.keys(b"\n").timeout().line("y").secret(b"hunter2");
        console.write(b"Booting in 10 seconds").unwrap();
        assert_eq!(
            console.read_key(Some(Duration::from_secs(1))).unwrap(),
            Some(b'\n')
        );
        assert_eq!(
            console.read_key(Some(Duration::from_secs(1))).unwrap(),
            None
        );
        assert_eq!(clock.now(), Duration::from_secs(1));
        assert_eq!(console.read_line().unwrap().as_deref(), Some("y"));
        assert_eq!(console.read_secret().unwrap().unwrap().expose(), b"hunter2");
        assert_eq!(console.read_secret().unwrap(), None);
        assert_eq!(console.read_key(None).unwrap(), None);
        assert_eq!(console.output_text(), "Booting in 10 seconds");
        console.set_size(50, 132);
        assert_eq!(console.size().unwrap().columns, 132);

        console.line("unread").push(ConsoleInput::End);
        assert_eq!(console.remaining().len(), 2);
    }

    #[test]
    #[should_panic(expected = "the code reads a secret, but the script holds Line")]
    fn the_console_fails_on_unexpected_reads() {
        let console = FakeConsole::new();
        console.line("text");
        let _ = console.read_secret();
    }

    #[test]
    fn processes_record_signals() {
        let processes = FakeProcesses::new(100);
        assert_eq!(processes.id(), 100);
        processes.signal(200, Signal::User1).unwrap();
        processes.end(300);
        assert_eq!(
            processes
                .signal(300, Signal::Terminate)
                .unwrap_err()
                .raw_os_error(),
            Some(3)
        );
        assert_eq!(processes.signals(), [(200, Signal::User1)]);
    }

    #[test]
    fn the_system_lends_its_fakes() {
        let fake = FakeSystem::new();
        let system = fake.system();
        system
            .files
            .append(&path("/dev/kmsg"), b"<6>ZFSBootMenu: hello\n")
            .unwrap();
        assert_eq!(fake.kmsg(), "<6>ZFSBootMenu: hello\n");
        let log = system.kernel_log("7", Path::new("/"));
        log.error("broken");
        assert!(fake.kmsg().ends_with("<3>ZFSBootMenu: broken\n"));
        assert!(fake.files.exists(&path("/have_errors")));
        assert_eq!(system.processes.id(), 4242);
    }

    #[test]
    fn leak_checks_pass_for_secrets_on_pipes() {
        let fake = FakeSystem::new();
        let secret = Secret::copy_from(b"hunter2");
        fake.runner
            .run(
                &Command::new("cryptsetup")
                    .args(["open", "--key-file=-", "/dev/vda2", "luks-x"])
                    .stdin(Input::Secret(secret))
                    .stdout(Stream::Null),
            )
            .unwrap();
        fake.files
            .write(&path("/zfsbootmenu-luks"), b"luks-x\t/dev/vda2\t0\n")
            .unwrap();
        fake.console.write(b"Passphrase: \n").unwrap();
        fake.assert_no_leak(b"hunter2");
    }

    fn leaks(setup: impl FnOnce(&FakeSystem)) -> String {
        let fake = FakeSystem::new();
        setup(&fake);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            fake.assert_no_leak(b"hunter2");
        }));
        let error = result.expect_err("the leak was found");
        match error.downcast_ref::<String>() {
            Some(message) => message.clone(),
            None => error
                .downcast_ref::<&str>()
                .map(|message| (*message).to_owned())
                .unwrap_or_default(),
        }
    }

    #[test]
    fn leak_checks_find_secrets_everywhere_else() {
        let run = |command: Command| {
            move |fake: &FakeSystem| {
                fake.runner.run(&command).unwrap();
            }
        };
        assert!(
            leaks(run(Command::new("openssl").arg("pass:hunter2")))
                .contains("arguments of `openssl pass:hunter2`")
        );
        assert!(
            leaks(run(Command::new("gpg").env("PIN", "hunter2"))).contains("environment of `gpg`")
        );
        assert!(
            leaks(run(
                Command::new("cryptsetup").stdin(Input::Bytes(b"hunter2".to_vec()))
            ))
            .contains("as plain bytes")
        );
        assert!(
            leaks(|fake| fake.files.write(&path("/key"), b"xhunter2x").unwrap())
                .contains("file operation")
        );
        assert!(
            leaks(|fake| fake.files.create_dir(&path("/hunter2")).unwrap())
                .contains("file operation")
        );
        assert!(leaks(|fake| fake.console.write(b"hunter2").unwrap()).contains("console"));
    }
}
