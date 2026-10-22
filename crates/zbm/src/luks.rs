//! `zluks`, which manages LUKS volumes at start-up, from the recovery shell or
//! over SSH, and in the test build the operations the LUKS unit suites call.

use std::path::Path;

use zbm_boot::prompt::{ConsolePrompter, colorize};
use zbm_luks::device::{describe_device, is_block_device, is_open, resolve_device};
use zbm_luks::session::{Luks, Session, Settings};
use zbm_luks::tokens::list_tokens;
use zbm_system::RealSystem;

use crate::Io;

/// The usage of `zluks`.
const USAGE: &str = "\
Usage: zluks [command]

Commands:
  unlock               Unlock all configured LUKS volumes (default)
  open <device> [name] Unlock a single LUKS volume; device may be a path or
                       UUID=, PARTUUID=, LABEL=, PARTLABEL=, ID=, PATH=
  close [name]         Export pools on, and close, mappings opened by
                       ZFSBootMenu (all of them unless a name is given)
  status               List mappings opened by ZFSBootMenu
  list                 List all LUKS volumes on the system and their
                       LUKS2 tokens (TPM2, FIDO2, clevis)
  --pilot open ...     Open one volume with a key file through the code of
                       the refactoring (zluks --pilot open --help)

After unlocking, return to ZFSBootMenu (type 'exit') to import pools, or
run 'zpool import' manually.
";

/// `zluks`: as the legacy script, with the configuration of
/// `/etc/zfsbootmenu.conf` (the state directory, the log level and the retry
/// delay) and the LUKS variables of the environment.
pub fn zluks(args: &[String], io: &mut Io) -> i32 {
    if args.first().is_some_and(|arg| arg == "--pilot") {
        return crate::pilot::zluks::zluks(args.get(1..).unwrap_or_default(), io);
    }
    let real = RealSystem::new();
    let terminal = match real.terminal() {
        Ok(terminal) => terminal,
        Err(error) => {
            let _ = writeln!(io.stderr, "zbm: no terminal: {error}");
            return 1;
        }
    };
    let system = real.with_console(&terminal);
    let Ok(conf) = zbm_boot::preinit::read_conf(&system, Path::new("/")) else {
        let _ = writeln!(
            io.stderr,
            "ERROR: unable to source {}",
            zbm_cmdline::conf::PATH
        );
        return 1;
    };
    let base = zbm_boot::preinit::base(&conf);
    let mut settings = Settings::from_environment(system.environment);
    settings.mappings = base.join("luks-mappings");
    settings.skipped = base.join("luks-skipped");
    if let Some(path) = system.environment.non_empty("LUKS_STATE") {
        settings.mappings = path.into();
    }
    if let Some(path) = system.environment.non_empty("LUKS_SKIPPED") {
        settings.skipped = path.into();
    }
    settings.base = base.clone();
    settings.retry_delay = conf.get("zbm_retry_delay").unwrap_or_default().to_owned();
    let log = system.kernel_log(conf.get("loglevel").unwrap_or_default(), &base);
    let prompter = ConsolePrompter::new(system);
    let mut session = Session::new(Luks {
        system,
        log: &log,
        settings: &settings,
        prompter: &prompter,
    });
    run_zluks(&mut session, args, io)
}

/// The commands of `zluks` in a session.
pub fn run_zluks(session: &mut Session<'_>, args: &[String], io: &mut Io) -> i32 {
    if !session.available() {
        let _ = writeln!(
            io.stderr,
            "ERROR: cryptsetup is not available in this image"
        );
        return 1;
    }
    let command = args.first().map_or("unlock", String::as_str);
    let rest = args.get(1..).unwrap_or_default();
    let status = |ok: bool| if ok { 0 } else { 1 };
    match command {
        "unlock" => status(session.unlock_all()),
        "open" => {
            let Some(spec) = rest.first() else {
                let _ = io.stderr.write_all(USAGE.as_bytes());
                return 1;
            };
            let system = *session.system();
            let Some(device) = resolve_device(&system, session.log(), spec) else {
                let _ = writeln!(io.stderr, "ERROR: device '{spec}' not found");
                return 1;
            };
            let name = rest.get(1).map_or("", String::as_str);
            let key = session.zbm_arg("zbm.luks_key").unwrap_or_default();
            let opened = session.open_device(name, &device, &key, "", false);
            session.forget_secrets();
            session.settle();
            status(opened)
        }
        "close" => status(session.close_all(rest.first().map(String::as_str))),
        "status" => {
            print_status(session, io);
            0
        }
        "list" => {
            print_list(session, io);
            0
        }
        "-h" | "--help" | "help" => {
            let _ = io.stdout.write_all(USAGE.as_bytes());
            0
        }
        _ => {
            let _ = io.stderr.write_all(USAGE.as_bytes());
            1
        }
    }
}

