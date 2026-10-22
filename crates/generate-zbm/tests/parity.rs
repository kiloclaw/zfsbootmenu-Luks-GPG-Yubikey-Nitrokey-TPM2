//! Parity tests: `legacy/bin/generate-zbm` and the Rust `generate-zbm` run with
//! the same arguments and configuration in identical sandboxes, with fake
//! `dracut`, `mkinitcpio`, mount tools and hooks and the real `objcopy`,
//! `objdump` and `strings`. They must exit the same way, print the same
//! messages and debug log, run the same commands and leave the same files, byte
//! for byte.

#![allow(
    clippy::disallowed_methods,
    reason = "the parity tests run the legacy programs"
)]

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use zbm_platform::exec::{Command, Output, Runner, Stream, SystemRunner};
use zbm_testkit::parity::Parity;
use zbm_testkit::{TempDir, have_program, legacy, parity};

const STUB: &str = "/usr/lib/systemd/boot/efi/linuxx64.efi.stub";

/// Whether the tools the EFI cases need are available.
fn efi_tools() -> bool {
    Path::new(STUB).is_file() && have_program("objcopy") && have_program("objdump")
}

fn tools() -> bool {
    have_program("perl") && have_program("strings")
}

/// Logs a fake program's arguments, one bracketed word each.
const LOG_ARGS: &str = r#"printf '%s' "${0##*/}" >> "$ZBM_FAKE_LOG"
for arg in "$@"; do printf ' [%s]' "$arg" >> "$ZBM_FAKE_LOG"; done
echo >> "$ZBM_FAKE_LOG"
"#;

/// A fake program that logs its arguments and then runs `body`.
fn logging_script(body: &str) -> String {
    format!("#!/bin/sh\n{LOG_ARGS}{body}")
}

/// dracut writes the image named by its next-to-last argument, for the kernel
/// version in its last.
const FAKE_DRACUT: &str = r#"echo "dracut: building"
echo "dracut: a warning" >&2
if [ -n "$FAKE_DRACUT_FAIL" ]; then echo "dracut: failed"; exit "$FAKE_DRACUT_FAIL"; fi
eval "out=\${$(($# - 1))}"; eval "kver=\${$#}"
printf 'initramfs for %s\n' "$kver" > "$out"
head -c 4096 /dev/zero >> "$out"
"#;

const FAKE_MKINITCPIO: &str = r#"out=; kver=
while [ $# -gt 0 ]; do
  case "$1" in
    --generate) out=$2; shift ;;
    --kernel) kver=$2; shift ;;
  esac
  shift
done
echo "==> Building image"
printf 'cpio for %s\n' "$kver" > "$out"
"#;

const FAKE_MOUNT: &str = r#"if [ -n "$FAKE_MOUNT_FAIL" ]; then
  echo "mount: $1: permission denied"
  exit 32
fi
"#;

const FAKE_UNAME: &str = r#"#!/bin/sh
[ "$1" = -r ] && [ "$FAKE_UNAME" != none ] && echo "${FAKE_UNAME:-6.2.5-test}"
exit 0
"#;

fn fake_programs() -> Vec<(&'static str, String)> {
    vec![
        ("dracut", logging_script(FAKE_DRACUT)),
        ("mkinitcpio", logging_script(FAKE_MKINITCPIO)),
        (
            "mountpoint",
            logging_script("exit \"${FAKE_MOUNTPOINT_STATUS:-0}\"\n"),
        ),
        ("mount", logging_script(FAKE_MOUNT)),
        ("umount", logging_script("")),
        ("uname", FAKE_UNAME.to_owned()),
    ]
}

/// A fake kernel whose printable strings include `strings`.
fn kernel_image(strings: &[&str]) -> Vec<u8> {
    let mut bytes = b"MZ\x00\x01".to_vec();
    for text in strings {
        bytes.extend_from_slice(b"\x00\x00");
        bytes.extend_from_slice(text.as_bytes());
        bytes.extend_from_slice(b"\x00\x00");
    }
    bytes.resize(6000, 0);
    bytes
}

/// Extra setup of a sandbox.
type Setup = fn(&Sandbox);

