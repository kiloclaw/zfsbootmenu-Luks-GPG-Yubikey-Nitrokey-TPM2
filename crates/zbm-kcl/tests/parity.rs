//! Parity tests: `legacy/bin/zbm-kcl` and the Rust `zbm-kcl` run with the same arguments, environment and input,
//! against the same fake `zfs` and real EFI images, and must produce the same exit code, output, `zfs` calls,
//! properties and files.

use std::path::{Path, PathBuf};

use zbm_core::exec::{Command, Input, Output, Runner, Stream, SystemRunner};
use zbm_testutil::fake_zfs::FakeZfs;
use zbm_testutil::{TempDir, have_program, legacy, repo_root};

const STUB: &str = "/usr/lib/systemd/boot/efi/linuxx64.efi.stub";
const PROPERTY: &str = "org.zfsbootmenu:commandline";
const DATASETS: &[&str] = &["pool", "pool/be", "pool/other", "pool/unset", "pool/flag"];

/// Whether the tools the EFI cases need are available; the cases are skipped otherwise.
fn efi_tools() -> bool {
    Path::new(STUB).is_file() && have_program("objcopy") && have_program("objdump")
}

fn run(command: Command) -> Output {
    SystemRunner
        .run(&command)
        .expect("the program can be started")
}

fn hex(field: &str) -> u64 {
    u64::from_str_radix(field, 16).expect("objdump prints hex numbers")
}

/// Builds an EFI image from the systemd stub with the given `.cmdline` section (none when `None`), followed by a
/// `.linux` section one alignment unit later, as `generate-zbm` lays them out.
fn build_efi(dir: &Path, name: &str, cmdline: Option<&[u8]>) -> PathBuf {
    let headers = run(Command::new("objdump").args(["-w", "-h", STUB])).stdout_text();
    let last: Vec<&str> = headers.lines().last().unwrap().split_whitespace().collect();
    let end = hex(last[2]) + hex(last[3]);
    let align = 0x1000;
    let start = end.div_ceil(align) * align;
    let linux = dir.join(format!("{name}.linux"));
    std::fs::write(&linux, vec![b'L'; 0x3000]).unwrap();
    let image = dir.join(name);
    let mut command = Command::new("objcopy");
    let mut next = start;
    if let Some(cmdline) = cmdline {
        let file = dir.join(format!("{name}.cmdline"));
        std::fs::write(&file, cmdline).unwrap();
        command = command
            .arg("--add-section")
            .arg(format!(".cmdline={}", file.display()))
            .arg("--change-section-vma")
            .arg(format!(".cmdline=0x{start:X}"));
        next += align;
    }
    command = command
        .arg("--add-section")
        .arg(format!(".linux={}", linux.display()))
        .arg("--change-section-vma")
        .arg(format!(".linux=0x{next:X}"))
        .arg(STUB)
        .arg(&image)
        .stderr(Stream::Capture)
        // objcopy stamps the PE header with the time unless this is set; a fixed stamp makes the images of the
        // two sandboxes identical.
        .env("SOURCE_DATE_EPOCH", "0");
    let output = run(command);
    assert!(output.status.success(), "{}", output.stderr_text());
    image
}

/// A sandbox for one implementation: a working directory, a fake `zfs`, a temporary directory and an editor.
struct Sandbox {
    work: TempDir,
    tmp: TempDir,
    zfs: FakeZfs,
}

impl Sandbox {
    fn new(properties: &[(&str, &str)], images: &[(&str, Option<&[u8]>)]) -> Sandbox {
        let sandbox = Sandbox {
            work: TempDir::new("zbm-kcl-parity-work"),
            tmp: TempDir::new("zbm-kcl-parity-tmp"),
            zfs: FakeZfs::new(DATASETS),
        };
        for (dataset, value) in properties {
            sandbox.zfs.set(dataset, PROPERTY, value);
        }
        for (name, cmdline) in images {
            build_efi(sandbox.work.path(), name, *cmdline);
        }
        sandbox.work.write_executable(
            "edit-ok",
            "#!/bin/sh\nsed -i -e '1s/quiet/loud/' -e '1a\\\\\nadded # with a comment' \"$1\"\n",
        );
        sandbox
            .work
            .write_executable("edit-fail", "#!/bin/sh\nexit 3\n");
        sandbox
    }

