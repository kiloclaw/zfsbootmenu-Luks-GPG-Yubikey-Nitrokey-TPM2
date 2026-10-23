//! Tests of the image build helpers that select the runtime and install `zbm`:
//! `install_zbm_runtime` and the `ZBM_RUNTIME` line of `create_zbm_conf` in
//! `legacy/zfsbootmenu/install-helpers.sh`, and the dracut module, run with
//! stubs for the dracut and mkinitcpio functions they call.
//!
//! The helpers are shell code until R6 of REFACTOR-ROADMAP.md, so their runs
//! are recorded as golden files (`zbm_testkit::parity`): a normal test run
//! reads the recordings, and `cargo xtask record-parity --check` runs the
//! helpers again. A `zbm` program that exists is written `$ZBM` in settings
//! and results.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use zbm_testkit::legacy::Bash;
use zbm_testkit::parity::Parity;
use zbm_testkit::{TempDir, parity};

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

/// What `install` and `dracut_install` call the `zbm` program that exists.
const ZBM: &str = "$ZBM";

/// `settings` with [`ZBM`] replaced by `zbm`.
fn with_zbm(settings: &[(&str, &str)], zbm: &str) -> Vec<(String, String)> {
    settings
        .iter()
        .map(|(key, value)| ((*key).to_owned(), value.replace(ZBM, zbm)))
        .collect()
}

/// Runs `install_zbm_runtime` with the given build style and settings, then
/// prints the selected runtime: the exit status, standard output and standard
/// error.
fn install(parity: &mut Parity, style: &str, settings: &[(&str, &str)]) -> (i32, String, String) {
    parity.legacy(format!("{style} {settings:?}"), || {
        let dir = TempDir::new("zbm-image");
        let zbm = dir.write_executable("zbm", "#!/bin/sh\n");
        let zbm = zbm.to_string_lossy().into_owned();
        let mut bash = Bash::new()
            .code(BUILDER_STUBS)
            .source("zfsbootmenu/install-helpers.sh")
            .code(&format!("ZBM_BUILDSTYLE={style}"));
        for (key, value) in with_zbm(settings, &zbm) {
            bash = bash.env(&key, &value);
        }
        let output = bash
            .code(
                "check() { install_zbm_runtime; local rc=$?; \
                 echo \"runtime=${ZBM_RUNTIME}\"; return $rc; }",
            )
            .call("check", &[], b"");
        (
            output.status.shell_code(),
            output.stdout_text().replace(&zbm, ZBM),
            output.stderr_text().replace(&zbm, ZBM),
        )
    })
}

#[test]
fn the_legacy_runtime_is_the_default() {
    let mut parity = parity!("the_legacy_runtime_is_the_default");

    let (code, stdout, stderr) = install(
        &mut parity,
        "dracut",
        &[("zfsbootmenu_zbm", "/nonexistent/zbm")],
    );

    assert_eq!(code, 0);
    assert_eq!(stdout, "runtime=legacy\n");
    assert_eq!(
        stderr,
        "dinfo: zbm not found at /nonexistent/zbm; the legacy runtime does not need it\n"
    );
}

#[test]
fn zbm_is_installed_when_found() {
    let mut parity = parity!("zbm_is_installed_when_found");

    let (code, stdout, _) = install(
        &mut parity,
        "dracut",
        &[("zfsbootmenu_runtime", "Legacy"), ("zfsbootmenu_zbm", ZBM)],
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "inst_binary $ZBM /libexec/zbm\nruntime=legacy\n");
    let (code, stdout, _) = install(&mut parity, "mkinitcpio", &[("zfsbootmenu_zbm", ZBM)]);
    assert_eq!(code, 0);
    assert_eq!(stdout, "add_binary $ZBM /libexec/zbm\nruntime=legacy\n");
    let (code, _, stderr) = install(
        &mut parity,
        "mkinitcpio",
        &[("zfsbootmenu_zbm", ZBM), ("FAIL_INSTALL", "1")],
    );
    assert_eq!(code, 1);
    assert_eq!(stderr, "error: failed to install $ZBM\n");
}