/// One scenario: a configuration and arguments, with `{root}` standing for the
/// sandbox, and extra setup.
struct Case {
    config: String,
    args: &'static [&'static str],
    env: &'static [(&'static str, &'static str)],
    setup: Setup,
}

impl Case {
    fn new(config: &str, args: &'static [&'static str]) -> Case {
        Case {
            config: config.to_owned(),
            args,
            env: &[],
            setup: |_| {},
        }
    }
}

/// A sandbox for one implementation.
struct Sandbox {
    dir: TempDir,
}

impl Sandbox {
    fn new(case: &Case) -> Sandbox {
        let sandbox = Sandbox {
            dir: TempDir::new("generate-zbm-parity"),
        };
        for dir in [
            "bin",
            "boot",
            "esp",
            "tmp",
            "pre.d",
            "post.d",
            "dracut.conf.d",
        ] {
            std::fs::create_dir_all(sandbox.path().join(dir)).unwrap();
        }
        for (name, script) in fake_programs() {
            sandbox.dir.write_executable(format!("bin/{name}"), script);
        }
        let kernels: &[(&str, &[&str])] = &[
            (
                "boot/vmlinuz-6.1.0-test",
                &["Linux version 6.1.0-test (zbm) #1 SMP"],
            ),
            (
                "boot/vmlinuz-6.2.5-test",
                &[
                    "gcc 12.2.0",
                    "Linux version 6.2.5-test (zbm) #2 SMP",
                    "6.2.5-test",
                ],
            ),
            ("boot/vmlinuz-5.10.0", &["Linux version 5.10.0-other (zbm)"]),
            ("boot/linux-lts", &["6.6.6-lts (zbm)"]),
        ];
        for (name, strings) in kernels {
            sandbox.dir.write(name, kernel_image(strings));
        }
        sandbox.dir.write_executable(
            "pre.d/10-hook",
            "#!/bin/sh\necho \"pre hook output\"\necho pre-hook >> \"$ZBM_FAKE_LOG\"\n",
        );
        sandbox
            .dir
            .write("pre.d/20-not-executable", "#!/bin/sh\nexit 1\n");
        sandbox.dir.write_executable(
            "post.d/10-hook",
            "#!/bin/sh\necho post-hook >> \"$ZBM_FAKE_LOG\"\n",
        );
        let config = case.config.replace("{root}", &sandbox.root());
        sandbox.dir.write("config.yaml", config);
        (case.setup)(&sandbox);
        sandbox
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn root(&self) -> String {
        self.path().display().to_string()
    }

    fn run(&self, program: &[String], case: &Case) -> Output {
        let root = self.root();
        let mut command = Command::new("sh")
            .arg("-c")
            .arg("cd \"$ZBM_PARITY_CWD\" && exec \"$@\"")
            .arg("sh")
            .args(program.iter().cloned())
            .args(case.args.iter().map(|arg| arg.replace("{root}", &root)))
            .stderr(Stream::Capture)
            .env("ZBM_PARITY_CWD", &root)
            .env("PATH", format!("{root}/bin:/usr/bin:/bin"))
            .env("TMPDIR", format!("{root}/tmp"))
            .env("HOME", &root)
            .env("LC_ALL", "C")
            .env("SOURCE_DATE_EPOCH", "0")
            .env("ZBM_FAKE_LOG", format!("{root}/calls.log"))
            .env_remove("POSIXLY_CORRECT");
        for (key, value) in case.env {
            command = command.env(*key, value.replace("{root}", &root));
        }
        SystemRunner
            .run(&command)
            .expect("the generator can be started")
    }

    /// Replaces the sandbox, the program name and random temporary names in text.
    fn normalize(&self, text: &str, program: &str) -> String {
        let mut text = text
            .replace(program, "<program>")
            .replace(&self.root(), "<root>");
        for (marker, length) in [("<root>/tmp/", 10), ("zbm.", 6), ("init.", 6), ("kern.", 6)] {
            let mut result = String::new();
            let mut rest = text.as_str();
            while let Some(found) = rest.find(marker) {
                let after = &rest[found + marker.len()..];
                let name_len = after
                    .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                    .unwrap_or(after.len());
                result.push_str(&rest[..found + marker.len()]);
                if name_len == length {
                    result.push_str("<random>");
                    rest = &after[name_len..];
                } else {
                    rest = after;
                }
            }
            result.push_str(rest);
            text = result;
        }
        for kind in ["HASH(0x", "ARRAY(0x"] {
            while let Some(found) = text.find(kind) {
                let end = text[found..]
                    .find(')')
                    .map_or(text.len(), |end| found + end + 1);
                text.replace_range(found..end, "<ref>");
            }
        }
        text
    }

    /// The files the run left, with sizes, contents and permissions; the call log
    /// is shown in full.
    fn state(&self, program: &str) -> String {
        let mut state = String::new();
        let mut files = Vec::new();
        collect(self.path(), &mut files);
        files.sort();
        for file in files {
            let relative = file
                .strip_prefix(self.path())
                .unwrap()
                .display()
                .to_string();
            if relative.starts_with("bin/") {
                continue;
            }
            let metadata = std::fs::symlink_metadata(&file).unwrap();
            let name = self.normalize(&relative, program);
            if metadata.is_dir() {
                state.push_str(&format!("{name}/\n"));
                continue;
            }
            let bytes = std::fs::read(&file).unwrap();
            // These name the sandbox, so their normalized text is compared instead of their
            // bytes.
            if relative == "calls.log" || relative == "config.yaml" {
                state.push_str(&format!(
                    "{relative}, mode {:o}:\n{}",
                    std::os::unix::fs::PermissionsExt::mode(&metadata.permissions()) & 0o7777,
                    self.normalize(&String::from_utf8_lossy(&bytes), program)
                ));
                continue;
            }
            let mode = std::os::unix::fs::PermissionsExt::mode(&metadata.permissions()) & 0o7777;
            // An EFI bundle holds the host's stub: only what was added to it is
            // compared, so that recordings hold on other machines.
            if let Some(payload) = zbm_testkit::efi::describe(&bytes) {
                state.push_str(&format!("{name}: {payload} mode {mode:o}\n"));
                continue;
            }
            let hash = zbm_testkit::efi::fnv1a(&bytes);
            state.push_str(&format!(
                "{name}: {} bytes, {hash:016x}, mode {mode:o}\n",
                bytes.len()
            ));
        }
        state
    }
}

fn collect(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        files.push(path.clone());
        if std::fs::symlink_metadata(&path).unwrap().is_dir() {
            collect(&path, files);
        }
    }
}

