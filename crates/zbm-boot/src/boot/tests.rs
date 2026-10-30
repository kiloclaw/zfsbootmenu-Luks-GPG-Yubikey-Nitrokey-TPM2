//! Tests of booting: kernel discovery and the boot sequence against the
//! legacy runtime on a fake machine, and the mount and pool helpers.

use super::*;
use std::path::{Path, PathBuf};
use zbm_platform::exec::{Command, Stream};
use zbm_platform::{Installed, KernelLog, Runner, System};
use zbm_state::State;

use serde::{Deserialize, Serialize};
use zbm_platform::exec::{Output, Status, SystemRunner};
use zbm_testkit::fakes::FakeSystem;
use zbm_testkit::legacy::{Bash, log_lines, path};
use zbm_testkit::parity::Parity;
use zbm_testkit::{TempDir, parity};

use crate::commandline::{ETC_HOSTID, SPL_HOSTID};
use crate::error::BootError;
use crate::kernels::Kernel;
use crate::recovery::Recovery;
use crate::testing::{LINE_LOG_STUBS, Redirect, name, stub_lines};

/// A fake for every program the boot runs: it records each call and
/// answers from `answers`, whose lines are `status<TAB>glob<TAB>output`
/// (`printf %b` escapes); the first glob that matches the command line
/// wins, and other commands succeed silently.
const FAKE: &str = r#"#!/bin/sh
Z="$(cd "$(dirname "$0")/.." && pwd)"
line="$(basename "$0") $*"
printf '%s\n' "$line" >> "$Z/calls"
tab="$(printf '\t')"
while IFS="$tab" read -r status glob output; do
  case "$line" in
    $glob) printf '%b' "$output"; exit "$status" ;;
  esac
done < "$Z/answers"
exit 0
"#;

/// A hook that records its stage and the variables it gets.
const HOOK: &str = r#"#!/bin/sh
Z="$(cd "$(dirname "$0")/../.." && pwd)"
printf 'hook %s %s|%s|%s|%s\n' "$(basename "$(dirname "$0")")" "${ZBM_SELECTED_BE}" \
  "${ZBM_SELECTED_KERNEL}" "${ZBM_SELECTED_INITRAMFS}" "${ZBM_SELECTED_MOUNTPOINT}" \
  >> "$Z/calls"
"#;

/// A `load-key.d` hook that records the variables it gets.
const KEY_HOOK: &str = r#"#!/bin/sh
Z="$(cd "$(dirname "$0")/../.." && pwd)"
printf 'hook load-key.d %s|%s\n' "${ZBM_LOCKED_FS}" "${ZBM_ENCRYPTION_ROOT}" >> "$Z/calls"
"#;

const PROGRAMS: [&str; 6] = ["zfs", "zpool", "mount", "umount", "kexec", "mountpoint"];

/// A machine with one boot environment.
#[derive(Clone, Debug)]
struct Machine {
    /// Answers of the fake programs: status, glob, output.
    answers: Vec<(u8, &'static str, &'static str)>,
    /// Files of the boot environment, relative to its root.
    files: Vec<(&'static str, &'static str)>,
    /// Whether hooks are installed.
    hooks: bool,
}

const FS: &str = "tank/ROOT/void";

/// What a run left behind, with the machine's directory written as `$Z`.
#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
struct Outcome {
    log: String,
    calls: String,
    kernels: Option<String>,
    kcl: Vec<(String, String)>,
    shells: String,
    prompts: usize,
    result: String,
}

impl Machine {
    fn dir(&self) -> TempDir {
        let dir = TempDir::new("zbm-boot");
        let answers = self
            .answers
            .iter()
            .map(|(status, glob, output)| format!("{status}\t{glob}\t{output}\n"))
            .collect::<Vec<_>>()
            .concat();
        dir.write("answers", answers);
        for program in PROGRAMS {
            dir.write_executable(format!("bin/{program}"), FAKE);
        }
        let mnt = format!("base/environments/{FS}/mnt");
        std::fs::create_dir_all(dir.join(&mnt)).unwrap();
        for (name, contents) in &self.files {
            dir.write(format!("{mnt}/{name}"), contents);
        }
        if self.hooks {
            dir.write_executable("hooks/boot-sel.d/10-record", HOOK);
            dir.write_executable("hooks/teardown.d/10-record", HOOK);
            dir.write_executable("hooks/load-key.d/10-record", KEY_HOOK);
        }
        dir
    }

