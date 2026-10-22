//! Pool import: the start-up stage `init.d/50-import-pools`.
//!
//! The Rust counterparts of `import_pool`, `check_for_pools`, `match_hostid`
//! and `log_unimportable` in `zfsbootmenu-core.sh`, and of the stage that uses
//! them: the import loop with its retries, the restriction of unhealthy pools,
//! the check for unsupported features and the choice of `bootfs`.

use zbm_cmdline::conf::Conf;
use zbm_core::exec::{Command, Stream};
use zbm_core::shell::{command_substitution, read_lines};
use zbm_state::State;
use zbm_system::{Runner, System};
use zbm_trace::Level;
use zbm_zfs::Zfs;

use crate::commandline;
use crate::hostid;
use crate::prompt::{Answer, TimedPrompt, colorize, retry_delay};
use crate::recovery::Recovery;
use zbm_types::ImportPolicy;

/// The settings of the stage, from `/etc/zfsbootmenu.conf`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Settings {
    /// `import_policy`.
    pub import_policy: ImportPolicy,
    /// `zbm_prefer_pool`: the pool to import first, or empty.
    pub prefer_pool: String,
    /// `zbm_require_pool`: `yes` or `only` when the preferred pool is required,
    /// or empty.
    pub require_pool: String,
    /// `zbm_prefer_bootfs`: the boot environment to prefer, or empty.
    pub prefer_bootfs: String,
    /// `zbm_retry_delay`: seconds between retries, or empty for 5.
    pub retry_delay: String,
    /// `ZBM_RELEASE_BUILD` is set: the image is a release build.
    pub release_build: bool,
}

impl Settings {
    /// The settings `conf` gives, as the stage reads its variables.
    pub fn from_conf(conf: &Conf) -> Settings {
        let get = |name: &str| conf.get(name).unwrap_or_default().to_owned();
        Settings {
            // The legacy stage tests for `force` and `hostid`; any other value,
            // or none, imports as `strict` does.
            import_policy: get("import_policy").parse().unwrap_or(ImportPolicy::Strict),
            prefer_pool: get("zbm_prefer_pool"),
            require_pool: get("zbm_require_pool"),
            prefer_bootfs: get("zbm_prefer_bootfs"),
            retry_delay: get("zbm_retry_delay"),
            release_build: !get("ZBM_RELEASE_BUILD").is_empty(),
        }
    }
}

/// How `import_pool` imports.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Import<'a> {
    /// The pool; empty imports every pool (`-a`).
    pub pool: &'a str,
    /// `import_policy` is `force`: import pools in use elsewhere (`-f`).
    pub force: bool,
    /// `read_write` is set: import read-write.
    pub read_write: bool,
    /// `rewind_to_checkpoint` is set: rewind the pool to its checkpoint.
    pub rewind: bool,
}

/// The lines `while read -r line ... done <<< "$( command )"` reads.
fn here_lines(output: &str) -> Vec<String> {
    read_lines(&format!("{}\n", command_substitution(output)))
}

/// The standard output of `command`, empty when it cannot start.
fn stdout(runner: &dyn Runner, command: &Command) -> String {
    runner
        .run(command)
        .map(|output| output.stdout_text())
        .unwrap_or_default()
}

/// Imports pools without mounting them, as `import_pool` does, and returns
/// whether `zpool import` succeeded.
pub fn import_pool(system: &System<'_>, state: &State<'_>, import: Import<'_>) -> bool {
    let pool = if !import.pool.is_empty() {
        zbm_trace::debug!("pool set to {}", import.pool);
        import.pool
    } else if import.rewind {
        zbm_trace::error!("rewind only works on a specific pool");
        return false;
    } else {
        zbm_trace::debug!("attempting to import all pools");
        "-a"
    };

    let mut args = vec!["-N"];
    if import.force {
        args.push("-f");
        zbm_trace::debug!("import_policy set: force");
    }
    if import.read_write {
        args.extend(["-o", "readonly=off"]);
        zbm_trace::debug!("read_write set: yes");
    } else {
        args.extend(["-o", "readonly=on"]);
        zbm_trace::debug!("read_write unset");
    }
    if import.rewind {
        args.push("--rewind-to-checkpoint");
        zbm_trace::debug!("rewind_to_checkpoint set: yes");
    }
    zbm_trace::debug!("zpool import arguments: {} {pool}", args.join(" "));

    let command = Command::new("zpool")
        .arg("import")
        .args(&args)
        .arg(pool)
        .merge_stderr();
    let (code, output) = match system.runner.run(&command) {
        Ok(output) => (output.status.shell_code(), output.stdout_text()),
        Err(error) => (126, error.to_string()),
    };
    if code == 0 {
        zbm_trace::debug!("successful pool import");
        return true;
    }
    let mut messages = Vec::new();
    let hostid = commandline::spl_hostid(system.files, state, &mut messages);
    for message in &messages {
        message.emit();
    }
    zbm_trace::debug!(
        "import process failed with code {code}, apparent hostid {}",
        hostid.as_deref().unwrap_or("unknown")
    );
    zbm_trace::debug!("{}", command_substitution(&output));
    false
}

/// Whether any pool is imported (`check_for_pools`).
pub fn check_for_pools(runner: &dyn Runner) -> bool {
    let command = Command::new("zpool")
        .args(["list", "-H", "-o", "name"])
        .stderr(Stream::Null);
    here_lines(&stdout(runner, &command))
        .iter()
        .any(|pool| !pool.is_empty())
}

/// The importable pools `zpool import` lists as `ONLINE`, leaving out the
/// preferred pool, as `match_hostid` reads them.
pub fn online_pools(output: &str, prefer_pool: &str) -> Vec<String> {
    let mut pools = Vec::new();
    let mut pool = String::new();
    for line in here_lines(output) {
        if line.starts_with("pool") {
            pool = line.strip_prefix("pool: ").unwrap_or(&line).to_owned();
        } else if line.starts_with("state") {
            let state = line.strip_prefix("state: ").unwrap_or(&line);
            if state == "ONLINE" && !pool.is_empty() && pool != prefer_pool {
                pools.push(std::mem::take(&mut pool));
            }
        }
    }
    pools
}