/// The debug log in standard error: `## ` lines and `Data::Dumper` blocks.
/// Perl's warnings and the texts of fatal errors, which name Perl source lines,
/// are left out.
fn debug_log(stderr: &str) -> String {
    let mut log = String::new();
    let mut in_dump = false;
    for line in stderr.lines() {
        if line.starts_with("$VAR1 = {") {
            in_dump = !line.ends_with("};");
            log.push_str(line);
            log.push('\n');
        } else if in_dump {
            log.push_str(line);
            log.push('\n');
            in_dump = line != "};";
        } else if line.starts_with("## ")
            || line.starts_with("Unknown option")
            || line.starts_with("Option ")
        {
            log.push_str(line);
            log.push('\n');
        }
    }
    log
}

/// What a run did, after normalization: exit status, standard output, debug log
/// and files.
#[derive(Debug, Deserialize, Serialize)]
struct Outcome {
    code: i32,
    stdout: String,
    log: String,
    state: String,
    /// Perl's standard error, for the messages of failed comparisons.
    stderr: String,
}

/// Runs a case with the Perl program.
fn run_perl(case: &Case) -> Outcome {
    let script = legacy::path("bin/generate-zbm").display().to_string();
    let perl_box = Sandbox::new(case);
    let output = perl_box.run(&["perl".to_owned(), script.clone()], case);
    // Perl dies over a configuration section that is not a mapping with whatever
    // error number an earlier call left; the Rust port exits with 255.
    let perl_code = output.status.shell_code();
    let type_error = (output.stderr_text().contains("as a HASH ref")
        || output.stderr_text().contains("Not a HASH reference"))
        && !output
            .stdout_text()
            .contains("Unable to parse configuration");
    let code = if type_error && perl_code != 0 {
        255
    } else {
        perl_code
    };
    Outcome {
        code,
        stdout: perl_box.normalize(&output.stdout_text(), &script),
        log: debug_log(&perl_box.normalize(&output.stderr_text(), &script)),
        state: perl_box.state(&script),
        stderr: perl_box.normalize(&output.stderr_text(), &script),
    }
}

