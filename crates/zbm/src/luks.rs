//! `zluks`, which manages LUKS volumes at start-up, from the recovery shell or
//! over SSH, and in the test build the operations the LUKS unit suites call.

use std::path::Path;

use zbm_app::model::{Action, Event, Outcome};
use zbm_app::{App, NO_CRYPTSETUP, device_not_found};
use zbm_boot::prompt::{ConsolePrompter, colorize};
use zbm_luks::device::{describe_device, is_block_device, is_open};
use zbm_luks::tokens::list_tokens;
use zbm_luks::unlocker::{Luks, Settings, Unlocker};
use zbm_platform::{Context, RealSystem};

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
    let Ok(conf) = zbm_startup::preinit::read_conf(&system, Path::new("/")) else {
        let _ = writeln!(
            io.stderr,
            "ERROR: unable to source {}",
            zbm_config::conf::PATH
        );
        return 1;
    };
    let base = zbm_startup::preinit::base(&conf);
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
    let _log = system.kernel_log(conf.get("loglevel").unwrap_or_default(), &base);
    let prompter = ConsolePrompter::new(system);
    let luks = Luks {
        system,
        settings: &settings,
        prompter: &prompter,
    };
    run_zluks(luks, args, io)
}

/// The action of a `zluks` command that `zbm-app` performs, if it is one;
/// `None` for the other commands, and for `open` without a device.
fn app_action(command: &str, rest: &[String]) -> Option<Action> {
    match command {
        "unlock" => Some(Action::UnlockAll),
        "open" => rest.first().map(|device| Action::Open {
            device: device.clone(),
            name: rest.get(1).cloned().unwrap_or_default(),
        }),
        "close" => Some(Action::Close(rest.first().cloned())),
        _ => None,
    }
}

/// Performs `action` through `zbm-app`; the exit status of `zluks`. A failure
/// that came before any volume was touched is said on standard error, as
/// the legacy script said it; the others are in the log.
fn perform(luks: Luks<'_>, action: &Action, io: &mut Io) -> i32 {
    let app = App::new(Context::new(luks.system)).with_luks(luks);
    match app.act(action).last() {
        Some(Event::Finished(Outcome::Done(_))) => 0,
        Some(Event::Finished(Outcome::Failed { why, .. })) => {
            let not_found = match action {
                Action::Open { device, .. } => Some(device_not_found(device)),
                _ => None,
            };
            let said = why
                .first()
                .filter(|reason| *reason == NO_CRYPTSETUP || not_found.as_ref() == Some(*reason));
            if let Some(reason) = said {
                let _ = writeln!(io.stderr, "ERROR: {reason}");
            }
            1
        }
        _ => 1,
    }
}

