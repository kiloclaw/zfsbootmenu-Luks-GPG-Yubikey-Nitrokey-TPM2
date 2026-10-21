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
fn the_rust_runtime_needs_zbm() {
    let (code, stdout, stderr) = install(
        "dracut",
        &[
            ("zfsbootmenu_runtime", "rust"),
            ("zfsbootmenu_zbm", "/nonexistent/zbm"),
        ],
    );
    assert_eq!(code, 1);
    assert_eq!(stdout, "runtime=rust\n");
    assert_eq!(
        stderr,
        "derror: zfsbootmenu_runtime=rust: zbm not found at /nonexistent/zbm\n"
    );
    let dir = TempDir::new("zbm-image");
    let zbm = dir.write_executable("zbm", "#!/bin/sh\n");
    let zbm = zbm.to_string_lossy().into_owned();
    let (code, stdout, _) = install(
        "mkinitcpio",
        &[("zfsbootmenu_runtime", "Rust"), ("zfsbootmenu_zbm", &zbm)],
    );
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        format!("add_binary {zbm} /libexec/zbm\nruntime=rust\n")
    );
}

#[test]
fn other_runtimes_are_refused() {
    let (code, stdout, stderr) = install("mkinitcpio", &[("zfsbootmenu_runtime", "bogus")]);
    assert_eq!(code, 1);
    assert_eq!(stdout, "runtime=\n");
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

/// Stubs for the dracut functions `module-setup.sh` calls: files are copied
/// into `initdir`, and hooks are recorded in `initdir/hooks.list`.
const DRACUT_STUBS: &str = r#"
inst_simple() { mkdir -p "$(dirname "${initdir}/$2")" && cp -P "$1" "${initdir}/$2"; }
inst_binary() { mkdir -p "$(dirname "${initdir}/$2")" && cp "$1" "${initdir}/$2"; }
inst() { :; }
inst_rules() { :; }
inst_library() { :; }
dracut_install() { :; }
inst_hook() { echo "$1 $2 ${3##*/}" >> "${initdir}/hooks.list"; }
mark_hostonly() { :; }
dfatal() { echo "dfatal: $*" >&2; }
dwarning() { echo "dwarning: $*" >&2; }
dinfo() { echo "dinfo: $*" >&2; }
derror() { echo "derror: $*" >&2; }
"#;

/// Runs the `install` function of the dracut module into a new directory,
/// with `settings` as the dracut configuration.
fn dracut_install(settings: &[(&str, &str)]) -> TempDir {
    let initdir = TempDir::new("zbm-dracut");
    std::fs::create_dir_all(initdir.join("etc/cmdline.d")).unwrap();
    let root = zbm_testutil::repo_root();
    let mut bash = Bash::new()
        .code(DRACUT_STUBS)
        .code(&format!(
            "source {}/packaging/dracut/module-setup.sh",
            root.display()
        ))
        .env("initdir", &initdir.path().to_string_lossy())
        .env("moddir", &root.join("packaging/dracut").to_string_lossy())
        .env(
            "zfsbootmenu_module_root",
            &root.join("legacy/zfsbootmenu").to_string_lossy(),
        )
        .env("zfsbootmenu_hook_root", "/nonexistent")
        .env("zfsbootmenu_luks", "no")
        .env("zfsbootmenu_release_build", "yes")
        .env("PATH", "/usr/bin:/bin");
    for (key, value) in settings {
        bash = bash.env(key, value);
    }
    let output = bash
        .code("find_libgcc_s() { echo; }")
        .call("install", &[], b"");
    assert!(output.status.success(), "{}", output.stderr_text());
    assert!(
        !output.stderr_text().contains("dfatal"),
        "{}",
        output.stderr_text()
    );
    initdir
}

#[test]
fn dracut_images_start_the_selected_runtime() {
    let dir = TempDir::new("zbm-image-zbm");
    let zbm = dir.write_executable("zbm", "#!/bin/sh\n");
    let zbm = zbm.to_string_lossy().into_owned();
    let settings = [
        ("zfsbootmenu_zbm", zbm.as_str()),
        ("embedded_kcl", "zbm.timeout=3"),
    ];

    let legacy = dracut_install(&settings);
    let hooks = std::fs::read_to_string(legacy.join("hooks.list")).unwrap();
    assert!(
        hooks.contains("cmdline 95 zfsbootmenu-parse-commandline.sh\n"),
        "{hooks}"
    );
    assert!(
        hooks.contains("pre-mount 90 zfsbootmenu-preinit.sh\n"),
        "{hooks}"
    );
    assert!(legacy.join("libexec/zfsbootmenu-init").exists());
    assert!(legacy.join("libexec/init.d/50-import-pools").exists());
    let conf = std::fs::read_to_string(legacy.join("etc/zfsbootmenu.conf")).unwrap();
    assert!(conf.contains("export ZBM_RUNTIME=\"legacy\"\n"), "{conf}");
    // The embedded command line survives the configuration file being written.
    assert!(
        conf.contains("export embedded_kcl=\"zbm.timeout=3\"\n"),
        "{conf}"
    );

    let mut rust_settings = settings.to_vec();
    rust_settings.push(("zfsbootmenu_runtime", "rust"));
    let rust = dracut_install(&rust_settings);
    let hooks = std::fs::read_to_string(rust.join("hooks.list")).unwrap();
    assert!(
        hooks.contains("cmdline 95 zfsbootmenu-rust-cmdline.sh\n"),
        "{hooks}"
    );
    assert!(
        hooks.contains("pre-mount 90 zfsbootmenu-rust-preinit.sh\n"),
        "{hooks}"
    );
    assert!(!hooks.contains("zfsbootmenu-preinit.sh\n"), "{hooks}");
    assert!(rust.join("libexec/zbm").exists());
    // The Rust start-up program replaces the legacy one; the shell libraries
    // stay for hooks and the recovery shell.
    assert!(!rust.join("libexec/zfsbootmenu-init").exists());
    assert!(!rust.join("libexec/init.d").exists());
    assert!(rust.join("lib/zfsbootmenu-core.sh").exists());
    assert!(
        rust.join("libexec/hooks/early-setup.d/30-console-autosize.sh")
            .exists()
    );
    let conf = std::fs::read_to_string(rust.join("etc/zfsbootmenu.conf")).unwrap();
    assert!(conf.contains("export ZBM_RUNTIME=\"rust\"\n"), "{conf}");
}