/// Runs a case with the Rust program, checks that it does what the Perl
/// program did (recorded in `parity`) and returns that.
fn compare(parity: &mut Parity, case: &Case) -> Outcome {
    let describe = format!(
        "args {:?}, env {:?}, config:\n{}",
        case.args, case.env, case.config
    );
    let legacy: Outcome = parity.legacy(describe.clone(), || run_perl(case));
    let binary = env!("CARGO_BIN_EXE_generate-zbm").to_owned();
    let rust_box = Sandbox::new(case);
    let rust = rust_box.run(std::slice::from_ref(&binary), case);
    let rust_stdout = rust_box.normalize(&rust.stdout_text(), &binary);
    assert_eq!(
        rust.status.shell_code(),
        legacy.code,
        "exit status, {describe}\nlegacy stdout:\n{}\nlegacy stderr:\n{}\n\
         rust stdout:\n{rust_stdout}\nrust stderr:\n{}",
        legacy.stdout,
        legacy.stderr,
        rust.stderr_text()
    );
    assert_eq!(rust_stdout, legacy.stdout, "stdout, {describe}");
    assert_eq!(
        debug_log(&rust_box.normalize(&rust.stderr_text(), &binary)),
        legacy.log,
        "debug log, {describe}\nlegacy stderr:\n{}",
        legacy.stderr
    );
    assert_eq!(rust_box.state(&binary), legacy.state, "files, {describe}");
    legacy
}

const BASIC: &str = "Global:
  ManageImages: true
  BootMountPoint: {root}/esp
  DracutConfDir: {root}/dracut.conf.d
  PreHooksDir: {root}/pre.d
  PostHooksDir: {root}/post.d
Components:
  ImageDir: {root}/esp/EFI/zbm
  Versions: 3
  Enabled: true
EFI:
  ImageDir: {root}/esp/EFI/zbm
  Versions: false
  Enabled: true
  Stub: /usr/lib/systemd/boot/efi/linuxx64.efi.stub
Kernel:
  CommandLine: ro quiet loglevel=0
";

const COMPONENTS: &str = "Global:
  ManageImages: true
  DracutConfDir: {root}/dracut.conf.d
Components:
  ImageDir: {root}/esp/EFI/zbm
  Versions: 3
  Enabled: true
";

#[test]
fn builds_images() {
    let mut parity = parity!("builds_images");
    if (parity.mode().needs_legacy() && !tools()) || !efi_tools() {
        eprintln!("skipped: perl, binutils or the systemd EFI stub is missing");
        return;
    }
    compare(
        &mut parity,
        &Case::new(BASIC, &["-c", "{root}/config.yaml", "-b", "{root}/boot"]),
    );
    compare(
        &mut parity,
        &Case::new(
            BASIC,
            &["-d", "-c", "{root}/config.yaml", "-b", "{root}/boot"],
        ),
    );
    compare(
        &mut parity,
        &Case::new(
            BASIC,
            &[
                "-c",
                "{root}/config.yaml",
                "-b",
                "{root}/boot",
                "--kver",
                "6.1.0-test",
                "-l",
                "rw",
            ],
        ),
    );
    compare(
        &mut parity,
        &Case::new(
            BASIC,
            &[
                "-c",
                "{root}/config.yaml",
                "-k",
                "{root}/boot/linux-lts",
                "-p",
                "zbm",
                "-v",
                "9.9",
            ],
        ),
    );
    // The boot partition is not mounted: it is mounted, and unmounted at the end.
    compare(
        &mut parity,
        &Case {
            env: &[("FAKE_MOUNTPOINT_STATUS", "1")],
            ..Case::new(BASIC, &["-c", "{root}/config.yaml", "-b", "{root}/boot"])
        },
    );
}

