//! `zluks`, which manages LUKS volumes at start-up, from the recovery shell or
//! over SSH, and in the test build the operations the LUKS unit suites call.

use std::path::Path;
use zbm_types::Printable;

use zbm_app::model::{Action, Event, Outcome};
use zbm_app::{NO_CRYPTSETUP, Volumes, device_not_found};
use zbm_boot::prompt::{ConsoleInteraction, colorize};
use zbm_luks::device::{describe_device, is_block_device, is_open};
use zbm_luks::tokens::list_tokens;
use zbm_luks::unlocker::{Luks, Settings, Unlocker};
use zbm_platform::{Context, RealSystem};
use zbm_types::{MappingName, ParseError, RetryDelay};

use crate::Io;

/// The usage of `zluks`.
const USAGE: &str = "\
Usage: zluks [command]

Commands:
  unlock               Unlock all configured LUKS volumes (default)
  open <device> [name] Unlock a single LUKS volume; device may be a path or
                       UUID=, PARTUUID=, LABEL=, PARTLABEL=, ID=, PATH=
  open <device> <name> --key-file <path> [--allow-discards] [--read-only]
       [--journal <file>] [--verbose]
                       Open a LUKS volume with a key file and nothing else,
                       with a JSON line in <file> for every event
  close [name]         Export pools on, and close, mappings opened by
                       ZFSBootMenu (all of them unless a name is given)
  status               List mappings opened by ZFSBootMenu
  list                 List all LUKS volumes on the system and their
                       LUKS2 tokens (TPM2, FIDO2, clevis)

After unlocking, return to ZFSBootMenu (type 'exit') to import pools, or
run 'zpool import' manually.
";

/// The LUKS settings of `zluks` and `zbm debug luks`: the state directory of
/// `conf`, its retry delay, and the LUKS variables of the environment.
pub fn settings(system: &zbm_platform::System<'_>, conf: &zbm_config::conf::Conf) -> Settings {
    let base = zbm_startup::preinit::base(conf);
    let mut settings = Settings::from_environment(system.environment);
    settings.mappings = base.join("luks-mappings");
    settings.skipped = base.join("luks-skipped");
    if let Some(path) = system.environment.non_empty("LUKS_STATE") {
        settings.mappings = path.into();
    }
    if let Some(path) = system.environment.non_empty("LUKS_SKIPPED") {
        settings.skipped = path.into();
    }
    settings.base = base;
    settings.retry_delay = RetryDelay::or_default(conf.get("zbm_retry_delay").unwrap_or_default());
    settings
}