    /// Runs `program` (with `prefix` arguments before `args`) in the sandbox.
    fn run(&self, program: &[String], args: &[&str], editor: Option<&str>, stdin: &[u8]) -> Output {
        let mut command = Command::new(&program[0])
            .args(&program[1..])
            .args(args.iter().copied())
            .stdin(Input::Bytes(stdin.to_vec()))
            .stderr(Stream::Capture)
            .env("PATH", self.zfs.path_env())
            .env("TMPDIR", self.tmp.path())
            .env("LC_ALL", "C")
            // Both programs modify images with objcopy, which would otherwise stamp them with the current time.
            .env("SOURCE_DATE_EPOCH", "0");
        command = match editor {
            Some(editor) => command.env("EDITOR", self.work.join(editor)),
            None => command.env_remove("EDITOR"),
        };
        run(command.env("ZBM_PARITY_CWD", self.work.path()))
    }

    /// The state the run left: properties, `zfs` calls and the files of the working directory.
    fn state(&self) -> String {
        let mut state = String::new();
        for dataset in DATASETS {
            state.push_str(&format!(
                "{dataset}: {:?}\n",
                self.zfs.local(dataset, PROPERTY)
            ));
        }
        state.push_str(&self.zfs.calls());
        let mut files: Vec<_> = std::fs::read_dir(self.work.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        files.sort();
        for file in files {
            let bytes = std::fs::read(&file).unwrap();
            state.push_str(&format!(
                "{}: {} bytes, {:x}\n",
                file.file_name().unwrap().to_string_lossy(),
                bytes.len(),
                bytes.iter().fold(0_u64, |hash, &byte| hash
                    .wrapping_mul(31)
                    .wrapping_add(u64::from(byte)))
            ));
        }
        let leftovers = std::fs::read_dir(self.tmp.path()).unwrap().count();
        state.push_str(&format!("temporary files left: {leftovers}\n"));
        state
    }

    /// Replaces the sandbox's paths and temporary names in program output.
    fn normalize(&self, text: &str, program: &str) -> String {
        let text = text
            .replace(program, "zbm-kcl")
            .replace(&self.work.path().display().to_string(), "<work>");
        let tmp = format!("{}/tmp.", self.tmp.path().display());
        let mut result = String::new();
        let mut rest = text.as_str();
        while let Some(found) = rest.find(&tmp) {
            result.push_str(&rest[..found]);
            result.push_str("<tmp>");
            rest = &rest[found + tmp.len()..];
            let name_len = rest
                .find(|c: char| !c.is_ascii_alphanumeric())
                .unwrap_or(rest.len());
            rest = &rest[name_len..];
        }
        result.push_str(rest);
        result
    }
}

/// Runs a case with both implementations and compares everything.
fn compare(
    properties: &[(&str, &str)],
    images: &[(&str, Option<&[u8]>)],
    args: &[&str],
    editor: Option<&str>,
    stdin: &[u8],
) {
    let script = legacy::path("bin/zbm-kcl").display().to_string();
    let binary = env!("CARGO_BIN_EXE_zbm-kcl").to_owned();
    // Both programs run from the sandbox's working directory, so relative paths in `args` name the same files.
    let in_dir = |program: &str| -> Vec<String> {
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            "cd \"$ZBM_PARITY_CWD\" && exec \"$@\"".to_owned(),
            "sh".to_owned(),
            program.to_owned(),
        ]
    };
    let mut legacy_program = in_dir("bash");
    legacy_program.push(script.clone());
    let rust_program = in_dir(&binary);

    let legacy_box = Sandbox::new(properties, images);
    let rust_box = Sandbox::new(properties, images);
    let legacy = legacy_box.run(&legacy_program, args, editor, stdin);
    let rust = rust_box.run(&rust_program, args, editor, stdin);

    let describe = format!("args {args:?}, editor {editor:?}");
    assert_eq!(
        rust.status.shell_code(),
        legacy.status.shell_code(),
        "exit code, {describe}\nlegacy stderr: {}\nrust stderr: {}",
        legacy.stderr_text(),
        rust.stderr_text()
    );
    assert_eq!(
        rust_box.normalize(&String::from_utf8_lossy(&rust.stdout), &binary),
        legacy_box.normalize(&String::from_utf8_lossy(&legacy.stdout), &script),
        "stdout, {describe}"
    );
    if !rust.stdout.is_empty() && rust.stdout.starts_with(b"MZ") {
        assert_eq!(
            rust.stdout, legacy.stdout,
            "EFI image on stdout, {describe}"
        );
    }
    assert_eq!(
        rust_box.normalize(&rust.stderr_text(), &binary),
        legacy_box.normalize(&legacy.stderr_text(), &script),
        "stderr, {describe}"
    );
    assert_eq!(rust_box.state(), legacy_box.state(), "state, {describe}");
}