#[test]
fn keeps_versions() {
    let mut parity = parity!("keeps_versions");
    if (parity.mode().needs_legacy() && !tools()) || !efi_tools() {
        eprintln!("skipped: perl, binutils or the systemd EFI stub is missing");
        return;
    }
    let versioned = "Global:
  ManageImages: true
  DracutConfDir: {root}/dracut.conf.d
  Version: 3.1.0
Components:
  ImageDir: {root}/esp/EFI/zbm
  Versions: 2
  Enabled: true
EFI:
  ImageDir: {root}/esp/EFI/zbm
  Versions: 2
  Enabled: true
  Stub: /usr/lib/systemd/boot/efi/linuxx64.efi.stub
";
    let setup: Setup = |sandbox| {
        std::fs::create_dir_all(sandbox.path().join("esp/EFI/zbm")).unwrap();
        for name in [
            "vmlinuz-3.1.0_1",
            "vmlinuz-3.1.0_2",
            "vmlinuz-3.1.0_3",
            "vmlinuz-3.0.0_1",
            "vmlinuz-2.0_1",
            "vmlinuz-2.0_2",
            "vmlinuz-1.0_1",
            "initramfs-3.1.0_1.img",
            "initramfs-3.1.0_2.img",
            "initramfs-3.1.0_3.img",
            "initramfs-3.0.0_1.img",
            "initramfs-2.0_2.img",
            "initramfs-1.0_1.img",
            "vmlinuz-3.1.0_1.EFI",
            "vmlinuz-3.1.0_7.EFI",
            "vmlinuz-3.0.0_1.EFI",
            "vmlinuz-2.0_4.EFI",
            "VMLINUZ-2.0_5.EFI",
            "vmlinuz-backup.EFI",
        ] {
            sandbox.dir.write(format!("esp/EFI/zbm/{name}"), name);
        }
    };
    compare(
        &mut parity,
        &Case {
            setup,
            ..Case::new(
                versioned,
                &["-d", "-c", "{root}/config.yaml", "-b", "{root}/boot"],
            )
        },
    );
    // Without versions, existing images become backups.
    let plain = "Global:
  ManageImages: true
Components:
  ImageDir: {root}/esp/EFI/zbm
  Enabled: true
EFI:
  ImageDir: {root}/esp/EFI/zbm
  Enabled: true
  Stub: /usr/lib/systemd/boot/efi/linuxx64.efi.stub
  SplashImage: {root}/splash.bmp
  DeviceTree: {root}/dtb-%{kernel}.dtb
";
    compare(
        &mut parity,
        &Case {
            setup: |sandbox| {
                setup_backups(sandbox);
                sandbox.dir.write("splash.bmp", "BM splash");
                sandbox.dir.write("dtb-6.2.5-test.dtb", "device tree");
            },
            ..Case::new(
                plain,
                &["-d", "-c", "{root}/config.yaml", "-b", "{root}/boot"],
            )
        },
    );
}

fn setup_backups(sandbox: &Sandbox) {
    std::fs::create_dir_all(sandbox.path().join("esp/EFI/zbm")).unwrap();
    for name in ["vmlinuz.EFI", "vmlinuz-bootmenu", "initramfs-bootmenu.img"] {
        sandbox.dir.write(format!("esp/EFI/zbm/{name}"), name);
    }
}