/// `zluks`: as the legacy script, with the configuration of
/// `/etc/zfsbootmenu.conf` (the state directory, the log level and the retry
/// delay) and the LUKS variables of the environment.
pub fn zluks(args: &[String], io: &mut Io) -> i32 {
    if key_file::is_asked(args) {
        let installed = key_file::parse(args.get(1..).unwrap_or_default())
            .and_then(|parsed| key_file::install_tracing(&parsed));
        if let Err(message) = installed {
            let _ = writeln!(io.stderr, "zluks: {message}");
            let _ = io.stderr.write_all(USAGE.as_bytes());
            return crate::USAGE;
        }
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
    let settings = settings(&system, &conf);
    let _log = system.kernel_log(conf.get("loglevel").unwrap_or_default(), &settings.base);
    let prompter = ConsoleInteraction::new(system).colors(settings.colors);
    let luks = Luks {
        context: Context::new(system),
        settings: &settings,
        interaction: &prompter,
    };
    run_zluks(luks, args, io)
}

/// The action of a `zluks` command that `zbm-app` performs, if it is one,
/// or why its mapping name is not one; `None` for the other commands, and
/// for `open` without a device.
fn app_action(command: &str, rest: &[String]) -> Option<Result<Action, ParseError>> {
    // An empty name is the default name, as in the legacy script.
    let name = |at: usize| {
        rest.get(at)
            .filter(|name| !name.is_empty())
            .map(|name| name.parse::<MappingName>())
            .transpose()
    };
    match command {
        "unlock" => Some(Ok(Action::UnlockAll)),
        "open" => rest.first().map(|device| {
            Ok(Action::Open {
                device: device.clone(),
                name: name(1)?,
            })
        }),
        "close" => Some(name(0).map(Action::Close)),
        _ => None,
    }
}

/// Performs `action` through `zbm-app`; the exit status of `zluks`. A failure
/// that came before any volume was touched is said on standard error, as
/// the legacy script said it; the others are in the log.
fn perform(luks: Luks<'_>, action: &Action, io: &mut Io) -> i32 {
    match Volumes::new(luks).apply(action).last() {
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
    if key_file::is_asked(args) {
        return match key_file::parse(rest) {
            Ok(parsed) => key_file::report(&Volumes::new(luks).apply(&parsed.action), io),
            Err(message) => {
                let _ = writeln!(io.stderr, "zluks: {message}");
                crate::USAGE
            }
        };
    }
    match app_action(command, rest) {
        Some(Ok(action)) => return perform(luks, &action, io),
        Some(Err(error)) => {
            let _ = writeln!(io.stderr, "ERROR: {error}");
            return 1;
        }
        None => {}
    }
    let mut session = Unlocker::new(luks);
    if !session.available() {
        let _ = writeln!(io.stderr, "ERROR: {NO_CRYPTSETUP}");
        return 1;
    }
    let session = &mut session;
    // `open` without the app's arguments is a usage error, as an unknown
    // word is.
    match command {
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
    let context = *session.context();
    let settings = session.settings();
    let empty = context
        .files()
        .metadata(&settings.mappings)
        .map_or(true, |metadata| metadata.len == 0);
    if empty {
        let _ = writeln!(io.stdout, "No LUKS mappings opened by ZFSBootMenu");
        return;
    }
    for mapping in session.mappings().read().unwrap_or_default() {
        let path = settings.mapper.join(&mapping.name);
        let path = path.to_string_lossy();
        let (state, pools) = if is_block_device(context.files(), &path) {
            let pools: String = session
                .pools_on_device(&path)
                .iter()
                .flat_map(|pool| [pool.as_str(), " "])
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
            Printable(&mapping.name),
            Printable(&mapping.device)
        );
    }
}

/// `zluks list`: every LUKS volume, whether it is in use, and its token
/// types.
fn print_list(session: &Unlocker<'_>, io: &mut Io) {
    let context = *session.context();
    for device in session.discover_devices() {
        let state = if is_open(
            context.files(),
            device.as_str(),
            &session.settings().open_check,
        ) {
            colorize("green", "in use")
        } else {
            colorize("yellow", "locked")
        };
        let kinds: std::collections::BTreeSet<String> = list_tokens(&context, device.as_str())
            .into_iter()
            .map(|token| token.kind)
            .collect();
        let tokens: String = kinds.iter().flat_map(|kind| [kind.as_str(), " "]).collect();
        let tokens = if tokens.is_empty() {
            String::new()
        } else {
            format!("\ttokens: {tokens}")
        };
        let _ = writeln!(
            io.stdout,
            "{}\t{state}{tokens}",
            describe_device(&context, device.as_str())
        );
    }
}

mod key_file;

#[cfg(test)]
mod tests {
    use super::*;
    use zbm_luks::device::OpenCheck;
    use zbm_platform::KernelLog;
    use zbm_testkit::fakes::FakeSystem;

    fn call(fake: &FakeSystem, args: &[&str]) -> (i32, String, String) {
        let mut settings = Settings::new("/zfsbootmenu");
        settings.open_check = OpenCheck::Markers("/fake/holders".into());
        let _log = KernelLog::new(&fake.files, "7", Path::new("/zfsbootmenu"), 1).install();
        let luks = Luks {
            context: fake.context(),
            settings: &settings,
            interaction: &zbm_platform::interaction::NoInteraction,
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
    fn a_key_file_alone_opens_a_volume_or_says_why_not() {
        let fake = machine();
        fake.files.file("/keys/b.key", "secret");
        let (code, stdout, stderr) = call(
            &fake,
            &["open", "/dev/sdb2", "data", "--key-file", "/keys/b.key"],
        );
        assert_eq!((code, stderr.as_str()), (0, ""));
        assert_eq!(stdout, "opened /dev/sdb2 as /dev/mapper/data (key file)\n");
        // The key file is read where it is, and no other method is tried.
        let lines = fake.runner.lines();
        assert!(
            lines.contains(
                &"cryptsetup open --type luks --key-file=/keys/b.key /dev/sdb2 data".to_owned()
            ),
            "{lines:?}"
        );
        assert!(
            !lines.iter().any(|line| line.contains("luksDump")),
            "{lines:?}"
        );

        let missing = machine();
        let (code, stdout, stderr) = call(
            &missing,
            &["open", "/dev/sdb2", "data", "--key-file", "/keys/none.key"],
        );
        assert_eq!((code, stdout.as_str()), (1, ""));
        assert_eq!(
            stderr,
            "zluks: unable to open /dev/sdb2\n  no key method opened /dev/sdb2\n  \
             KeyFile: the key file /keys/none.key of /dev/sdb2 could not be read\n"
        );
        let (code, _, stderr) = call(&missing, &["open", "/dev/sdb2", "--key-file", "/k"]);
        assert_eq!(
            (code, stderr.as_str()),
            (crate::USAGE, "zluks: expected DEVICE and NAME\n")
        );
    }

    #[test]
    fn a_name_that_is_no_mapping_name_is_refused() {
        let fake = machine();
        for args in [&["open", "/dev/sdb2", "--help"][..], &["close", "a/b"]] {
            let (code, stdout, stderr) = call(&fake, args);
            assert_eq!(code, 1, "{args:?}");
            assert_eq!(stdout, "");
            assert!(stderr.starts_with("ERROR: "), "{stderr}");
            assert!(stderr.contains("is not a valid mapping name"), "{stderr}");
        }
        assert!(
            fake.runner
                .lines()
                .iter()
                .all(|line| !line.starts_with("cryptsetup open"))
        );
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