/// The digits `hostid=([A-Fa-f0-9]{1,8})` captures in `text`: the first
/// `hostid=` followed by a hex digit, and up to eight digits.
pub fn scrape_hostid(text: &str) -> Option<&str> {
    text.match_indices("hostid=").find_map(|(start, needle)| {
        let digits = &text[start + needle.len()..];
        let len = digits
            .bytes()
            .take(8)
            .take_while(u8::is_ascii_hexdigit)
            .count();
        (len > 0).then(|| &digits[..len])
    })
}

/// Imports a pool that belongs to another host with that host's ID, as
/// `match_hostid` does: `single`, or else each `ONLINE` pool that is not the
/// preferred one. Returns the pool and the host ID written to `/etc/hostid`.
pub fn match_hostid(
    system: &System<'_>,
    state: &State<'_>,
    settings: &Settings,
    single: &str,
) -> Option<(String, String)> {
    let importable = if single.is_empty() {
        let list = Command::new("zpool").arg("import").stderr(Stream::Null);
        online_pools(&stdout(system.runner, &list), &settings.prefer_pool)
    } else {
        vec![single.to_owned()]
    };
    zbm_trace::debug!("importable pools: {}", importable.join(" "));

    for pool in importable {
        zbm_trace::debug!("trying to import: {pool}");
        let probe = Command::new("zpool")
            .args(["import", "-o", "readonly=on", "-N", &pool])
            .merge_stderr();
        let output = stdout(system.runner, &probe);
        let Some(hostid) = scrape_hostid(command_substitution(&output)).and_then(hostid::normalize)
        else {
            zbm_trace::debug!("unable to scrape hostid for {pool}, skipping");
            continue;
        };
        zbm_trace::debug!("discovered pool owner hostid: {hostid}");

        if hostid::write_hostid(system.files, &hostid).is_none() {
            zbm_trace::debug!("failed to set hostid {hostid}, skipping import of pool {pool}");
            continue;
        }

        let import = Import {
            pool: &pool,
            force: settings.import_policy == ImportPolicy::Force,
            ..Import::default()
        };
        if import_pool(system, state, import) {
            zbm_trace::debug!("successfully imported {pool}");
            if !settings.release_build {
                zbm_trace::warn!("imported {pool} with assumed hostid {hostid}");
                zbm_trace::warn!(
                    "set spl_hostid={hostid} on ZBM KCL or regenerate with corrected /etc/hostid"
                );
            }
            return Some((pool, hostid));
        }
    }
    None
}

/// Logs why each `UNAVAIL` pool cannot be imported, as `log_unimportable`
/// does.
pub fn log_unimportable(runner: &dyn Runner) {
    let list = Command::new("zpool").arg("import").stderr(Stream::Null);
    let mut id = String::new();
    for line in here_lines(&stdout(runner, &list)) {
        if line.starts_with("pool") {
            continue;
        } else if line.starts_with("id") {
            id = line.strip_prefix("id: ").unwrap_or(&line).to_owned();
        } else if line.starts_with("state") {
            if line.strip_prefix("state: ").unwrap_or(&line) == "UNAVAIL" {
                let import = Command::new("zpool")
                    .args(["import", "-N", &id])
                    .merge_stderr();
                for error in here_lines(&stdout(runner, &import)) {
                    zbm_trace::error!("{}", error);
                }
            }
            id.clear();
        }
    }
}

/// The two fields `IFS=$'\t' read -r first rest` reads from a line.
pub(crate) fn tab_fields(line: &str) -> (&str, &str) {
    let line = line.trim_matches('\t');
    match line.split_once('\t') {
        Some((first, rest)) => (first, rest.trim_start_matches('\t')),
        None => (line, ""),
    }
}

/// The lines `while IFS=$'\t' read -r ...; done <<< "$( command )"` reads:
/// only tabs are trimmed.
pub(crate) fn tab_lines(output: &str) -> Vec<String> {
    let text = format!("{}\n", command_substitution(output));
    let mut lines: Vec<String> = text
        .split('\n')
        .map(|line| line.replace('\0', ""))
        .collect();
    lines.pop();
    lines
}

/// What the stage found.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Imported {
    /// The default boot environment (`BOOTFS`), if one was found.
    pub bootfs: Option<String>,
    /// The host ID that host ID matching wrote, if it matched one.
    pub spl_hostid: Option<String>,
}

/// The start-up stage `init.d/50-import-pools`.
pub fn stage(
    system: &System<'_>,
    state: &State<'_>,
    settings: &Settings,
    recovery: &mut dyn Recovery,
) -> Imported {
    let mut imported = Imported::default();
    let mut try_pool = settings.prefer_pool.clone();
    let mut attempt = 0_u64;

    loop {
        // On a retry, first unlock LUKS volumes on disks that appeared during the
        // delay, so that a pool is not imported without them.
        if attempt > 0 && recovery.unlock_late() {
            zbm_trace::info!("unlocked LUKS volumes that appeared late");
        }
        if !try_pool.is_empty() {
            zbm_trace::debug!("attempting to import preferred pool {try_pool}");
        }
        let import = Import {
            pool: &try_pool,
            force: settings.import_policy == ImportPolicy::Force,
            ..Import::default()
        };
        import_pool(system, state, import);

        if check_for_pools(system.runner) {
            if settings.require_pool == "only" {
                zbm_trace::debug!("only importing {try_pool}");
                break;
            } else if !try_pool.is_empty() {
                // A single pool was imported: try the others too.
                try_pool.clear();
                continue;
            }
            break;
        } else if settings.import_policy == ImportPolicy::Hostid
            && let Some((pool, hostid)) = match_hostid(system, state, settings, &try_pool)
        {
            zbm_trace::debug!("match_hostid returned: {pool};{hostid}");
            if let Err(error) = state.set_spl_hostid(&hostid) {
                zbm_trace::error!("{}", error.to_string());
            }
            imported.spl_hostid = Some(hostid);
            if !try_pool.is_empty() && settings.require_pool == "only" {
                break;
            }
            // Pick up the other pools of this host ID.
            try_pool.clear();
            continue;
        }
        if !try_pool.is_empty() && settings.require_pool.is_empty() {
            // The preferred pool is not required: try any other pool.
            try_pool.clear();
            continue;
        }

        attempt += 1;
        zbm_trace::info!("unable to import a pool on attempt {attempt}");
        if recovery.unlock_late() {
            zbm_trace::info!("unlocked LUKS volumes that appeared late, retrying import");
            continue;
        }

        let pool = if try_pool.is_empty() {
            "pool"
        } else {
            &try_pool
        };
        let pool = colorize("magenta", pool);
        let answer = TimedPrompt::new()
            .delay(retry_delay(&settings.retry_delay))
            .prompt(move |seconds| {
                let seconds = colorize("yellow", &format!("{seconds:02}"));
                format!("Unable to import {pool}, retrying in {seconds} seconds")
            })
            .on_return("to retry immediately")
            .on_escape("for a recovery shell")
            .run(system);
        if answer == Answer::Continue {
            continue;
        }
        log_unimportable(system.runner);
        recovery.emergency_shell("unable to successfully import a pool");
    }

    restrict_unhealthy(system.runner, state);
    // `zdebug && zdebug "$( zreport )"`: the first call writes an empty line.
    // The system report itself arrives with the recovery tools (package F).
    if zbm_trace::enabled(Level::Debug) {
        zbm_trace::debug!("");
    }
    check_unsupported(system, state, settings);
    imported.bootfs = find_bootfs(system.runner, state, settings);
    imported
}