/// `zluks status`: the mappings ZFSBootMenu opened, and the pools on them.
fn print_status(session: &Session<'_>, io: &mut Io) {
    let system = *session.system();
    let settings = session.settings();
    let empty = system
        .files
        .metadata(&settings.mappings)
        .map_or(true, |metadata| metadata.len == 0);
    if empty {
        let _ = writeln!(io.stdout, "No LUKS mappings opened by ZFSBootMenu");
        return;
    }
    for mapping in session.read_mappings().unwrap_or_default() {
        let path = settings.mapper.join(&mapping.name);
        let path = path.to_string_lossy();
        let (state, pools) = if is_block_device(system.files, &path) {
            let pools: String = session
                .pools_on_device(&path)
                .iter()
                .map(|pool| format!("{pool} "))
                .collect();
            (colorize("green", "open"), pools)
        } else {
            (colorize("red", "closed"), String::new())
        };
        let pools = if pools.is_empty() {
            String::new()
        } else {
            format!("\tpools: {pools}")
        };
        let _ = writeln!(
            io.stdout,
            "{}\t{}\t{state}{pools}",
            mapping.name, mapping.device
        );
    }
}

/// `zluks list`: every LUKS volume, whether it is in use, and its token
/// types.
fn print_list(session: &Session<'_>, io: &mut Io) {
    let system = *session.system();
    for device in session.discover_devices() {
        let state = if is_open(system.files, &device, &session.settings().open_check) {
            colorize("green", "in use")
        } else {
            colorize("yellow", "locked")
        };
        let kinds: std::collections::BTreeSet<String> = list_tokens(&system, &device)
            .into_iter()
            .map(|token| token.kind)
            .collect();
        let tokens: String = kinds.iter().map(|kind| format!("{kind} ")).collect();
        let tokens = if tokens.is_empty() {
            String::new()
        } else {
            format!("\ttokens: {tokens}")
        };
        let _ = writeln!(
            io.stdout,
            "{}\t{state}{tokens}",
            describe_device(&system, &device)
        );
    }
}

/// The LUKS session of the test build: messages, timed prompts and the check
/// for open volumes go to the files of the unit suites (see
/// `tests/luks/unit/impl.sh`).
#[cfg(feature = "test-seams")]
pub mod seams {
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use zbm_luks::device::OpenCheck;
    use zbm_luks::session::{PromptRequest, Prompter};
    use zbm_system::files::Files;
    use zbm_system::{KernelLog, System};

    use super::*;