#[test]
fn the_rust_runtime_needs_zbm() {
    let mut parity = parity!("the_rust_runtime_needs_zbm");

    let (code, stdout, stderr) = install(
        &mut parity,
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
    let (code, stdout, _) = install(
        &mut parity,
        "mkinitcpio",
        &[("zfsbootmenu_runtime", "Rust"), ("zfsbootmenu_zbm", ZBM)],
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "add_binary $ZBM /libexec/zbm\nruntime=rust\n");
}

#[test]
fn other_runtimes_are_refused() {
    let mut parity = parity!("other_runtimes_are_refused");

    let (code, stdout, stderr) = install(
        &mut parity,
        "mkinitcpio",
        &[("zfsbootmenu_runtime", "bogus")],
    );

    assert_eq!(code, 1);
    assert_eq!(stdout, "runtime=\n");
    assert!(stderr.starts_with("error: zfsbootmenu_runtime=bogus: unknown runtime"));
}

#[test]
fn the_image_records_the_runtime() {
    let mut parity = parity!("the_image_records_the_runtime");

    let conf: String = parity.legacy("create_zbm_conf, dracut, legacy", || {
        let dir = TempDir::new("zbm-image-conf");
        std::fs::create_dir_all(dir.join("etc")).unwrap();
        let output = Bash::new()
            .code(BUILDER_STUBS)
            .source("zfsbootmenu/install-helpers.sh")
            .env("BUILDROOT", &dir.path().to_string_lossy())
            .env("PATH", "/usr/bin:/bin")
            .code("ZBM_BUILDSTYLE=dracut; ZBM_RUNTIME=legacy")
            .call("create_zbm_conf", &[], b"");
        assert!(output.status.is_success(), "{}", output.stderr_text());
        std::fs::read_to_string(dir.join("etc/zfsbootmenu.conf")).unwrap()
    });

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

/// The files of an image that the tests read.
const TEXTS: &[&str] = &[
    "hooks.list",
    "bin/zluks",
    "etc/zfsbootmenu.conf",
    "libexec/hooks/teardown.d/80-luks-close.sh",
    "libexec/hooks/early-setup.d/30-console-autosize.sh",
    "libexec/hooks/boot-sel.d/99-zbm-shadow.sh",
];

/// What the dracut module installed: every path, and the texts of [`TEXTS`].
#[derive(Debug, Deserialize, Serialize)]
struct Image {
    /// Each path below the image root: `dir`, `file <mode>` or `link <target>`.
    entries: BTreeMap<String, String>,
    /// The contents of the files of [`TEXTS`] that exist.
    texts: BTreeMap<String, String>,
}

impl Image {
    /// Describes the tree under `root`, writing `zbm` as [`ZBM`].
    fn describe(root: &std::path::Path, zbm: &str) -> Image {
        let mut entries = BTreeMap::new();
        let mut pending = vec![root.to_path_buf()];
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                let relative = path.strip_prefix(root).unwrap().display().to_string();
                let metadata = std::fs::symlink_metadata(&path).unwrap();
                let kind = if metadata.is_symlink() {
                    let target = std::fs::read_link(&path).unwrap();
                    format!("link {}", target.display())
                } else if metadata.is_dir() {
                    pending.push(path);
                    "dir".to_owned()
                } else {
                    let mode = std::os::unix::fs::PermissionsExt::mode(&metadata.permissions());
                    format!("file {:o}", mode & 0o7777)
                };
                entries.insert(relative, kind.replace(zbm, ZBM));
            }
        }
        let texts = TEXTS
            .iter()
            .filter_map(|name| {
                let text = std::fs::read_to_string(root.join(name)).ok()?;
                Some(((*name).to_owned(), text.replace(zbm, ZBM)))
            })
            .collect();
        Image { entries, texts }
    }

    /// Whether `path` exists in the image.
    fn exists(&self, path: &str) -> bool {
        self.entries.contains_key(path)
    }

    /// The text of `path`, one of [`TEXTS`]; empty when it does not exist.
    fn text(&self, path: &str) -> &str {
        self.texts.get(path).map_or("", String::as_str)
    }
}

/// Runs the `install` function of the dracut module into a new directory,
/// with `settings` as the dracut configuration, and describes the image.
fn dracut_install(parity: &mut Parity, settings: &[(&str, &str)]) -> Image {
    parity.legacy(format!("{settings:?}"), || {
        let dir = TempDir::new("zbm-image-zbm");
        let zbm = dir.write_executable("zbm", "#!/bin/sh\n");
        let zbm = zbm.to_string_lossy().into_owned();
        let initdir = TempDir::new("zbm-dracut");
        std::fs::create_dir_all(initdir.join("etc/cmdline.d")).unwrap();
        let root = zbm_testkit::repo_root();
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
                &zbm_testkit::legacy::path("zfsbootmenu").to_string_lossy(),
            )
            .env("zfsbootmenu_hook_root", "/nonexistent")
            .env("zfsbootmenu_luks", "no")
            .env("zfsbootmenu_release_build", "yes")
            .env("PATH", "/usr/bin:/bin");
        for (key, value) in with_zbm(settings, &zbm) {
            bash = bash.env(&key, &value);
        }
        let output = bash
            .code("find_libgcc_s() { echo; }")
            .call("install", &[], b"");
        assert!(output.status.is_success(), "{}", output.stderr_text());
        assert!(
            !output.stderr_text().contains("dfatal"),
            "{}",
            output.stderr_text()
        );
        Image::describe(initdir.path(), &zbm)
    })
}