#[test]
fn boot_environment_cases_match() {
    let properties: &[(&str, &str)] = &[
        ("pool/be", "quiet  loglevel=4 \"a  b\" rw"),
        ("pool/flag", "-n"),
        ("pool", "%{parent} spl_hostid=1"),
    ];
    let cases: &[&[&str]] = &[
        &["pool/be"],
        &["pool/unset"],
        &["pool/flag"],
        &["pool"],
        &["-a", "nomodeset", "-r", "quiet", "pool/be"],
        &["-a", "a b", "-a", "x=\"1 2\"", "-a", "", "pool/be"],
        &["-r", "loglevel", "-a", "loglevel=7", "pool/be"],
        &["-d", "pool/be"],
        &["-d", "-a", "only", "pool/be"],
        &[
            "-r", "quiet", "-r", "loglevel", "-r", "\"a b\"", "-r", "rw", "pool/be",
        ],
        &["-a", "x", "-o", "pool/other", "pool/be"],
        &["-a", "x", "-o", "missing", "pool/be"],
        &["-a", "x", "-o", "", "pool/be"],
        &["missing"],
        &["-x"],
        &["-a"],
        &["-h"],
        &["-ed"],
        &["--", "-a"],
        &[],
        &["pool/be", "extra", "operands"],
    ];
    for args in cases {
        compare(properties, &[], args, None, b"");
    }
}

#[test]
fn editing_matches() {
    let properties: &[(&str, &str)] = &[("pool/be", "quiet loglevel=4")];
    compare(properties, &[], &["-e", "pool/be"], Some("edit-ok"), b"");
    compare(properties, &[], &["-e", "pool/be"], Some("edit-fail"), b"");
    compare(
        properties,
        &[],
        &["-e", "pool/be"],
        Some("missing-editor"),
        b"",
    );
    compare(
        properties,
        &[],
        &["-e", "-a", "x", "pool/be"],
        Some("edit-ok"),
        b"",
    );
}

#[test]
fn efi_image_cases_match() {
    if !efi_tools() {
        eprintln!("skipped: the systemd EFI stub, objcopy or objdump is missing");
        return;
    }
    let images: &[(&str, Option<&[u8]>)] = &[
        ("image.efi", Some(b"root=zfs:AUTO quiet  \"a  b\"\0")),
        ("plain.efi", None),
    ];
    let long = "x".repeat(5000);
    let cases: &[&[&str]] = &[
        &["image.efi"],
        &["-a", "zbm.timeout=5", "image.efi"],
        &["-r", "quiet", "-o", "out.efi", "image.efi"],
        &["-d", "image.efi"],
        &["-a", &long, "image.efi"],
        &["plain.efi"],
        &["-a", "x", "plain.efi"],
        &["-a", "x", "-o", "missing-dir/out.efi", "image.efi"],
    ];
    for args in cases {
        compare(&[], images, args, None, b"");
    }
    compare(&[], images, &["-e", "image.efi"], Some("edit-ok"), b"");
}

#[test]
fn streamed_efi_images_match() {
    if !efi_tools() {
        eprintln!("skipped: the systemd EFI stub, objcopy or objdump is missing");
        return;
    }
    let dir = TempDir::new("zbm-kcl-parity-stream");
    let image = build_efi(dir.path(), "stream.efi", Some(b"quiet rw\0"));
    let bytes = std::fs::read(image).unwrap();
    compare(&[], &[], &["-"], None, &bytes);
    compare(
        &[],
        &[],
        &["-a", "loglevel=7", "-o", "-", "-"],
        None,
        &bytes,
    );
    compare(&[], &[], &["-e", "-"], Some("edit-ok"), &bytes);
}

#[test]
fn repository_layout_is_as_expected() {
    assert!(repo_root().join("legacy/bin/zbm-kcl").is_file());
}