/// Restricts read-write access to every pool that is not `ONLINE`.
fn restrict_unhealthy(runner: &dyn Runner, state: &State<'_>) {
    let list = Command::new("zpool").args(["list", "-H", "-o", "name,health"]);
    for line in tab_lines(&stdout(runner, &list)) {
        let (pool, health) = tab_fields(&line);
        if health != "ONLINE" {
            if let Err(error) = state.restrict(pool) {
                zbm_trace::error!("{}", error.to_string());
            }
            zbm_trace::error!("prohibiting read/write operations on {pool}");
        }
    }
}

/// Restricts pools with features this build of ZFS does not support, and
/// tells the user.
fn check_unsupported(system: &System<'_>, state: &State<'_>, settings: &Settings) {
    let get = Command::new("zpool").args(["get", "all", "-H", "-o", "name,property"]);
    let mut unsupported = false;
    for line in tab_lines(&stdout(system.runner, &get)) {
        let (pool, property) = tab_fields(&line);
        if !property.contains("unsupported@") {
            continue;
        }
        zbm_trace::error!("unsupported property: {property}");
        if !state.is_degraded(pool)
            && let Err(error) = state.restrict(pool)
        {
            zbm_trace::error!("{}", error.to_string());
        }
        unsupported = true;
    }
    if !unsupported {
        return;
    }
    let upgrade = if settings.release_build {
        "Check for a new binary release of ZFSBootMenu"
    } else {
        "Upgrade ZFS modules in ZFSBootMenu with generate-zbm"
    };
    zbm_trace::error!("Unsupported features detected, {upgrade}");
    TimedPrompt::new()
        .message(colorize("red", "Unsupported features detected"))
        .message(colorize("red", upgrade))
        .run(system);
}

