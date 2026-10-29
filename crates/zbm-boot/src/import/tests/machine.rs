//! A machine for the import stage: a fake `zpool` (and `zfs`) that keeps its
//! pools in a directory, so that the legacy stage and the Rust stage run
//! against the same pools, and what each run leaves behind.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use zbm_platform::console::Screen;
use zbm_platform::exec::{Command, Stream, SystemRunner};
use zbm_platform::{KernelLog, Runner, System};
use zbm_pool::{BootfsPreference, Build, ImportSettings, PoolRequirement, RetryDelay};
use zbm_state::State;
use zbm_testkit::TempDir;
use zbm_testkit::fakes::FakeSystem;
use zbm_testkit::legacy::{Bash, log_lines, path};
use zbm_testkit::parity::Parity;
use zbm_types::ImportPolicy;
use zbm_types::PoolName;

use crate::commandline::{ETC_HOSTID, SPL_HOSTID};
use crate::import::stage;
use crate::recovery::Recovery;
use crate::testing::{LINE_LOG_STUBS, Redirect, stub_lines};

/// The settings of a scenario, as the legacy stage reads its variables. Their
/// printed form is part of the input of the recordings, so the name and the
/// fields of this struct stay as they were recorded.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct Settings {
    pub(super) import_policy: ImportPolicy,
    pub(super) prefer_pool: String,
    pub(super) require_pool: String,
    pub(super) prefer_bootfs: String,
    pub(super) retry_delay: String,
    pub(super) release_build: bool,
}

impl Settings {
    /// The settings as the Rust stage takes them.
    fn import(&self) -> ImportSettings {
        ImportSettings {
            policy: self.import_policy,
            prefer_pool: self.prefer_pool.parse::<PoolName>().ok(),
            requirement: PoolRequirement::from_conf(&self.require_pool),
            prefer_bootfs: BootfsPreference::from_conf(&self.prefer_bootfs),
            retry_delay: RetryDelay::or_default(&self.retry_delay),
            build: if self.release_build {
                Build::Release
            } else {
                Build::Local
            },
        }
    }
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
pub(super) struct Scenario {
    /// Pools and their `key=value` lines.
    pub(super) pools: Vec<(&'static str, &'static str)>,
    /// Filesystems `zfs list` knows.
    pub(super) datasets: &'static str,
    /// The contents of `/etc/hostid`, if it exists.
    pub(super) hostid: Option<[u8; 4]>,
    /// The settings of the stage.
    pub(super) settings: Settings,
    /// The answers to prompts, in order: `true` for ESCAPE.
    pub(super) escapes: Vec<bool>,
    /// What the recovery shell does (shell code run in the machine's
    /// directory).
    pub(super) on_shell: &'static str,
}

/// What a run of the stage left behind.
#[derive(Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(super) struct Outcome {
    pub(super) log: String,
    pub(super) calls: String,
    pub(super) imported: String,
    pub(super) degraded: Option<String>,
    pub(super) bootfs_file: Option<String>,
    pub(super) spl_hostid_file: Option<String>,
    pub(super) etc_hostid: Option<Vec<u8>>,
    pub(super) shells: String,
    pub(super) bootfs: String,
    pub(super) prompts: usize,
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
        let core = std::fs::read_to_string(path("zfsbootmenu/lib/zfsbootmenu-core.sh")).unwrap();
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
        assert!(output.status.is_success(), "{}", output.stderr_text());
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
        let recovery = Shell {
            dir: dir.path().to_path_buf(),
            shells: std::sync::atomic::AtomicUsize::new(0),
        };
        let imported = stage(&system, &state, &self.settings.import(), &recovery);
        let log = stub_lines(&std::fs::read_to_string(&kmsg).unwrap_or_default());
        // The marker files of warnings and errors are the Rust log's own.
        let _ = std::fs::remove_file(dir.join("base/have_errors"));
        let _ = std::fs::remove_file(dir.join("base/have_warnings"));
        let bootfs = imported
            .bootfs
            .map(|bootfs| bootfs.to_string())
            .unwrap_or_default();
        let prompts = fake.console.screen_count(Screen::HideCursor);
        Scenario::outcome(&dir, log, bootfs, prompts)
    }

    pub(super) fn check(&self, parity: &mut Parity) -> Outcome {
        let legacy: Outcome = parity.legacy(format!("{self:?}"), || self.run_legacy());
        let rust = self.rust();
        assert_eq!(rust, legacy);
        rust
    }
}

/// The recovery shell of the Rust runs: the scenario's shell code.
struct Shell {
    dir: PathBuf,
    shells: std::sync::atomic::AtomicUsize,
}

impl Recovery for Shell {
    fn unlock_late(&self) -> Result<(), crate::error::BootError> {
        Err(crate::error::BootError::NoCryptsetup)
    }

    fn emergency_shell(&self, message: &str) {
        let shells = self
            .shells
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;
        assert!(shells <= 3, "too many recovery shells");
        let shells = self.dir.join("shells");
        let mut text = std::fs::read_to_string(&shells).unwrap_or_default();
        text.push_str(message);
        text.push('\n');
        std::fs::write(&shells, text).unwrap();
        let status = std::process::Command::new("sh")
            .arg("./on_shell")
            .current_dir(&self.dir)
            .status()
            .unwrap();
        assert!(status.success());
    }
}