#[test]
fn chooses_kernels() {
    let mut parity = parity!("chooses_kernels");
    if parity.mode().needs_legacy() && !tools() {
        eprintln!("skipped: perl or binutils is missing");
        return;
    }
    let args: &[&[&str]] = &[
        &["-c", "{root}/config.yaml", "-b", "{root}/boot", "-d"],
        &[
            "-c",
            "{root}/config.yaml",
            "-b",
            "{root}/boot",
            "-K",
            "6.1*",
        ],
        &["-c", "{root}/config.yaml", "-b", "{root}/boot", "-K", "7.0"],
        &[
            "-c",
            "{root}/config.yaml",
            "-b",
            "{root}/boot",
            "-K",
            "5.10.0",
        ],
        &["-c", "{root}/config.yaml", "-k", "{root}/boot/none"],
        &[
            "-c",
            "{root}/config.yaml",
            "-k",
            "{root}/boot/vmlinuz-5.10.0",
        ],
        &["-c", "{root}/config.yaml", "-b", "{root}/empty"],
        &[
            "-c",
            "{root}/config.yaml",
            "-b",
            "{root}/boot",
            "-K",
            "%current",
        ],
    ];
    for list in args {
        compare(&mut parity, &Case::new(COMPONENTS, list));
    }
    // `uname -r` printing nothing is an error; the Perl program used to exit with
    // status 0.
    let outcome = compare(
        &mut parity,
        &Case {
            env: &[("FAKE_UNAME", "none")],
            ..Case::new(
                COMPONENTS,
                &[
                    "-c",
                    "{root}/config.yaml",
                    "-b",
                    "{root}/boot",
                    "-K",
                    "%{CURRENT}",
                ],
            )
        },
    );
    assert_eq!(outcome.code, 1);
    assert!(
        outcome
            .stdout
            .ends_with("Cannot determine current kernel version\n")
    );
    let configured = "Global:
  ManageImages: true
Components:
  ImageDir: {root}/esp/EFI/zbm
  Enabled: true
Kernel:
  Path: {root}/boot/vmlinuz-6.1.0-test
  Version: \"%current\"
  Prefix: kernel
";
    compare(
        &mut parity,
        &Case::new(configured, &["-c", "{root}/config.yaml", "-d"]),
    );
}

#[test]
fn initramfs_generators() {
    let mut parity = parity!("initramfs_generators");
    if parity.mode().needs_legacy() && !tools() {
        eprintln!("skipped: perl or binutils is missing");
        return;
    }
    let cpio = "Global:
  ManageImages: true
  InitCPIO: true
  InitCPIOConfig: {root}/mkinitcpio.conf
  InitCPIOHookDirs:
    - {root}/hooks
    - {root}/more hooks
  InitCPIOFlags: -S autodetect
Components:
  ImageDir: {root}/esp/EFI/zbm
  Enabled: true
";
    compare(
        &mut parity,
        &Case::new(cpio, &["-c", "{root}/config.yaml", "-b", "{root}/boot"]),
    );
    compare(
        &mut parity,
        &Case::new(
            cpio,
            &[
                "-c",
                "{root}/config.yaml",
                "-b",
                "{root}/boot",
                "-H",
                "{root}/h",
                "-d",
            ],
        ),
    );
    compare(
        &mut parity,
        &Case::new(
            cpio,
            &[
                "-c",
                "{root}/config.yaml",
                "-b",
                "{root}/boot",
                "--no-initcpio",
            ],
        ),
    );
    let flags = "Global:
  ManageImages: true
  DracutFlags:
    - \"--add-drivers 'a b'\"
    - --no-early-microcode
Components:
  ImageDir: {root}/esp/EFI/zbm
  Enabled: true
";
    compare(
        &mut parity,
        &Case::new(
            flags,
            &[
                "-c",
                "{root}/config.yaml",
                "-b",
                "{root}/boot",
                "-C",
                "{root}/conf d",
            ],
        ),
    );
    compare(
        &mut parity,
        &Case::new(
            flags,
            &["-c", "{root}/config.yaml", "-b", "{root}/boot", "-i"],
        ),
    );
    compare(
        &mut parity,
        &Case {
            env: &[("FAKE_DRACUT_FAIL", "3")],
            ..Case::new(flags, &["-c", "{root}/config.yaml", "-b", "{root}/boot"])
        },
    );
}

