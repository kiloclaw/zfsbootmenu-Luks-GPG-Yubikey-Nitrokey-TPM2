//! Tests of the image build helpers that select the runtime and install `zbm`:
//! `install_zbm_runtime` and the `ZBM_RUNTIME` line of `create_zbm_conf` in
//! `legacy/zfsbootmenu/install-helpers.sh`, run with stubs for the dracut and
//! mkinitcpio functions they call.

use zbm_testutil::TempDir;
use zbm_testutil::legacy::Bash;

/// Stubs for the dracut and mkinitcpio functions the helpers use; each records
/// its call on standard output.
const BUILDER_STUBS: &str = r#"
inst_binary() { echo "inst_binary $*"; [ -z "$FAIL_INSTALL" ]; }
add_binary() { echo "add_binary $*"; [ -z "$FAIL_INSTALL" ]; }
dinfo() { echo "dinfo: $*" >&2; }
msg2() { echo "msg2: $*" >&2; }
derror() { echo "derror: $*" >&2; }
error() { echo "error: $*" >&2; }
"#;

/// Runs `install_zbm_runtime` with the given build style and settings, then
/// prints the selected runtime.
fn install(style: &str, settings: &[(&str, &str)]) -> (i32, String, String) {
    let mut bash = Bash::new()
        .code(BUILDER_STUBS)
        .source("zfsbootmenu/install-helpers.sh")
        .code(&format!("ZBM_BUILDSTYLE={style}"));
    for (key, value) in settings {
        bash = bash.env(key, value);
    }
    let output = bash
        .code(
            "check() { install_zbm_runtime; local rc=$?; \
             echo \"runtime=${ZBM_RUNTIME}\"; return $rc; }",
        )
        .call("check", &[], b"");
    (
        output.status.shell_code(),
        output.stdout_text(),
        output.stderr_text(),
    )
}

#[test]
fn the_legacy_runtime_is_the_default() {
    let (code, stdout, stderr) = install("dracut", &[("zfsbootmenu_zbm", "/nonexistent/zbm")]);
    assert_eq!(code, 0);
    assert_eq!(stdout, "runtime=legacy\n");
    assert_eq!(
        stderr,
        "dinfo: zbm not found at /nonexistent/zbm; the legacy runtime does not need it\n"
    );
}

#[test]
fn zbm_is_installed_when_found() {
    let dir = TempDir::new("zbm-image");
    let zbm = dir.write_executable("zbm", "#!/bin/sh\n");
    let zbm = zbm.to_string_lossy().into_owned();
    let (code, stdout, _) = install(
        "dracut",
        &[("zfsbootmenu_runtime", "Legacy"), ("zfsbootmenu_zbm", &zbm)],
    );
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        format!("inst_binary {zbm} /libexec/zbm\nruntime=legacy\n")
    );
    let (code, stdout, _) = install("mkinitcpio", &[("zfsbootmenu_zbm", &zbm)]);
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        format!("add_binary {zbm} /libexec/zbm\nruntime=legacy\n")
    );
    let (code, _, stderr) = install(
        "mkinitcpio",
        &[("zfsbootmenu_zbm", &zbm), ("FAIL_INSTALL", "1")],
    );
    assert_eq!(code, 1);
    assert_eq!(stderr, format!("error: failed to install {zbm}\n"));
}

#[test]
fn other_runtimes_are_refused() {
    let (code, stdout, stderr) = install("dracut", &[("zfsbootmenu_runtime", "rust")]);
    assert_eq!(code, 1);
    assert_eq!(stdout, "runtime=\n");
    assert!(
        stderr
            .starts_with("derror: zfsbootmenu_runtime=rust: the Rust runtime is not available yet")
    );
    let (code, _, stderr) = install("mkinitcpio", &[("zfsbootmenu_runtime", "bogus")]);
    assert_eq!(code, 1);
    assert!(stderr.starts_with("error: zfsbootmenu_runtime=bogus: unknown runtime"));
}

#[test]
fn the_image_records_the_runtime() {
    let dir = TempDir::new("zbm-image-conf");
    std::fs::create_dir_all(dir.join("etc")).unwrap();
    let output = Bash::new()
        .code(BUILDER_STUBS)
        .source("zfsbootmenu/install-helpers.sh")
        .env("BUILDROOT", &dir.path().to_string_lossy())
        .env("PATH", "/usr/bin:/bin")
        .code("ZBM_BUILDSTYLE=dracut; ZBM_RUNTIME=legacy")
        .call("create_zbm_conf", &[], b"");
    assert!(output.status.success(), "{}", output.stderr_text());
    let conf = std::fs::read_to_string(dir.join("etc/zfsbootmenu.conf")).unwrap();
    assert!(conf.contains("export ZBM_RUNTIME=\"legacy\"\n"), "{conf}");
}