    fn outcome(dir: &TempDir, log: &str, shells: &str, prompts: usize, result: &str) -> Outcome {
        let z = dir.path().to_string_lossy().into_owned();
        let read = |name: &str| std::fs::read_to_string(dir.join(name)).ok();
        let kcl_dir = dir.join(format!("base/environments/{FS}/kcl"));
        let mut kcl: Vec<(String, String)> = std::fs::read_dir(&kcl_dir)
            .map(|entries| {
                entries
                    .map(|entry| {
                        let entry = entry.unwrap();
                        let name = entry.file_name().to_string_lossy().into_owned();
                        (name, std::fs::read_to_string(entry.path()).unwrap())
                    })
                    .collect()
            })
            .unwrap_or_default();
        kcl.sort();
        Outcome {
            log: log.replace(&z, "$Z"),
            calls: read("calls").unwrap_or_default().replace(&z, "$Z"),
            kernels: read(&format!("base/environments/{FS}/kernels")),
            kcl,
            shells: shells.replace(&z, "$Z"),
            prompts,
            result: result.replace(&z, "$Z"),
        }
    }

    /// Runs `call` (shell code using the legacy functions) and returns what
    /// it printed on standard output as the result.
    fn run_legacy(&self, call: &str) -> Outcome {
        let dir = self.dir();
        let z = dir.path().to_string_lossy().into_owned();
        dir.write("stubs.sh", LINE_LOG_STUBS);
        dir.write("empty.conf", "");
        let run_hooks = std::fs::read_to_string(path("zfsbootmenu/libexec/zfsbootmenu-run-hooks"))
            .unwrap()
            .replace("/lib/profiling-lib.sh", &format!("{z}/stubs.sh"))
            .replace("/lib/kmsg-log-lib.sh", &format!("{z}/stubs.sh"))
            .replace("/etc/zfsbootmenu.conf", &format!("{z}/empty.conf"))
            .replace("/libexec/hooks", &format!("{z}/hooks"));
        dir.write_executable("run-hooks", run_hooks);
        let core = std::fs::read_to_string(path("zfsbootmenu/lib/zfsbootmenu-core.sh")).unwrap();
        let function = |name: &str| {
            let start = core.find(&format!("\n{name}() {{\n")).unwrap() + 1;
            let end = start + core[start..].find("\n}\n").unwrap() + 3;
            core[start..end]
                .replace(SPL_HOSTID, &format!("{z}/spl_hostid"))
                .replace("/etc/hostid", &format!("{z}/etc_hostid"))
                .replace("/libexec/zfsbootmenu-run-hooks", &format!("{z}/run-hooks"))
        };
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
            .code(&function("get_spl_hostid"))
            .code(&function("kexec_kernel"))
            .code(&function("load_key"))
            .code(
                r#"
                    tput() { :; }
                    timed_prompt() { echo x >> "$Z/prompts"; }
                    emergency_shell() { echo "$1" >> "$Z/shells"; }
                    run() { eval "$1"; }"#,
            )
            .env("Z", &z)
            .env("BASE", &format!("{z}/base"))
            .env(
                "PATH",
                &format!("{z}/bin:{}", std::env::var("PATH").unwrap()),
            )
            .env("zbm_set_hostid", "1")
            .env("loglevel", "7")
            .env("ZBM_LOG", &format!("{z}/log"))
            .call("run", &[call], b"");
        let log = log_lines(&std::fs::read_to_string(dir.join("log")).unwrap_or_default());
        let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap_or_default();
        let prompts = read("prompts").lines().count();
        Machine::outcome(&dir, &log, &read("shells"), prompts, &output.stdout_text())
    }