/// The commands of `zluks` with the LUKS ports `luks`: unlocking, opening
/// and closing through `zbm-app`, the lists here.
pub fn run_zluks(luks: Luks<'_>, args: &[String], io: &mut Io) -> i32 {
    let command = args.first().map_or("unlock", String::as_str);
    let rest = args.get(1..).unwrap_or_default();
    if let Some(action) = app_action(command, rest) {
        return perform(luks, &action, io);
    }
    let mut session = Unlocker::new(luks);
    if !session.available() {
        let _ = writeln!(io.stderr, "ERROR: {NO_CRYPTSETUP}");
        return 1;
    }
    let session = &mut session;
    match command {
        "open" => {
            let _ = io.stderr.write_all(USAGE.as_bytes());
            1
        }
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
fn print_status(session: &Unlocker<'_>, io: &mut Io) {
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
    for mapping in session.mappings().read().unwrap_or_default() {
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
fn print_list(session: &Unlocker<'_>, io: &mut Io) {
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

    use zbm_luks::device::{OpenCheck, resolve_device};
    use zbm_luks::mappings::Origin;
    use zbm_luks::unlocker::{Answer, PromptRequest, Prompter};
    use zbm_platform::files::Files;
    use zbm_platform::{KernelLog, System};

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
        fn timed_prompt(&self, request: &PromptRequest) -> Answer {
            if let Some(prompts) = &self.prompts {
                let line = format!("timed_prompt {}\n", request.arguments().join(" "));
                let _ = self.files.append(prompts, line.as_bytes());
            }
            std::thread::sleep(Duration::from_secs(self.delay));
            let Some(answers) = &self.answers else {
                return Answer::Escape;
            };
            let text = self.files.read_to_string(answers).unwrap_or_default();
            let (first, rest) = text.split_once('\n').unwrap_or((&text, ""));
            let _ = self.files.write(answers, rest.as_bytes());
            if first == "0" {
                Answer::Continue
            } else {
                Answer::Escape
            }
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
        let _log = log.install();
        let prompter = Recorded {
            files: system.files,
            prompts: environment.non_empty("ZBMTEST_PROMPTS").map(PathBuf::from),
            answers: environment.non_empty("ZBMTEST_ANSWERS").map(PathBuf::from),
            delay: environment
                .non_empty("ZBMTEST_PROMPT_DELAY")
                .and_then(|delay| delay.parse().ok())
                .unwrap_or(0),
        };
        let mut session = Unlocker::new(Luks {
            system,
            settings: &settings,
            prompter: &prompter,
        });
        operation(&mut session, &system, args, io)
    }

    fn operation(
        session: &mut Unlocker<'_>,
        system: &System<'_>,
        args: &[String],
        io: &mut Io,
    ) -> i32 {
        let arg = |index: usize| args.get(index).map_or("", String::as_str);
        let status = |ok: bool| if ok { 0 } else { 1 };
        match arg(0) {
            "unlock_all" => status(session.unlock_all().is_ok()),
            "close_all" => status(session.close_all(args.get(1).map(String::as_str)).is_ok()),
            "resolve_device" => match resolve_device(system, arg(1)) {
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
                let origin = if arg(3) == "1" {
                    Origin::Discovered
                } else {
                    Origin::Configured
                };
                session.mappings().record(arg(1), arg(2), origin);
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
    use zbm_platform::KernelLog;
    use zbm_testkit::fakes::FakeSystem;

    struct NoPrompts;

    impl zbm_luks::unlocker::Prompter for NoPrompts {
        fn timed_prompt(
            &self,
            _: &zbm_luks::unlocker::PromptRequest,
        ) -> zbm_luks::unlocker::Answer {
            zbm_luks::unlocker::Answer::Escape
        }
    }

    fn call(fake: &FakeSystem, args: &[&str]) -> (i32, String, String) {
        let mut settings = Settings::new("/zfsbootmenu");
        settings.open_check = OpenCheck::Markers("/fake/holders".into());
        let _log = KernelLog::new(&fake.files, "7", Path::new("/zfsbootmenu"), 1).install();
        let luks = Luks {
            system: fake.system(),
            settings: &settings,
            prompter: &NoPrompts,
        };
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let code = run_zluks(
            luks,
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
        let unavailable = (
            1,
            String::new(),
            "ERROR: cryptsetup is not available in this image\n".to_owned(),
        );
        for args in [
            &["status"][..],
            &["unlock"],
            &["close"],
            &["open"],
            &["open", "/dev/x"],
        ] {
            assert_eq!(call(&fake, args), unavailable, "{args:?}");
        }
    }

    #[test]
    fn unlock_open_and_close_report_their_outcome_as_the_exit_status() {
        let fake = machine();
        assert_eq!(call(&fake, &["close"]), (0, String::new(), String::new()));
        assert_eq!(call(&fake, &["open", "/dev/sda2", "a1"]).0, 0);
        fake.runner.respond_program("cryptsetup", 2, "");
        let (code, stdout, stderr) = call(&fake, &["open", "/dev/sdb2", "b1"]);
        assert_eq!((code, stdout.as_str(), stderr.as_str()), (1, "", ""));
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