#[test]
fn configuration_states() {
    let mut parity = parity!("configuration_states");
    if parity.mode().needs_legacy() && !tools() {
        eprintln!("skipped: perl or binutils is missing");
        return;
    }
    let disabled =
        "Global:\n  ManageImages: false\n  Other: &x {a: 1}\n  List: [1, 'two', 3.10]\nCopy: *x\n";
    compare(
        &mut parity,
        &Case::new(disabled, &["-c", "{root}/config.yaml"]),
    );
    compare(
        &mut parity,
        &Case::new(disabled, &["-c", "{root}/config.yaml", "--enable"]),
    );
    compare(
        &mut parity,
        &Case::new(BASIC, &["-c", "{root}/config.yaml", "--disable"]),
    );
    compare(
        &mut parity,
        &Case::new(
            BASIC,
            &["-c", "{root}/config.yaml", "--enable", "--disable"],
        ),
    );
    compare(
        &mut parity,
        &Case::new(BASIC, &["-c", "{root}/missing.yaml"]),
    );
    compare(
        &mut parity,
        &Case::new("a: [\n", &["-c", "{root}/config.yaml"]),
    );
    compare(
        &mut parity,
        &Case::new("- a\n", &["-c", "{root}/config.yaml"]),
    );
    compare(
        &mut parity,
        &Case::new(
            "EFI: text\nGlobal:\n  ManageImages: 1\n",
            &["-c", "{root}/config.yaml", "-b", "{root}/boot"],
        ),
    );
    compare(
        &mut parity,
        &Case::new("Global: [1]\n", &["-c", "{root}/config.yaml"]),
    );
    compare(&mut parity, &Case::new(BASIC, &["-V"]));
    compare(&mut parity, &Case::new(BASIC, &["--bogus", "-c"]));
}

#[test]
fn mounts_and_hooks() {
    let mut parity = parity!("mounts_and_hooks");
    if parity.mode().needs_legacy() && !tools() {
        eprintln!("skipped: perl or binutils is missing");
        return;
    }
    let config = "Global:
  ManageImages: true
  BootMountPoint: {root}/my esp
  PreHooksDir: {root}/pre.d
  PostHooksDir: {root}/post.d
Components:
  ImageDir: {root}/esp/zbm
  Enabled: true
";
    compare(
        &mut parity,
        &Case {
            env: &[("FAKE_MOUNTPOINT_STATUS", "1"), ("FAKE_MOUNT_FAIL", "1")],
            ..Case::new(config, &["-c", "{root}/config.yaml", "-b", "{root}/boot"])
        },
    );
    compare(
        &mut parity,
        &Case {
            env: &[("FAKE_MOUNTPOINT_STATUS", "1")],
            ..Case::new(
                config,
                &["-d", "-c", "{root}/config.yaml", "-b", "{root}/boot"],
            )
        },
    );
    // A hook that sends SIGTERM to the generator: the partition is unmounted once
    // and the generator exits. The Perl program used to unmount twice, once in the
    // handler and again in its END block.
    let outcome = compare(
        &mut parity,
        &Case {
            env: &[("FAKE_MOUNTPOINT_STATUS", "1")],
            setup: |sandbox| {
                sandbox.dir.write_executable("pre.d/05-signal", SIGNAL_HOOK);
            },
            ..Case::new(config, &["-c", "{root}/config.yaml", "-b", "{root}/boot"])
        },
    );
    assert_eq!(outcome.code, 1);
    assert!(
        outcome
            .stdout
            .ends_with("<program> terminating on signal TERM\n")
    );
    assert_eq!(outcome.stdout.matches("Unmounting").count(), 1);
    assert_eq!(outcome.state.matches("umount [").count(), 1);
    // A hook killed by a signal reports 128 plus the signal number.
    let outcome = compare(
        &mut parity,
        &Case {
            setup: |sandbox| {
                sandbox.dir.write_executable(
                    "pre.d/05-killed",
                    "#!/bin/sh
kill -KILL $$
",
                );
            },
            ..Case::new(
                config,
                &["-d", "-c", "{root}/config.yaml", "-b", "{root}/boot"],
            )
        },
    );
    assert!(outcome.log.contains("## 137\n"), "{}", outcome.log);
    // A boot directory and kernel names with spaces and quotes, which the Perl
    // program used to pass to the shell unquoted.
    let outcome = compare(
        &mut parity,
        &Case {
            setup: |sandbox| {
                std::fs::create_dir_all(sandbox.path().join("my boot")).unwrap();
                sandbox.dir.write(
                    "my boot/kernel it's",
                    kernel_image(&["Linux version 6.3.0-it's (zbm)"]),
                );
            },
            ..Case::new(
                config,
                &[
                    "-c",
                    "{root}/config.yaml",
                    "-k",
                    "{root}/my boot/kernel it's",
                    "-d",
                ],
            )
        },
    );
    assert_eq!(outcome.code, 0, "{}", outcome.stdout);
    assert!(
        outcome.log.contains("using 6.3.0-it's from kernel strings"),
        "{}",
        outcome.log
    );
    assert!(
        outcome
            .state
            .contains("esp/zbm/kernel it's-bootmenu: 6000 bytes"),
        "{}",
        outcome.state
    );
}