    /// Runs `call` with the Rust code; it returns the result.
    fn rust<F>(&self, call: F) -> Outcome
    where
        F: FnOnce(&System<'_>, &State<'_>, &mut Shells, &Path) -> String,
    {
        let dir = self.dir();
        let fake = FakeSystem::new();
        let bin = dir.join("bin");
        fake.runner.on(move |command| {
            let program = command.program().to_string_lossy().into_owned();
            let real = |program: PathBuf| {
                let mut real = Command::new(program)
                    .args(command.get_args())
                    .stdin(command.get_stdin().clone())
                    .stdout(command.get_stdout());
                for (key, value) in command.get_env() {
                    if let Some(value) = value {
                        real = real.env(key, value);
                    }
                }
                real = if command.merges_stderr() {
                    real.merge_stderr()
                } else {
                    real.stderr(Stream::Null)
                };
                Some(SystemRunner.run(&real))
            };
            if PROGRAMS.contains(&program.as_str()) {
                return real(bin.join(&program));
            }
            if program == "bash" || program.contains("/hooks/") {
                return real(PathBuf::from(program));
            }
            (program == "tput").then(|| Ok(Output::new(Status::exited(0), "")))
        });
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
        let mut shells = Shells::default();
        let result = call(&system, &state, &mut shells, &dir.join("hooks"));
        let log = stub_lines(&std::fs::read_to_string(&kmsg).unwrap_or_default());
        let prompts = fake
            .console
            .screen_count(zbm_platform::console::Screen::HideCursor);
        let shells = shells.messages.lock().unwrap().concat();
        Machine::outcome(&dir, &log, &shells, prompts, &result)
    }
}

/// Records the recovery shells the Rust code opens.
#[derive(Default)]
struct Shells {
    messages: std::sync::Mutex<Vec<String>>,
}

impl Recovery for Shells {
    fn unlock_late(&self) -> Result<(), crate::error::BootError> {
        Err(crate::error::BootError::NoCryptsetup)
    }

    fn emergency_shell(&self, message: &str) {
        self.messages.lock().unwrap().push(format!("{message}\n"));
    }
}

/// The debugging messages of `find_be_initramfs`, which the Rust search
/// does not repeat.
fn without_initramfs_search(log: &str) -> String {
    log.lines()
        .filter(|line| {
            ![
                "DEBUG: kernel path: ",
                "DEBUG: kernel version: ",
                "DEBUG: short-matching ",
                "DEBUG: matching '",
            ]
            .iter()
            .any(|prefix| line.starts_with(prefix))
        })
        .flat_map(|line| [line, "\n"])
        .collect()
}

fn kernels_machine() -> Machine {
    Machine {
        answers: vec![(0, "zfs get -H -o value org.zfsbootmenu:kernel *", "-\\n")],
        files: vec![
            ("boot/vmlinuz-6.1.10", ""),
            ("boot/initramfs-6.1.10.img", ""),
            ("boot/vmlinuz-6.1.9", ""),
            ("boot/initrd.img-6.1.9", ""),
            ("boot/vmlinuz-6.1.9.kcl", "quiet\n"),
            ("boot/vmlinuz-5.0", ""),
            ("boot/vmlinuz-5.0.kcl", "single\n"),
            ("boot/config-6.1.9", ""),
        ],
        hooks: false,
    }
}

#[test]
fn kernels_are_found_as_the_legacy_function_finds_them() {
    let mut machines = vec![kernels_machine()];
    let mut chosen = kernels_machine();
    chosen.answers = vec![(
        0,
        "zfs get -H -o value org.zfsbootmenu:kernel *",
        "6\\.1\\.9\\n",
    )];
    machines.push(chosen);
    let mut none = kernels_machine();
    none.files = vec![("boot/vmlinuz-1", "")];
    machines.push(none);
    let mut unmountable = kernels_machine();
    unmountable.answers.push((32, "mount *", "mount failed\\n"));
    machines.push(unmountable);

    let mut parity = parity!("kernels_are_found_as_the_legacy_function_finds_them");
    for machine in machines {
        let legacy: Outcome = parity.legacy(format!("{machine:?}"), || {
            machine.run_legacy(&format!("find_be_kernels {FS} && select_kernel {FS}"))
        });
        let rust = machine.rust(|system, state, _, _| {
            let fs = name(FS);
            if find_kernels(system, state, &fs).is_err() {
                return String::new();
            }
            select_kernel(system, state, &fs)
                .map(|kernel| kernel.line())
                .unwrap_or_default()
        });
        let legacy = Outcome {
            log: without_initramfs_search(&legacy.log),
            ..legacy
        };
        assert_eq!(rust, legacy);
    }
}

fn boot_machine() -> Machine {
    Machine {
        answers: vec![
            (0, "zpool list -H -o name", "tank\\nboot\\n"),
            (0, "zpool get -H -o value readonly tank", "off\\n"),
            (0, "zpool get -H -o value readonly boot", "on\\n"),
            (
                0,
                "zfs get -H -o value org.zfsbootmenu:commandline *",
                "quiet loglevel=4\\n",
            ),
            (
                0,
                "zfs get -H -o value org.zfsbootmenu:rootprefix *",
                "-\\n",
            ),
            (
                0,
                "zfs get -H -o value org.zfsbootmenu:devicetree *",
                "-\\n",
            ),
            (0, "zfs get -H -o value -s inherited *", ""),
            (1, "kexec -e -i", "kexec: no kernel loaded\\n"),
        ],
        files: vec![("etc/os-release", "ID=void\n")],
        hooks: true,
    }
}