    /// Timed prompts recorded in `ZBMTEST_PROMPTS` and answered from
    /// `ZBMTEST_ANSWERS`, one status per line (0 for RETURN), ESCAPE when
    /// there is none, after `ZBMTEST_PROMPT_DELAY` seconds.
    struct Recorded<'a> {
        files: &'a dyn Files,
        prompts: Option<PathBuf>,
        answers: Option<PathBuf>,
        delay: u64,
    }

    impl Prompter for Recorded<'_> {
        fn timed_prompt(&self, request: &PromptRequest) -> bool {
            if let Some(prompts) = &self.prompts {
                let line = format!("timed_prompt {}\n", request.arguments().join(" "));
                let _ = self.files.append(prompts, line.as_bytes());
            }
            std::thread::sleep(Duration::from_secs(self.delay));
            let Some(answers) = &self.answers else {
                return false;
            };
            let text = self.files.read_to_string(answers).unwrap_or_default();
            let (first, rest) = text.split_once('\n').unwrap_or((&text, ""));
            let _ = self.files.write(answers, rest.as_bytes());
            matches!(first, "0")
        }
    }

    /// `zbm-luks-test <operation> [arguments]`: the operations of the LUKS
    /// unit suites (`luks_unlock_all`, `luks_close_all [name]`,
    /// `luks_resolve_device <spec>`, `luks_list_tokens <device>`,
    /// `luks_record_mapping <name> <device> <auto>` and `luks_token_wait`).
    pub fn run(args: &[String], io: &mut Io) -> i32 {
        let real = RealSystem::new();
        let terminal = match real.terminal() {
            Ok(terminal) => terminal,
            Err(error) => {
                let _ = writeln!(io.stderr, "zbm: no terminal: {error}");
                return 1;
            }
        };
        let system = real.with_console(&terminal);
        let environment = system.environment;
        let mut settings = Settings::from_environment(environment);
        settings.colors = false;
        if let Some(dir) = environment.non_empty("ZBMTEST_HOLDERS") {
            settings.open_check = OpenCheck::Markers(dir.into());
        }
        let mut log = KernelLog::new(system.files, "7", &settings.base, std::process::id());
        if let Some(path) = environment.non_empty("ZBMTEST_LOG") {
            log = log.with_device(Path::new(&path)).labelled();
        }
        let prompter = Recorded {
            files: system.files,
            prompts: environment.non_empty("ZBMTEST_PROMPTS").map(PathBuf::from),
            answers: environment.non_empty("ZBMTEST_ANSWERS").map(PathBuf::from),
            delay: environment
                .non_empty("ZBMTEST_PROMPT_DELAY")
                .and_then(|delay| delay.parse().ok())
                .unwrap_or(0),
        };
        let mut session = Session::new(Luks {
            system,
            log: &log,
            settings: &settings,
            prompter: &prompter,
        });
        operation(&mut session, &system, args, io)
    }

    fn operation(
        session: &mut Session<'_>,
        system: &System<'_>,
        args: &[String],
        io: &mut Io,
    ) -> i32 {
        let arg = |index: usize| args.get(index).map_or("", String::as_str);
        let status = |ok: bool| if ok { 0 } else { 1 };
        match arg(0) {
            "unlock_all" => status(session.unlock_all()),
            "close_all" => status(session.close_all(args.get(1).map(String::as_str))),
            "resolve_device" => match resolve_device(system, session.log(), arg(1)) {
                Some(device) => {
                    let _ = writeln!(io.stdout, "{device}");
                    0
                }
                None => 1,
            },
            "list_tokens" => {
                for token in list_tokens(system, arg(1)) {
                    let _ = writeln!(io.stdout, "{token}");
                }
                0
            }
            "record_mapping" => {
                session.record_mapping(arg(1), arg(2), arg(3) == "1");
                0
            }
            "token_wait" => {
                let _ = writeln!(io.stdout, "{}", session.token_wait());
                0
            }
            other => {
                let _ = writeln!(io.stderr, "zbm-luks-test: unknown operation '{other}'");
                crate::USAGE
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbm_luks::device::OpenCheck;
    use zbm_system::KernelLog;
    use zbm_testkit::fakes::FakeSystem;

    struct NoPrompts;

    impl zbm_luks::session::Prompter for NoPrompts {
        fn timed_prompt(&self, _: &zbm_luks::session::PromptRequest) -> bool {
            false
        }
    }

    fn call(fake: &FakeSystem, args: &[&str]) -> (i32, String, String) {
        let mut settings = Settings::new("/zfsbootmenu");
        settings.open_check = OpenCheck::Markers("/fake/holders".into());
        let log = KernelLog::new(&fake.files, "7", Path::new("/zfsbootmenu"), 1);
        let mut session = Session::new(Luks {
            system: fake.system(),
            log: &log,
            settings: &settings,
            prompter: &NoPrompts,
        });
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let code = run_zluks(
            &mut session,
            &args,
            &mut Io {
                stdout: &mut stdout,
                stderr: &mut stderr,
            },
        );
        (
            code,
            String::from_utf8(stdout).unwrap(),
            String::from_utf8(stderr).unwrap(),
        )
    }

    fn machine() -> FakeSystem {
        let fake = FakeSystem::new();
        fake.environment.set("PATH", "/sbin");
        fake.files
            .file_with_mode("/sbin/cryptsetup", "", 0o755)
            .file("/usr/lib/udev/rules.d/95-dm-notify.rules", "")
            .block_device("/dev/sda2")
            .block_device("/dev/sdb2")
            .block_device("/dev/mapper/a1")
            .dir("/fake/holders")
            .file("/fake/holders/sda2", "a1\n");
        fake.runner.respond(
            "blkid -c /dev/null -o device -t TYPE=crypto_LUKS",
            0,
            "/dev/sda2\n/dev/sdb2\n",
        );
        fake.runner.respond("zpool list -H -o name", 0, "zroot\n");
        fake.runner.respond(
            "zpool status -L -P zroot",
            0,
            "config:\n\t/dev/mapper/a1  ONLINE\n",
        );
        fake.runner.respond(
            "cryptsetup luksDump --dump-json-metadata /dev/sdb2",
            0,
            "{\n  \"tokens\":{\n    \"0\":{\n      \"type\":\"systemd-tpm2\"\n    },\n    \
             \"1\":{\n      \"type\":\"clevis\"\n    },\n    \"2\":{\n      \
             \"type\":\"systemd-tpm2\"\n    }\n  }\n}\n",
        );
        fake.runner.respond_program("lsblk", 0, "10G\n");
        fake.runner.respond_program("blkid", 2, "");
        fake
    }

    #[test]
    fn status_lists_mappings_and_their_pools() {
        let fake = machine();
        assert_eq!(
            call(&fake, &["status"]),
            (
                0,
                "No LUKS mappings opened by ZFSBootMenu\n".to_owned(),
                String::new()
            )
        );
        fake.files.file(
            "/zfsbootmenu/luks-mappings",
            "a1\t/dev/sda2\t0\nb1\t/dev/sdb2\t1\n",
        );
        let (code, stdout, _) = call(&fake, &["status"]);
        assert_eq!(code, 0);
        assert_eq!(
            stdout,
            "a1\t/dev/sda2\t\x1b[0;32mopen\x1b[0m\tpools: zroot \n\
             b1\t/dev/sdb2\t\x1b[0;31mclosed\x1b[0m\n"
        );
    }

    #[test]
    fn list_shows_volumes_and_token_types() {
        let fake = machine();
        let (code, stdout, _) = call(&fake, &["list"]);
        assert_eq!(code, 0);
        assert_eq!(
            stdout,
            "/dev/sda2 (10G)\t\x1b[0;32min use\x1b[0m\n\
             /dev/sdb2 (10G)\t\x1b[1;33mlocked\x1b[0m\ttokens: clevis systemd-tpm2 \n"
        );
    }

    #[test]
    fn usage_and_errors() {
        let fake = machine();
        let (code, stdout, _) = call(&fake, &["--help"]);
        assert_eq!(code, 0);
        assert!(stdout.starts_with("Usage: zluks [command]"));
        assert_eq!(call(&fake, &["bogus"]).0, 1);
        assert_eq!(call(&fake, &["open"]).0, 1);
        let (code, _, stderr) = call(&fake, &["open", "LABEL=none"]);
        assert_eq!(
            (code, stderr.as_str()),
            (1, "ERROR: device 'LABEL=none' not found\n")
        );
        let fake = FakeSystem::new();
        assert_eq!(
            call(&fake, &["status"]),
            (
                1,
                String::new(),
                "ERROR: cryptsetup is not available in this image\n".to_owned()
            )
        );
    }

    #[test]
    fn close_exports_and_closes() {
        let fake = machine();
        fake.files
            .file("/zfsbootmenu/luks-mappings", "a1\t/dev/sda2\t0\n");
        fake.runner.respond("cryptsetup close a1", 0, "");
        assert_eq!(call(&fake, &["close"]).0, 0);
        let lines = fake.runner.lines();
        assert!(lines.contains(&"zpool export zroot".to_owned()));
        assert_eq!(
            fake.files.text("/zfsbootmenu/luks-mappings").as_deref(),
            Some("")
        );
    }
}