/// The default boot environment: `zbm.prefer` naming a filesystem, or else the
/// `bootfs` property of the preferred pool or of the first imported pool that
/// sets it. It is recorded in the state directory.
fn find_bootfs(runner: &dyn Runner, state: &State<'_>, settings: &Settings) -> Option<String> {
    let mut bootfs = String::new();
    if !settings.prefer_bootfs.is_empty() {
        if Zfs::new(runner).is_filesystem(&settings.prefer_bootfs) {
            bootfs = settings.prefer_bootfs.clone();
        } else {
            zbm_trace::error!(
                "bootfs value '{}' not a ZFS dataset",
                settings.prefer_bootfs
            );
        }
    } else {
        let pool = if settings.prefer_pool.is_empty() {
            "--"
        } else {
            &settings.prefer_pool
        };
        let list = Command::new("zpool").args(["list", "-H", "-o", "bootfs", pool]);
        for line in here_lines(&stdout(runner, &list)) {
            if line == "-" {
                bootfs.clear();
            } else {
                bootfs = line;
                break;
            }
        }
    }
    if bootfs.is_empty() {
        return None;
    }
    if let Err(error) = state.set_bootfs(&bootfs) {
        zbm_trace::error!("{}", error.to_string());
    }
    Some(bootfs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::path::{Path, PathBuf};
    use zbm_system::{Installed, KernelLog};

    use serde::{Deserialize, Serialize};
    use zbm_core::exec::{Output, Status, SystemRunner};
    use zbm_testkit::fakes::FakeSystem;
    use zbm_testkit::legacy::{Bash, log_lines, path};
    use zbm_testkit::parity::Parity;
    use zbm_testkit::{TempDir, parity};

    use crate::commandline::{ETC_HOSTID, SPL_HOSTID};
    use crate::testing::{LINE_LOG_STUBS, Redirect, stub_lines};

    fn log(fake: &FakeSystem) -> Installed {
        fake.files.dir("/zfsbootmenu");
        KernelLog::new(&fake.files, "7", Path::new("/zfsbootmenu"), 1).install()
    }

    #[test]
    fn settings_come_from_the_configuration() {
        let conf = Conf::parse(
            "export ZBM_RELEASE_BUILD=\"1\"\n\
             export import_policy='hostid'\n\
             export zbm_prefer_pool='tank'\n\
             export zbm_require_pool='only'\n\
             export zbm_retry_delay='7'\n",
        )
        .unwrap();
        let settings = Settings::from_conf(&conf);
        assert_eq!(settings.import_policy, ImportPolicy::Hostid);
        assert_eq!(settings.prefer_pool, "tank");
        assert_eq!(settings.require_pool, "only");
        assert_eq!(settings.prefer_bootfs, "");
        assert_eq!(retry_delay(&settings.retry_delay), 7);
        assert!(settings.release_build);
        assert!(!Settings::from_conf(&Conf::default()).release_build);
        assert_eq!(retry_delay(""), 5);
        assert_eq!(retry_delay("x"), 0);
    }

    #[test]
    fn import_arguments_follow_the_options() {
        let fake = FakeSystem::new();
        let _log = log(&fake);
        let state = State::new(&fake.files, "/zfsbootmenu");
        let system = fake.system();
        assert!(import_pool(&system, &state, Import::default()));
        let import = Import {
            pool: "tank",
            force: true,
            read_write: true,
            rewind: true,
        };
        assert!(import_pool(&system, &state, import));
        let rewind_all = Import {
            rewind: true,
            ..Import::default()
        };
        assert!(!import_pool(&system, &state, rewind_all));
        assert_eq!(
            fake.runner.lines(),
            [
                "zpool import -N -o readonly=on -a",
                "zpool import -N -f -o readonly=off --rewind-to-checkpoint tank",
            ]
        );
        assert!(fake.runner.calls()[0].merges_stderr());
        assert_eq!(
            stub_lines(&fake.kmsg()),
            "DEBUG: attempting to import all pools\n\
             DEBUG: read_write unset\n\
             DEBUG: zpool import arguments: -N -o readonly=on -a\n\
             DEBUG: successful pool import\n\
             DEBUG: pool set to tank\n\
             DEBUG: import_policy set: force\n\
             DEBUG: read_write set: yes\n\
             DEBUG: rewind_to_checkpoint set: yes\n\
             DEBUG: zpool import arguments: -N -f -o readonly=off --rewind-to-checkpoint tank\n\
             DEBUG: successful pool import\n\
             ERROR: rewind only works on a specific pool\n"
        );
    }

    #[test]
    fn failed_imports_log_the_apparent_host_id() {
        let fake = FakeSystem::new();
        let _log = log(&fake);
        fake.files.file(ETC_HOSTID, [0x0c, 0xb1, 0xba, 0x00]);
        fake.runner.respond(
            "zpool import -N -o readonly=on tank",
            1,
            "cannot import\n\n",
        );
        let state = State::new(&fake.files, "/zfsbootmenu");
        let import = Import {
            pool: "tank",
            ..Import::default()
        };
        assert!(!import_pool(&fake.system(), &state, import));
        let lines = stub_lines(&fake.kmsg());
        assert!(lines.ends_with(
            "DEBUG: hostid from /etc/hostid: 00bab10c\n\
             DEBUG: import process failed with code 1, apparent hostid 0x00bab10c\n\
             DEBUG: cannot import\n"
        ));
    }

    #[test]
    fn host_ids_are_scraped_as_the_legacy_pattern_matches() {
        let text = "Last accessed by host (hostid=deadbeef1) at ...";
        assert_eq!(scrape_hostid(text), Some("deadbeef"));
        assert_eq!(scrape_hostid("hostid=x hostid=A1"), Some("A1"));
        assert_eq!(scrape_hostid("hostid="), None);
        assert_eq!(scrape_hostid("myhostid=0b"), Some("0b"));
        // Found by the fuzz target program_output: text that is not ASCII
        // before the match.
        assert_eq!(scrape_hostid("tan\u{fffd}\tDEGRADED"), None);
        assert_eq!(scrape_hostid("pool «tank» hostid=0c"), Some("0c"));
        let mut parity = parity!("host_ids_are_scraped_as_the_legacy_pattern_matches");
        for text in [
            "hostid=12345678",
            "(hostid=a) x",
            "hostid=hostid=7",
            "hostid=g1",
        ] {
            let legacy: String = parity.legacy(text, || {
                Bash::new()
                    .code(concat!(
                        r#"m() { [[ $1 =~ hostid=([A-Fa-f0-9]{1,8}) ]] "#,
                        r#"&& echo "${BASH_REMATCH[1]}"; }"#
                    ))
                    .call("m", &[text], b"")
                    .stdout_text()
            });
            let expected = command_substitution(&legacy).to_owned();
            assert_eq!(scrape_hostid(text).unwrap_or_default(), expected, "{text}");
        }
    }

    #[test]
    fn online_pools_leave_out_the_preferred_one() {
        let output = "   pool: tank\n     id: 1\n  state: ONLINE\n config:\n\n\
                      \ttank  ONLINE\n\n   pool: dead\n     id: 2\n  state: UNAVAIL\n\
                      \n   pool: boot\n     id: 3\n  state: ONLINE\n";
        assert_eq!(online_pools(output, ""), ["tank", "boot"]);
        assert_eq!(online_pools(output, "boot"), ["tank"]);
        assert_eq!(tab_fields("\ttank\t\tDEGRADED\t"), ("tank", "DEGRADED"));
        assert_eq!(tab_fields(" tank "), (" tank ", ""));
    }

    /// A fake `zpool` (and `zfs`) that keeps its pools in a directory, so the
    /// legacy stage and the Rust stage run against the same machine. Each pool
    /// is a file of `key=value` lines: `id`, `hostid` (of the host that last
    /// used it), `state` and `health` (`ONLINE` if not given), `bootfs`,
    /// `unsupported` (a feature) and `appear` (the number of import attempts
    /// after which it shows up).
    const FAKE_ZPOOL: &str = r#"#!/bin/sh
Z="$(cd "$(dirname "$0")/.." && pwd)"
printf '%s %s\n' "$(basename "$0")" "$*" >> "$Z/calls"
prop() { sed -n "s/^$2=//p" "$Z/pools/$1"; }
attempts() { cat "$Z/attempts" 2>/dev/null || echo 0; }
visible() { a="$(prop "$1" appear)"; [ "$(attempts)" -ge "${a:-0}" ]; }
imported() { grep -qxF "$1" "$Z/imported"; }
current() { od -An -tx4 -N4 "$Z/etc_hostid" 2>/dev/null | tr -d ' \n'; }
if [ "$(basename "$0")" = zfs ]; then
  for last; do :; done
  if grep -qxF "$last" "$Z/datasets"; then echo "$last"; exit 0; fi
  echo "cannot open '$last': dataset does not exist" >&2; exit 1
fi
cmd="$1"; shift
case "$cmd" in
  import)
    if [ $# -eq 0 ]; then
      for p in $(ls "$Z/pools"); do
        imported "$p" && continue
        visible "$p" || continue
        s="$(prop "$p" state)"
        printf '   pool: %s\n     id: %s\n  state: %s\n action: import\n config:\n\n\t%s  %s\n\n' \
          "$p" "$(prop "$p" id)" "${s:-ONLINE}" "$p" "${s:-ONLINE}"
      done
      exit 0
    fi
    force=; target=
    while [ $# -gt 0 ]; do
      case "$1" in -f) force=1 ;; -o) shift ;; -N|--rewind-to-checkpoint) ;; *) target="$1" ;; esac
      shift
    done
    echo $(( $(attempts) + 1 )) > "$Z/attempts"
    status=0; found=
    for p in $(ls "$Z/pools"); do
      [ "$target" = -a ] || [ "$target" = "$p" ] || [ "$target" = "$(prop "$p" id)" ] || continue
      imported "$p" && continue
      visible "$p" || continue
      found=1
      if [ "$(prop "$p" state)" = UNAVAIL ]; then
        echo "cannot import '$p': one or more devices is currently unavailable" >&2
        status=1; continue
      fi
      h="$(prop "$p" hostid)"
      if [ -n "$h" ] && [ "$h" != "$(current)" ] && [ -z "$force" ]; then
        echo "cannot import '$p': pool was previously in use from another system." >&2
        echo "Last accessed by host (hostid=$h) at Thu Jan  1 00:00:00 1970" >&2
        echo "The pool can be imported, use 'zpool import -f' to import the pool." >&2
        status=1; continue
      fi
      echo "$p" >> "$Z/imported"
    done
    if [ -z "$found" ]; then
      if [ "$target" = -a ]; then
        echo "no pools available to import" >&2
      else
        echo "cannot import '$target': no such pool available" >&2; status=1
      fi
    fi
    exit $status
    ;;
  list)
    fields=; target=
    while [ $# -gt 0 ]; do
      case "$1" in -H|--) ;; -o) shift; fields="$1" ;; *) target="$1" ;; esac
      shift
    done
    if [ -n "$target" ] && ! imported "$target"; then
      echo "cannot open '$target': no such pool" >&2; exit 1
    fi
    while read -r p; do
      [ -n "$target" ] && [ "$target" != "$p" ] && continue
      case "$fields" in
        name) echo "$p" ;;
        name,health) h="$(prop "$p" health)"; printf '%s\t%s\n' "$p" "${h:-ONLINE}" ;;
        bootfs) b="$(prop "$p" bootfs)"; echo "${b:--}" ;;
      esac
    done < "$Z/imported"
    ;;
  get)
    while read -r p; do
      printf '%s\tsize\n%s\tfeature@async_destroy\n' "$p" "$p"
      u="$(prop "$p" unsupported)"
      [ -n "$u" ] && printf '%s\tunsupported@%s\n' "$p" "$u"
    done < "$Z/imported"
    ;;