fn boot(machine: &Machine, parity: &mut Parity) {
    let _secrets = crate::testing::booting();
    let selected = Kernel {
        dataset: FS.to_owned(),
        kernel: "/boot/vmlinuz-6.1.10".to_owned(),
        initramfs: "/boot/initramfs-6.1.10.img".to_owned(),
    };
    let legacy: Outcome = parity.legacy(format!("{machine:?}"), || {
        machine.run_legacy(&format!(
            "kexec_kernel \"$(printf '%s\\t%s\\t%s' {FS} /boot/vmlinuz-6.1.10 \
                 /boot/initramfs-6.1.10.img)\" >/dev/null"
        ))
    });
    let rust = machine.rust(|system, state, shells, hooks| {
        let settings = BootSettings {
            set_hostid: true,
            hooks: hooks.to_path_buf(),
            env: Vec::new(),
            save_logs: None,
            command_line: None,
            import_policy: zbm_types::ImportPolicy::default(),
        };
        kexec_kernel(system, state, &settings, &selected, shells);
        String::new()
    });
    assert_eq!(rust, legacy);
}

#[test]
fn boots_as_the_legacy_function_boots() {
    let mut parity = parity!("boots_as_the_legacy_function_boots");
    let machine = boot_machine();
    boot(&machine, &mut parity);

    // A device tree, and a kernel that cannot be loaded.
    let mut machine = boot_machine();
    machine.answers.insert(
        0,
        (
            0,
            "zfs get -H -o value org.zfsbootmenu:devicetree *",
            "board.dtb\\n",
        ),
    );
    machine
        .answers
        .insert(0, (1, "kexec -a -l *", "kexec: bad image\\n"));
    machine.files.push(("boot/dtbs/dtbs-6.1.10/board.dtb", ""));
    boot(&machine, &mut parity);

    // A boot environment that cannot be mounted.
    let mut machine = boot_machine();
    machine
        .answers
        .insert(0, (32, "mount *", "mount: failed\\n"));
    boot(&machine, &mut parity);
}

fn fake_log(fake: &FakeSystem) -> Installed {
    fake.files.dir("/zfsbootmenu");
    KernelLog::new(&fake.files, "7", Path::new("/zfsbootmenu"), 1).install()
}

#[test]
fn recursive_unmounts_go_deepest_first() {
    let fake = FakeSystem::new();
    let _log = fake_log(&fake);
    fake.files.file(
        "/proc/self/mounts",
        "tank/a /mnt/be zfs ro 0 0\n\
             tank/a/b /mnt/be/usr zfs ro 0 0\n\
             tank/a/c /mnt/be/usr/local zfs ro 0 0\n\
             tank/d /mnt/other zfs ro 0 0\n\
             tank/e /mnt/bee zfs ro 0 0\n",
    );
    fake.runner.respond("umount -R /mnt/be", 32, "");
    fake.runner.respond("umount /mnt/be/usr", 32, "");
    fake.runner.on(|command| {
        (command.program() == "bash")
            .then(|| Some(SystemRunner.run(command)))
            .flatten()
    });
    let ok = recursive_umount(&fake.system(), Path::new("/mnt/be"));
    assert!(ok.is_err());
    let umounts: Vec<String> = fake
        .runner
        .lines()
        .into_iter()
        .filter(|line| line.starts_with("umount"))
        .collect();
    // `^/mnt/be` is a pattern, so /mnt/bee matches too, as in the legacy code.
    assert_eq!(
        umounts,
        [
            "umount -R /mnt/be",
            "umount /mnt/be/usr/local",
            "umount /mnt/be/usr",
            "umount /mnt/bee",
            "umount /mnt/be",
        ]
    );
    assert!(fake.kmsg().contains("unable to unmount /mnt/be/usr"));
}

