//! The sandboxes the parity tests run both programs in, with fake programs and
//! kernels, and the comparison of what the programs did.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use zbm_platform::exec::{Command, Output, Runner, Stream, SystemRunner};
use zbm_testkit::parity::Parity;
use zbm_testkit::{TempDir, have_program, legacy};

pub(crate) const STUB: &str = "/usr/lib/systemd/boot/efi/linuxx64.efi.stub";

/// Whether the tools the EFI cases need are available.
pub(crate) fn efi_tools() -> bool {
    Path::new(STUB).is_file() && have_program("objcopy") && have_program("objdump")
}

pub(crate) fn tools() -> bool {
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

/// Logs the build option of the console fonts, which only the Rust program
/// sets.
const LOG_FONT: &str = r#"[ -z "$zfsbootmenu_console_font" ] \
  || echo "zfsbootmenu_console_font=$zfsbootmenu_console_font" >> "$ZBM_FAKE_LOG"
"#;

/// Logs the build options of the graphics drivers, which only the Rust
/// program sets.
const LOG_GRAPHICS: &str = r#"[ -z "$zfsbootmenu_graphics_drivers" ] \
  || echo "zfsbootmenu_graphics_drivers=$zfsbootmenu_graphics_drivers" >> "$ZBM_FAKE_LOG"
[ -z "$zfsbootmenu_graphics_firmware" ] \
  || echo "zfsbootmenu_graphics_firmware=$zfsbootmenu_graphics_firmware" >> "$ZBM_FAKE_LOG"
"#;

/// modinfo prints the firmware a driver lists from `modinfo/<driver>` in the
/// sandbox, which only the Rust program asks for.
const FAKE_MODINFO: &str = r#"eval "module=\${$#}"
if [ -f "$ZBM_PARITY_CWD/modinfo/$module" ]; then
  cat "$ZBM_PARITY_CWD/modinfo/$module"
else
  echo "modinfo: ERROR: Module $module not found." >&2
  exit 1
fi
"#;

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

/// The real objcopy, with a fixed time for the PE header, so that both
/// programs' bundles are identical; the generators themselves run without
/// `SOURCE_DATE_EPOCH`, which makes the Rust one build reproducibly.
const OBJCOPY: &str = r#"#!/bin/sh
PATH=/usr/bin:/bin SOURCE_DATE_EPOCH=0 exec objcopy "$@"
"#;

fn fake_programs() -> Vec<(&'static str, String)> {
    vec![
        (
            "dracut",
            logging_script(&format!("{LOG_FONT}{LOG_GRAPHICS}{FAKE_DRACUT}")),
        ),
        (
            "mkinitcpio",
            logging_script(&format!("{LOG_FONT}{LOG_GRAPHICS}{FAKE_MKINITCPIO}")),
        ),
        (
            "mountpoint",
            logging_script("exit \"${FAKE_MOUNTPOINT_STATUS:-0}\"\n"),
        ),
        ("mount", logging_script(FAKE_MOUNT)),
        ("modinfo", logging_script(FAKE_MODINFO)),
        ("umount", logging_script("")),
        ("uname", FAKE_UNAME.to_owned()),
        ("objcopy", OBJCOPY.to_owned()),
    ]
}

/// A fake kernel whose printable strings include `strings`.
pub(crate) fn kernel_image(strings: &[&str]) -> Vec<u8> {
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
pub(crate) type Setup = fn(&Sandbox);

/// One scenario: a configuration and arguments, with `{root}` standing for the
/// sandbox, and extra setup.
pub(crate) struct Case {
    pub(crate) config: String,
    pub(crate) args: &'static [&'static str],
    pub(crate) env: &'static [(&'static str, &'static str)],
    pub(crate) setup: Setup,
    /// Intended differences from the Perl program (CHANGELOG.md): each
    /// text of its outcome that the Rust program changes on purpose, and
    /// what it is instead.
    pub(crate) differs: &'static [(&'static str, &'static str)],
}

impl Case {
    pub(crate) fn new(config: &str, args: &'static [&'static str]) -> Case {
        Case {
            config: config.to_owned(),
            args,
            env: &[],
            setup: |_| {},
            differs: &[],
        }
    }
}

/// A sandbox for one implementation.
pub(crate) struct Sandbox {
    pub(crate) dir: TempDir,
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

    pub(crate) fn path(&self) -> &Path {
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
            .env_remove("SOURCE_DATE_EPOCH")
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
pub(crate) struct Outcome {
    pub(crate) code: i32,
    pub(crate) stdout: String,
    pub(crate) log: String,
    pub(crate) state: String,
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

/// What the Rust program did in a case that the Perl program has no part in.
pub(crate) struct RustRun {
    pub(crate) code: i32,
    pub(crate) stdout: String,
    /// The commands the fake programs logged.
    pub(crate) log: String,
    /// The files the run left, as the comparisons describe them.
    pub(crate) state: String,
}

/// Runs a case with the Rust program alone, for configuration keys that the
/// Perl program does not read.
pub(crate) fn run_rust(case: &Case) -> RustRun {
    let binary = env!("CARGO_BIN_EXE_generate-zbm").to_owned();
    let rust_box = Sandbox::new(case);
    let rust = rust_box.run(std::slice::from_ref(&binary), case);
    let log = std::fs::read_to_string(rust_box.path().join("calls.log")).unwrap_or_default();
    RustRun {
        code: rust.status.shell_code(),
        stdout: rust_box.normalize(&rust.stdout_text(), &binary),
        log: rust_box.normalize(&log, &binary),
        state: rust_box.state(&binary),
    }
}

/// Runs a case with the Rust program, checks that it does what the Perl
/// program did (recorded in `parity`) and returns that.
pub(crate) fn compare(parity: &mut Parity, case: &Case) -> Outcome {
    let describe = format!(
        "args {:?}, env {:?}, config:\n{}",
        case.args, case.env, case.config
    );
    let mut legacy: Outcome = parity.legacy(describe.clone(), || run_perl(case));
    for (legacy_text, rust_text) in case.differs {
        for text in [&mut legacy.stdout, &mut legacy.log, &mut legacy.state] {
            *text = text.replace(legacy_text, rust_text);
        }
    }
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