esac
exit 0
"#;

    /// A machine for the import stage, and what the user does.
    #[derive(Clone, Debug, Default)]
    struct Scenario {
        /// Pools and their `key=value` lines.
        pools: Vec<(&'static str, &'static str)>,
        /// Filesystems `zfs list` knows.
        datasets: &'static str,
        /// The contents of `/etc/hostid`, if it exists.
        hostid: Option<[u8; 4]>,
        /// The settings of the stage.
        settings: Settings,
        /// The answers to prompts, in order: `true` for ESCAPE.
        escapes: Vec<bool>,
        /// What the recovery shell does (shell code run in the machine's
        /// directory).
        on_shell: &'static str,
    }

    /// What a run of the stage left behind.
    #[derive(Debug, Deserialize, PartialEq, Eq, Serialize)]
    struct Outcome {
        log: String,
        calls: String,
        imported: String,
        degraded: Option<String>,
        bootfs_file: Option<String>,
        spl_hostid_file: Option<String>,
        etc_hostid: Option<Vec<u8>>,
        shells: String,
        bootfs: String,
        prompts: usize,
    }

    impl Scenario {
        fn machine(&self) -> TempDir {
            let dir = TempDir::new("zbm-import");
            for (name, lines) in &self.pools {
                dir.write(format!("pools/{name}"), format!("{lines}\n"));
            }
            dir.write("imported", "");
            dir.write("datasets", format!("{}\n", self.datasets));
            dir.write("on_shell", self.on_shell);
            if let Some(hostid) = self.hostid {
                dir.write("etc_hostid", hostid);
            }
            dir.write_executable("bin/zpool", FAKE_ZPOOL);
            dir.write_executable("bin/zfs", FAKE_ZPOOL);
            std::fs::create_dir(dir.join("base")).unwrap();
            dir
        }

        fn outcome(dir: &TempDir, log: String, bootfs: String, prompts: usize) -> Outcome {
            let read = |name: &str| std::fs::read_to_string(dir.join(name)).ok();
            Outcome {
                log,
                calls: read("calls").unwrap_or_default(),
                imported: read("imported").unwrap_or_default(),
                degraded: read("base/degraded"),
                bootfs_file: read("base/bootfs"),
                spl_hostid_file: read("base/spl_hostid"),
                etc_hostid: std::fs::read(dir.join("etc_hostid")).ok(),
                shells: read("shells").unwrap_or_default(),
                bootfs,
                prompts,
            }
        }

        /// Runs `init.d/50-import-pools` with the legacy functions.
        fn run_legacy(&self) -> Outcome {
            let dir = self.machine();
            let z = dir.path().to_string_lossy().into_owned();
            let core =
                std::fs::read_to_string(path("zfsbootmenu/lib/zfsbootmenu-core.sh")).unwrap();
            let function = |name: &str| {
                let start = core.find(&format!("\n{name}() {{\n")).unwrap() + 1;
                let end = start + core[start..].find("\n}\n").unwrap() + 3;
                core[start..end]
                    .replace(SPL_HOSTID, &format!("{z}/spl_hostid"))
                    .replace("/etc/hostid", &format!("{z}/etc_hostid"))
            };
            let stage = std::fs::read_to_string(path("zfsbootmenu/init.d/50-import-pools"))
                .unwrap()
                .replace(
                    "/lib/zfsbootmenu-luks.sh",
                    "/nonexistent/zfsbootmenu-luks.sh",
                );
            dir.write("stage.sh", stage);
            let answers: String = self
                .escapes
                .iter()
                .map(|escape| if *escape { "escape\n" } else { "return\n" })
                .collect();
            dir.write("answers", answers);

            let settings = &self.settings;
            let output = Bash::new()
                .code(LINE_LOG_STUBS)
                .code(
                    r#"source() {
                        case "$1" in
                          /lib/*) builtin source "$LEGACY/zfsbootmenu/lib/${1#/lib/}" ;;
                          *) builtin source "$@" ;;
                        esac
                      }"#,
                )
                .source("zfsbootmenu/lib/zfsbootmenu-core.sh")
                .code(&function("write_hostid"))
                .code(&function("get_spl_hostid"))
                .code(
                    r#"
                    timed_prompt() {
                      echo x >> "$Z/prompts"
                      [ "$(wc -l < "$Z/prompts")" -le 10 ] || exit 3
                      local answer
                      answer="$(head -n 1 "$Z/answers")"
                      sed -i 1d "$Z/answers"
                      [ "${answer}" != escape ]
                    }
                    emergency_shell() {
                      echo "$1" >> "$Z/shells"
                      [ "$(wc -l < "$Z/shells")" -le 3 ] || exit 4
                      (cd "$Z" && sh ./on_shell)
                    }
                    zreport() { echo REPORT; }
                    run() {
                      ZFSBOOTMENU_INITIALIZATION=yes
                      source "$Z/stage.sh"
                      printf '%s' "${BOOTFS}" > "$Z/BOOTFS"
                    }"#,
                )
                .env("Z", &z)
                .env("BASE", &format!("{z}/base"))
                .env(
                    "PATH",
                    &format!("{z}/bin:{}", std::env::var("PATH").unwrap()),
                )
                .env("import_policy", settings.import_policy.as_str())
                .env("zbm_prefer_pool", &settings.prefer_pool)
                .env("zbm_require_pool", &settings.require_pool)
                .env("zbm_prefer_bootfs", &settings.prefer_bootfs)
                .env("zbm_retry_delay", &settings.retry_delay)
                .env(
                    "ZBM_RELEASE_BUILD",
                    if settings.release_build { "yes" } else { "" },
                )
                .call("run", &[], b"");
            assert!(output.status.success(), "{}", output.stderr_text());
            // zreport is not ported yet; it only adds debugging output. The
            // messages name the moved files by their real paths.
            let log = log_lines(&output.stderr_text())
                .replace("DEBUG: REPORT\n", "")
                .replace(&format!("{z}/etc_hostid"), "/etc/hostid");
            let bootfs = std::fs::read_to_string(dir.join("BOOTFS")).unwrap();
            let prompts = std::fs::read_to_string(dir.join("prompts"))
                .unwrap_or_default()
                .lines()
                .count();
            Scenario::outcome(&dir, log, bootfs, prompts)
        }

        /// Runs the Rust stage.
        fn rust(&self) -> Outcome {
            let dir = self.machine();
            let fake = FakeSystem::new();
            let bin = dir.join("bin");
            let prompts = std::rc::Rc::new(Cell::new(0));
            let counter = prompts.clone();
            fake.runner.on(move |command| {
                let program = command.program().to_string_lossy().into_owned();
                match program.as_str() {
                    "zpool" | "zfs" => {
                        let mut real = Command::new(bin.join(&program))
                            .args(command.get_args())
                            .stdin(command.get_stdin().clone())
                            .stdout(command.get_stdout());
                        real = if command.merges_stderr() {
                            real.merge_stderr()
                        } else {
                            real.stderr(Stream::Null)
                        };
                        Some(SystemRunner.run(&real))
                    }
                    "tput" if command.get_args() == ["civis"] => {
                        counter.set(counter.get() + 1);
                        assert!(counter.get() <= 10, "too many prompts");
                        None
                    }
                    "tput" if command.get_args() == ["lines"] => {
                        Some(Ok(Output::new(Status::exited(0), "25\n")))
                    }
                    "tput" if command.get_args() == ["cols"] => {
                        Some(Ok(Output::new(Status::exited(0), "80\n")))
                    }
                    _ => None,
                }
            });
            for escape in &self.escapes {
                fake.console.keys(if *escape { b"\x1b" } else { b"\n" });
            }
            let files = Redirect::new(&[
                (ETC_HOSTID, &dir.join("etc_hostid")),
                (SPL_HOSTID, &dir.join("spl_hostid")),
            ]);
            let system = System {
                files: &files,
                ..fake.system()
            };
            let state = State::new(&files, dir.join("base"));
            let kmsg = dir.join("kmsg");
            let _log = KernelLog::new(&files, "7", &dir.join("base"), 1)
                .with_device(&kmsg)
                .install();
            let mut recovery = Shell {
                dir: dir.path().to_path_buf(),
                shells: 0,
            };
            let imported = stage(&system, &state, &self.settings, &mut recovery);
            let log = stub_lines(&std::fs::read_to_string(&kmsg).unwrap_or_default());
            // The marker files of warnings and errors are the Rust log's own.
            let _ = std::fs::remove_file(dir.join("base/have_errors"));
            let _ = std::fs::remove_file(dir.join("base/have_warnings"));
            let bootfs = imported.bootfs.unwrap_or_default();
            Scenario::outcome(&dir, log, bootfs, prompts.get())
        }

        fn check(&self, parity: &mut Parity) -> Outcome {
            let legacy: Outcome = parity.legacy(format!("{self:?}"), || self.run_legacy());
            let rust = self.rust();
            assert_eq!(rust, legacy);
            rust
        }
    }

    /// The recovery shell of the Rust runs: the scenario's shell code.
    struct Shell {
        dir: PathBuf,
        shells: usize,
    }

    impl Recovery for Shell {
        fn unlock_late(&mut self) -> bool {
            false
        }

        fn emergency_shell(&mut self, message: &str) {
            self.shells += 1;
            assert!(self.shells <= 3, "too many recovery shells");
            let shells = self.dir.join("shells");
            let mut text = std::fs::read_to_string(&shells).unwrap_or_default();
            text.push_str(&format!("{message}\n"));
            std::fs::write(&shells, text).unwrap();
            let status = std::process::Command::new("sh")
                .arg("./on_shell")
                .current_dir(&self.dir)
                .status()
                .unwrap();
            assert!(status.success());
        }
    }

    fn hostid_settings() -> Settings {
        Settings {
            import_policy: ImportPolicy::Hostid,
            ..Settings::default()
        }
    }

    #[test]
    fn imports_every_pool_and_finds_bootfs() {
        let mut parity = parity!("imports_every_pool_and_finds_bootfs");
        let outcome = Scenario {
            pools: vec![
                ("boot", "id=1\nbootfs=-"),
                ("tank", "id=2\nbootfs=tank/ROOT/void"),
            ],
            settings: hostid_settings(),
            ..Scenario::default()
        }
        .check(&mut parity);
        assert_eq!(outcome.imported, "boot\ntank\n");
        assert_eq!(outcome.bootfs, "tank/ROOT/void");
        assert_eq!(outcome.bootfs_file.as_deref(), Some("tank/ROOT/void\n"));
        assert_eq!(outcome.degraded, None);
    }

    #[test]
    fn matches_the_host_id_of_a_foreign_pool() {
        let mut parity = parity!("matches_the_host_id_of_a_foreign_pool");
        let outcome = Scenario {
            pools: vec![("tank", "id=7\nhostid=deadbeef\nbootfs=tank/ROOT/a")],
            hostid: Some([0x0c, 0xb1, 0xba, 0x00]),
            settings: hostid_settings(),
            ..Scenario::default()
        }
        .check(&mut parity);
        assert_eq!(outcome.imported, "tank\n");
        assert_eq!(outcome.spl_hostid_file.as_deref(), Some("deadbeef"));
        assert_eq!(outcome.etc_hostid, Some(vec![0xef, 0xbe, 0xad, 0xde]));
        assert!(
            outcome
                .log
                .contains("WARN: imported tank with assumed hostid deadbeef")
        );

        // A release build does not warn; a preferred pool is matched alone.
        let mut settings = hostid_settings();
        settings.release_build = true;
        settings.prefer_pool = "tank".to_owned();
        let outcome = Scenario {
            pools: vec![
                ("tank", "id=7\nhostid=deadbeef\nbootfs=tank/ROOT/a"),
                ("other", "id=8\nhostid=deadbeef"),
            ],
            hostid: Some([0x0c, 0xb1, 0xba, 0x00]),
            settings,
            ..Scenario::default()
        }
        .check(&mut parity);
        assert_eq!(outcome.imported, "tank\nother\n");
        assert!(!outcome.log.contains("WARN:"));

        // A required pool alone: the other pool stays where it is.
        let mut settings = hostid_settings();
        settings.prefer_pool = "tank".to_owned();
        settings.require_pool = "only".to_owned();
        let outcome = Scenario {
            pools: vec![
                ("tank", "id=7\nhostid=deadbeef\nbootfs=tank/ROOT/a"),
                ("other", "id=8"),
            ],
            hostid: Some([0x0c, 0xb1, 0xba, 0x00]),
            settings,
            ..Scenario::default()
        }
        .check(&mut parity);
        assert_eq!(outcome.imported, "tank\n");
    }

    /// Recovery that opens LUKS volumes on the given calls of `unlock_late`.
    struct Late {
        calls: usize,
        opens: Vec<usize>,
    }

    impl Recovery for Late {
        fn unlock_late(&mut self) -> bool {
            self.calls += 1;
            self.opens.contains(&self.calls)
        }

        fn emergency_shell(&mut self, message: &str) {
            panic!("unexpected recovery shell: {message}");
        }
    }

    #[test]
    fn late_luks_volumes_are_unlocked_before_retrying() {
        let fake = FakeSystem::new();
        let _log = log(&fake);
        let lists = Cell::new(0);
        fake.runner.on(move |command| {
            if command.display() != "zpool list -H -o name" {
                return None;
            }
            lists.set(lists.get() + 1);
            let pools = if lists.get() >= 3 { "tank\n" } else { "" };
            Some(Ok(Output::new(Status::exited(0), pools)))
        });
        let settings = Settings {
            import_policy: ImportPolicy::Strict,
            ..Settings::default()
        };
        let state = State::new(&fake.files, "/zfsbootmenu");
        // The first failure finds a volume at once (call 1); the retry finds none
        // before importing (2), nor does the second failure (3), so the prompt
        // comes; the retry after it finds one before importing (4).
        fake.console.keys(b"\n");
        let mut recovery = Late {
            calls: 0,
            opens: vec![1, 4],
        };
        stage(&fake.system(), &state, &settings, &mut recovery);
        assert_eq!(recovery.calls, 4);
        let info: Vec<String> = stub_lines(&fake.kmsg())
            .lines()
            .filter(|line| line.starts_with("INFO"))
            .map(str::to_owned)
            .collect();
        assert_eq!(
            info,
            [
                "INFO: unable to import a pool on attempt 1",
                "INFO: unlocked LUKS volumes that appeared late, retrying import",
                "INFO: unable to import a pool on attempt 2",
                "INFO: unlocked LUKS volumes that appeared late",
            ]
        );
    }

    #[test]
    fn host_id_cases() {
        let mut parity = parity!("host_id_cases");
        // Two pools of different foreign hosts: the first match decides the host
        // ID, and only pools of that host follow.
        let outcome = Scenario {
            pools: vec![
                ("alpha", "id=1\nhostid=0000abcd\nbootfs=alpha/ROOT/a"),
                ("beta", "id=2\nhostid=deadbeef"),
                ("gamma", "id=3\nhostid=0000abcd"),
            ],
            hostid: Some([0x0c, 0xb1, 0xba, 0x00]),
            settings: hostid_settings(),
            ..Scenario::default()
        }
        .check(&mut parity);
        assert_eq!(outcome.imported, "alpha\ngamma\n");
        assert_eq!(outcome.spl_hostid_file.as_deref(), Some("0000abcd"));

        // The pool's host ID is this machine's: no matching.
        let outcome = Scenario {
            pools: vec![("tank", "id=7\nhostid=00bab10c")],
            hostid: Some([0x0c, 0xb1, 0xba, 0x00]),
            settings: hostid_settings(),
            ..Scenario::default()
        }
        .check(&mut parity);
        assert_eq!(outcome.spl_hostid_file, None);

        // Without /etc/hostid, a pool with a host ID is matched too.
        let outcome = Scenario {
            pools: vec![("tank", "id=7\nhostid=00bab10c")],
            settings: hostid_settings(),
            ..Scenario::default()
        }
        .check(&mut parity);
        assert_eq!(outcome.etc_hostid, Some(vec![0x0c, 0xb1, 0xba, 0x00]));
    }

    #[test]
    fn a_required_pool_that_never_appears() {
        let mut parity = parity!("a_required_pool_that_never_appears");
        for policy in [ImportPolicy::Strict, ImportPolicy::Hostid] {
            let mut settings = hostid_settings();
            settings.import_policy = policy;
            settings.prefer_pool = "missing".to_owned();
            settings.require_pool = "only".to_owned();
            let outcome = Scenario {
                pools: vec![("other", "id=1\nbootfs=other/ROOT/a")],
                settings,
                escapes: vec![false, true],
                on_shell: "printf 'id=9\\n' > pools/missing\n",
                ..Scenario::default()
            }
            .check(&mut parity);
            assert_eq!(outcome.prompts, 2, "{policy}");
            assert_eq!(outcome.imported, "missing\n", "{policy}");
        }
    }

    #[test]
    fn strict_and_forced_policies() {
        let mut parity = parity!("strict_and_forced_policies");
        let mut settings = hostid_settings();
        settings.import_policy = ImportPolicy::Force;
        let outcome = Scenario {
            pools: vec![("tank", "id=7\nhostid=deadbeef")],
            settings,
            ..Scenario::default()
        }
        .check(&mut parity);
        assert_eq!(outcome.imported, "tank\n");
        assert_eq!(outcome.spl_hostid_file, None);

        // Strict: no host ID matching; ESCAPE opens the recovery shell, which
        // fixes the pool.
        let mut settings = hostid_settings();
        settings.import_policy = ImportPolicy::Strict;
        let outcome = Scenario {
            pools: vec![("tank", "id=7\nhostid=deadbeef")],
            hostid: Some([0x0c, 0xb1, 0xba, 0x00]),
            settings,
            escapes: vec![false, true],
            on_shell: "sed -i '/^hostid=/d' pools/tank\n",
            ..Scenario::default()
        }
        .check(&mut parity);
        assert_eq!(outcome.prompts, 2);
        assert_eq!(outcome.shells, "unable to successfully import a pool\n");
        assert_eq!(outcome.imported, "tank\n");
    }

    #[test]
    fn retries_until_late_pools_appear() {
        let mut parity = parity!("retries_until_late_pools_appear");
        let outcome = Scenario {
            pools: vec![("late", "id=9\nappear=3\nbootfs=late/ROOT/x")],
            settings: hostid_settings(),
            ..Scenario::default()
        }
        .check(&mut parity);
        assert_eq!(outcome.imported, "late\n");
        assert!(outcome.prompts >= 1);
        assert_eq!(outcome.bootfs, "late/ROOT/x");
    }

    #[test]
    fn preferred_and_required_pools() {
        let mut parity = parity!("preferred_and_required_pools");
        for require in ["", "yes", "only"] {
            let mut settings = hostid_settings();
            settings.prefer_pool = "tank".to_owned();
            settings.require_pool = require.to_owned();
            settings.retry_delay = "2".to_owned();
            Scenario {
                pools: vec![
                    ("boot", "id=1\nbootfs=boot/ROOT/b"),
                    ("tank", "id=2\nappear=2\nbootfs=-"),
                ],
                settings,
                ..Scenario::default()
            }
            .check(&mut parity);
        }
        // A missing preferred pool that is not required.
        let mut settings = hostid_settings();
        settings.prefer_pool = "missing".to_owned();
        Scenario {
            pools: vec![("boot", "id=1\nbootfs=boot/ROOT/b")],
            settings,
            ..Scenario::default()
        }
        .check(&mut parity);
    }

    #[test]
    fn restricts_unhealthy_and_unsupported_pools() {
        let mut parity = parity!("restricts_unhealthy_and_unsupported_pools");
        for release_build in [false, true] {
            let mut settings = hostid_settings();
            settings.release_build = release_build;
            let outcome = Scenario {
                pools: vec![
                    ("data", "id=1\nhealth=DEGRADED"),
                    ("data2", "id=2\nunsupported=com.example:new"),
                    ("tank", "id=3\nhealth=DEGRADED\nunsupported=org.x:y"),
                ],
                settings,
                ..Scenario::default()
            }
            .check(&mut parity);
            assert_eq!(outcome.degraded.as_deref(), Some("data\ntank\ndata2\n"));
            assert_eq!(outcome.prompts, 1);
        }
    }

    #[test]
    fn unavailable_pools_are_logged_before_the_recovery_shell() {
        let mut parity = parity!("unavailable_pools_are_logged_before_the_recovery_shell");
        let outcome = Scenario {
            pools: vec![("broken", "id=42\nstate=UNAVAIL")],
            settings: hostid_settings(),
            escapes: vec![true],
            on_shell: "printf 'id=43\\nbootfs=-\\n' > pools/fixed\n",
            ..Scenario::default()
        }
        .check(&mut parity);
        assert!(outcome.log.contains(
            "ERROR: cannot import 'broken': one or more devices is currently unavailable"
        ));
        assert_eq!(outcome.imported, "fixed\n");
        assert_eq!(outcome.bootfs, "");
    }

    #[test]
    fn a_preferred_boot_environment_must_be_a_filesystem() {
        let mut parity = parity!("a_preferred_boot_environment_must_be_a_filesystem");
        for (datasets, prefer) in [("tank/ROOT/a", "tank/ROOT/a"), ("", "tank/ROOT/none")] {
            let mut settings = hostid_settings();
            settings.prefer_pool = "tank".to_owned();
            settings.prefer_bootfs = prefer.to_owned();
            Scenario {
                pools: vec![("tank", "id=1\nbootfs=tank/ROOT/b")],
                datasets,
                settings,
                ..Scenario::default()
            }
            .check(&mut parity);
        }
    }
}