#[test]
fn efivarfs_is_mounted_or_remounted() {
    let fake = FakeSystem::new();
    let _log_fake = fake_log(&fake);
    mount_efivarfs(&fake.system(), "ro");
    assert!(fake.runner.lines().is_empty());
    assert!(fake.kmsg().contains("efivarfs unsupported"));

    fake.files.dir("/sys/firmware/efi/efivars");
    mount_efivarfs(&fake.system(), "ro");
    assert_eq!(
        fake.runner.lines(),
        ["mount -t efivarfs efivarfs /sys/firmware/efi/efivars -o ro"]
    );

    // Mounted already, as the kernel's mount table says: remounted.
    let fake2 = FakeSystem::new();
    let _log_fake2 = fake_log(&fake2);
    fake2.files.dir("/sys/firmware/efi/efivars").file(
        "/proc/self/mounts",
        "proc /proc proc rw 0 0\nefivarfs /sys/firmware/efi/efivars efivarfs ro 0 0\n",
    );
    mount_efivarfs(&fake2.system(), "rw");
    assert_eq!(
        fake2.runner.lines(),
        ["mount -t efivarfs efivarfs /sys/firmware/efi/efivars -o remount,rw"]
    );
}

#[test]
fn writable_pools_and_exports() {
    let fake = FakeSystem::new();
    fake.runner
        .respond("zpool get -H -o value readonly tank", 0, "off\n");
    fake.runner
        .respond("zpool get -H -o value readonly gone", 1, "");
    let _log = fake_log(&fake);
    assert!(is_writable(&fake.runner, &name("tank")));
    assert!(!is_writable(&fake.runner, &name("gone")));
    assert_eq!(export_pool(&fake.runner, &name("tank")), Ok(()));
    assert!(fake.kmsg().contains("tank export process return: 0"));
}

#[test]
fn boots_a_locked_environment_as_the_legacy_function_does() {
    let mut parity = parity!("boots_a_locked_environment_as_the_legacy_function_does");
    // The hook leaves it locked, and so does the passphrase prompt: the
    // mount refuses it.
    let locked = |keylocation| Machine {
        answers: vec![
            (0, "zpool list -H -o feature@encryption tank", "active\\n"),
            (0, "zfs get -H -o value encryptionroot *", "tank/ROOT\\n"),
            (0, "zfs get -H -o value keystatus *", "unavailable\\n"),
            (0, "zfs get -H -o value keylocation *", keylocation),
            (0, "zfs get -H -o value keyformat *", "hex\\n"),
            (1, "zfs load-key *", ""),
        ],
        files: vec![],
        hooks: true,
    };
    boot(&locked("prompt\\n"), &mut parity);
    // A key file that is not in the image, and a key that is not a
    // passphrase.
    boot(&locked("file:///zbm-test/missing.key\\n"), &mut parity);
    // A key source, locked with the same missing key: its own cache
    // attempt is not repeated.
    let mut with_source = locked("file:///zbm-test/missing.key\\n");
    with_source.answers.insert(
        0,
        (
            0,
            "zfs get -H -o value org.zfsbootmenu:keysource *",
            "tank/keys\\n",
        ),
    );
    boot(&with_source, &mut parity);
}

#[test]
fn failed_exports_and_missing_kernels_are_errors() {
    let fake = FakeSystem::new();
    let _log = fake_log(&fake);
    fake.files.dir("/zfsbootmenu");
    fake.runner
        .respond("zpool export busy", 1, "pool is busy\n");
    let system = fake.system();
    let state = State::new(&fake.files, "/zfsbootmenu");
    assert_eq!(
        export_pool(system.runner, &name("busy")),
        Err(BootError::Export {
            pool: "busy".to_owned(),
            status: 1
        })
    );
    assert_eq!(encryption_root(system.runner, &name("tank/ROOT")), None);
    assert_eq!(locked_root(system.runner, &name("tank/ROOT")), None);
    assert_eq!(
        select_kernel(&system, &state, &name("tank/ROOT/none")),
        None
    );
    assert!(fake.kmsg().contains("missing or empty"));
    let defaults = BootSettings::default();
    assert!(defaults.set_hostid && defaults.env.is_empty());
}

mod kexec;
mod root_prefix;

#[test]
fn a_program_that_does_not_start_substitutes_its_error() {
    let ran = zfs::substituted(Ok(Output::new(Status::exited(0), "tank\n\n")));
    assert_eq!(ran, (true, "tank".to_owned()));
    let failed = zfs::substituted(Err(std::io::Error::other("no zfs")));
    assert_eq!(failed, (false, "no zfs".to_owned()));
}