#[test]
fn dracut_images_start_the_selected_runtime() {
    let mut parity = parity!("dracut_images_start_the_selected_runtime");
    let settings = [("zfsbootmenu_zbm", ZBM), ("embedded_kcl", "zbm.timeout=3")];

    let legacy = dracut_install(&mut parity, &settings);
    let hooks = legacy.text("hooks.list");
    assert!(
        hooks.contains("cmdline 95 zfsbootmenu-parse-commandline.sh\n"),
        "{hooks}"
    );
    assert!(
        hooks.contains("pre-mount 90 zfsbootmenu-preinit.sh\n"),
        "{hooks}"
    );
    assert!(legacy.exists("libexec/zfsbootmenu-init"));
    assert!(legacy.exists("libexec/init.d/50-import-pools"));
    // zluks and the LUKS teardown hook are the legacy scripts.
    assert!(legacy.text("bin/zluks").contains("source"));
    let teardown = "libexec/hooks/teardown.d/80-luks-close.sh";
    assert!(legacy.text(teardown).contains("luks_close_all"));
    let conf = legacy.text("etc/zfsbootmenu.conf");
    assert!(conf.contains("export ZBM_RUNTIME=\"legacy\"\n"), "{conf}");
    // The embedded command line survives the configuration file being written.
    assert!(
        conf.contains("export embedded_kcl=\"zbm.timeout=3\"\n"),
        "{conf}"
    );

    let mut rust_settings = settings.to_vec();
    rust_settings.push(("zfsbootmenu_runtime", "rust"));
    let rust = dracut_install(&mut parity, &rust_settings);
    let hooks = rust.text("hooks.list");
    assert!(
        hooks.contains("cmdline 95 zfsbootmenu-rust-cmdline.sh\n"),
        "{hooks}"
    );
    assert!(
        hooks.contains("pre-mount 90 zfsbootmenu-rust-preinit.sh\n"),
        "{hooks}"
    );
    assert!(!hooks.contains("zfsbootmenu-preinit.sh\n"), "{hooks}");
    assert!(rust.exists("libexec/zbm"));
    // The Rust start-up program replaces the legacy one; the shell libraries
    // stay for hooks and the recovery shell.
    assert!(!rust.exists("libexec/zfsbootmenu-init"));
    assert!(!rust.exists("libexec/init.d"));
    assert!(rust.exists("lib/zfsbootmenu-core.sh"));
    // zbm sizes the console font, with the hook's name, so that
    // zbm.skip_hooks=30-console-autosize.sh still works.
    let autosize = rust.text("libexec/hooks/early-setup.d/30-console-autosize.sh");
    assert!(autosize.starts_with("#!/bin/bash\n"), "{autosize}");
    assert!(
        autosize.ends_with("\nexec /libexec/zbm console-autosize\n"),
        "{autosize}"
    );
    assert_eq!(
        rust.entries["libexec/hooks/early-setup.d/30-console-autosize.sh"],
        "file 755"
    );
    let conf = rust.text("etc/zfsbootmenu.conf");
    assert!(conf.contains("export ZBM_RUNTIME=\"rust\"\n"), "{conf}");
    // zbm is zluks, which also closes the LUKS mappings before booting.
    assert_eq!(rust.entries["bin/zluks"], "link /libexec/zbm");
    let hook = rust.text(teardown);
    assert!(hook.starts_with("#!/bin/bash\n"), "{hook}");
    assert!(hook.contains("\n/bin/zluks close\nexit 0\n"), "{hook}");
    assert!(!hook.contains("luks_close_all"), "{hook}");
    assert_eq!(rust.entries[teardown], "file 755");
}

#[test]
fn shadow_mode_adds_a_boot_hook_to_legacy_images() {
    let mut parity = parity!("shadow_mode_adds_a_boot_hook_to_legacy_images");
    let hook = "libexec/hooks/boot-sel.d/99-zbm-shadow.sh";
    let settings = [("zfsbootmenu_zbm", ZBM)];

    let plain = dracut_install(&mut parity, &settings);
    assert!(!plain.exists(hook));

    let mut shadow_settings = settings.to_vec();
    shadow_settings.push(("zfsbootmenu_shadow", "yes"));
    let shadow = dracut_install(&mut parity, &shadow_settings);
    assert_eq!(shadow.entries[hook], "file 755");
    let text = shadow.text(hook);
    assert!(text.starts_with("#!/bin/bash\n"), "{text}");
    assert!(text.ends_with("\n/libexec/zbm shadow\nexit 0\n"), "{text}");

    // The Rust runtime makes these decisions itself: nothing to compare.
    shadow_settings.push(("zfsbootmenu_runtime", "rust"));
    let rust = dracut_install(&mut parity, &shadow_settings);
    assert!(!rust.exists(hook));
}