/// A hook that sends SIGTERM to the generator, which may be its parent or the
/// parent of the shell that runs it.
const SIGNAL_HOOK: &str = r#"#!/bin/sh
p=$PPID
[ "$(cat /proc/$p/comm)" = sh ] && p=$(cut -d' ' -f4 /proc/$p/stat)
kill -TERM "$p"
"#;

/// An objdump that prints neither the section alignment nor sections.
const OBJDUMP_NOTHING: &str = "#!/bin/sh\necho \"no headers here\"\n";

/// An objdump that prints the section alignment but no sections.
const OBJDUMP_NO_SECTIONS: &str = r#"#!/bin/sh
[ "$1" = -p ] && printf 'SectionAlignment\t00001000\n'
echo "Idx Name"
"#;

/// Copies the stub into the sandbox and replaces objdump with `script`.
fn fake_objdump(sandbox: &Sandbox, script: &str) {
    std::fs::copy(STUB, sandbox.path().join("stub.efi")).unwrap();
    sandbox.dir.write_executable("bin/objdump", script);
}

#[test]
fn bundle_failures() {
    let mut parity = parity!("bundle_failures");
    if (parity.mode().needs_legacy() && !tools()) || !efi_tools() {
        eprintln!("skipped: perl, binutils or the systemd EFI stub is missing");
        return;
    }
    let config = "Global:
  ManageImages: true
EFI:
  ImageDir: {root}/esp/EFI/zbm
  Enabled: true
  Stub: {root}/stub.efi
";
    compare(
        &mut parity,
        &Case::new(config, &["-c", "{root}/config.yaml", "-b", "{root}/boot"]),
    );
    // An objdump that reports no section alignment or sections: the Perl program
    // used to go on with offsets it could not compute.
    let cases: [(Setup, &str); 2] = [
        (
            |sandbox| fake_objdump(sandbox, OBJDUMP_NOTHING),
            "Unable to determine stub alignment!\n",
        ),
        (
            |sandbox| fake_objdump(sandbox, OBJDUMP_NO_SECTIONS),
            "Unable to determine initial stub offset!\n",
        ),
    ];
    for (setup, expected) in cases {
        let outcome = compare(
            &mut parity,
            &Case {
                setup,
                ..Case::new(config, &["-c", "{root}/config.yaml", "-b", "{root}/boot"])
            },
        );
        assert_eq!(outcome.code, 1);
        assert!(outcome.stdout.ends_with(expected), "{}", outcome.stdout);
    }
    let device_tree = "Global:
  ManageImages: true
EFI:
  ImageDir: {root}/esp/EFI/zbm
  Enabled: true
  Stub: /usr/lib/systemd/boot/efi/linuxx64.efi.stub
  DeviceTree: board.dtb
";
    compare(
        &mut parity,
        &Case::new(
            device_tree,
            &["-c", "{root}/config.yaml", "-b", "{root}/boot"],
        ),
    );
    // An image directory that cannot be created.
    compare(
        &mut parity,
        &Case {
            setup: |sandbox| {
                sandbox.dir.write("esp/EFI", "a file");
            },
            ..Case::new(
                &config.replace("{root}/stub.efi", STUB),
                &["-c", "{root}/config.yaml", "-b", "{root}/boot"],
            )
        },
    );
}
