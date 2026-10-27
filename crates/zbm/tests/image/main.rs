//! Tests of the image build helpers that select the runtime and install `zbm`:
//! `install_zbm_runtime` and the `ZBM_RUNTIME` line of `create_zbm_conf` in
//! `legacy/zfsbootmenu/install-helpers.sh`, and the dracut module, run with
//! stubs for the dracut and mkinitcpio functions they call.
//!
//! The helpers are shell code until R6 of ROADMAP.md, so their runs
//! are recorded as golden files (`zbm_testkit::parity`): a normal test run
//! reads the recordings, and `cargo xtask record-parity` runs the helpers
//! again. A `zbm` program that exists is written `$ZBM` in settings
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
dwarning() { echo "dwarning: $*" >&2; }
warning() { echo "warning: $*" >&2; }
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
        let dir = dir.path().to_string_lossy().into_owned();
        let plain = |text: String| text.replace(&zbm, ZBM).replace(&dir, "$DIR");
        (
            output.status.shell_code(),
            plain(output.stdout_text()),
            plain(output.stderr_text()),
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
    // Without brush, the shell is Bash.
    let (code, stdout, stderr) = install(
        &mut parity,
        "mkinitcpio",
        &[("zfsbootmenu_runtime", "Rust"), ("zfsbootmenu_zbm", ZBM)],
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "add_binary $ZBM /libexec/zbm\nruntime=rust\n");
    assert_eq!(
        stderr,
        "warning: brush not found at $DIR/brush; the shell of the Rust runtime is Bash\n"
    );
    let brush = [("zfsbootmenu_brush", ZBM), ("zfsbootmenu_runtime", "rust")];
    let (_, stdout, _) = install(
        &mut parity,
        "dracut",
        &[("zfsbootmenu_zbm", ZBM), brush[0], brush[1]],
    );
    assert_eq!(
        stdout,
        "inst_binary $ZBM /libexec/zbm\ninst_binary $ZBM /usr/bin/brush\nruntime=rust\n"
    );
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

/// Where images keep their console fonts.
const FONTS: &str = "usr/share/zfsbootmenu/fonts";

/// The files of an image that the tests read.
const TEXTS: &[&str] = &[
    "hooks.list",
    "bin/zluks",
    "etc/zfsbootmenu.conf",
    "libexec/hooks/teardown.d/80-luks-close.sh",
    "libexec/hooks/early-setup.d/30-console-autosize.sh",
    "usr/share/zfsbootmenu/layouts/layouts",
    "etc/inputrc",
    "etc/profile",
    "root/.bashrc",
    "bin/zbm",
];

/// Texts of [`TEXTS`] kept only when they are this short: `bin/zbm` is a
/// wrapper in Rust images and the whole legacy menu in legacy ones.
const SHORT: &[&str] = &["bin/zbm"];

/// What the dracut module installed: every path, and the texts of [`TEXTS`].
#[derive(Debug, Deserialize, Serialize)]
struct Image {
    /// Each path below the image root: `dir`, `file <mode>` or `link <target>`.
    entries: BTreeMap<String, String>,
    /// The contents of the files of [`TEXTS`] that exist.
    texts: BTreeMap<String, String>,
    /// The console fonts, by name, with their numbers of glyphs.
    fonts: BTreeMap<String, usize>,
    /// What the build warned about.
    #[serde(default)]
    warnings: Vec<String>,
}

impl Image {
    /// Describes the tree under `root`, writing `zbm` as [`ZBM`].
    fn describe(root: &std::path::Path, zbm: &str, stderr: &str) -> Image {
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
                let is_short = text.len() < 1024 || !SHORT.contains(name);
                is_short.then(|| ((*name).to_owned(), text.replace(zbm, ZBM)))
            })
            .collect();
        let fonts = std::fs::read_dir(root.join(FONTS))
            .into_iter()
            .flatten()
            .map(|entry| {
                let entry = entry.unwrap();
                // Directories, such as the fallback fonts', are no font.
                let bytes = std::fs::read(entry.path()).unwrap_or_default();
                let font = zbm_types::psf::Font::parse(&bytes);
                let name = entry.file_name().to_string_lossy().into_owned();
                (name, font.map_or(0, |font| font.glyph_count()))
            })
            .collect();
        let warnings = stderr
            .lines()
            .filter_map(|line| line.strip_prefix("dwarning: "))
            .map(|line| line.replace(zbm, ZBM))
            .collect();
        Image {
            entries,
            texts,
            fonts,
            warnings,
        }
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
        // The real zbm, which makes the console fonts of Rust images.
        let real = env!("CARGO_BIN_EXE_zbm");
        let zbm = dir.write_executable("zbm", format!("#!/bin/sh\nexec {real} \"$@\"\n"));
        // brush, next to zbm, where the helpers look for it.
        dir.write_executable("brush", "#!/bin/sh\n");
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
            // Not the layout of the machine's /etc/vconsole.conf
            .env("zfsbootmenu_keyboard_layouts", "us")
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
        Image::describe(initdir.path(), &zbm, &output.stderr_text())
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
    // Legacy images keep the whole fonts.
    assert_eq!(legacy.fonts.len(), 6, "{:?}", legacy.fonts);
    assert!(
        legacy.fonts.values().all(|&glyphs| glyphs == 512),
        "{:?}",
        legacy.fonts
    );
    // The embedded command line survives the configuration file being written.
    assert!(
        conf.contains("export embedded_kcl='zbm.timeout=3'\n"),
        "{conf}"
    );

    // Only Rust images have brush.
    assert!(!legacy.exists("usr/bin/brush"));
    assert!(!legacy.text("root/.bashrc").contains("brush"));
    assert!(!legacy.exists("usr/share/zfsbootmenu/layouts"));
    assert!(!legacy.exists("etc/inputrc"));
    assert!(!legacy.text("etc/profile").contains("LC_CTYPE"));

    let mut rust_settings = settings.to_vec();
    rust_settings.push(("zfsbootmenu_runtime", "rust"));
    rust_settings.push(("zfsbootmenu_keyboard_layouts", "de,us zz"));
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
    // brush, and the SSH logins that hand over to it.
    assert_eq!(rust.entries["usr/bin/brush"], "file 755");
    let bashrc = rust.text("root/.bashrc");
    assert!(bashrc.contains("\n  exec /usr/bin/brush\n"), "{bashrc}");
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
    // Rust images get the same fonts as 256-glyph subsets, which leave the
    // console all 16 colours, and the TrueType font of the console on the
    // display, with its licence.
    let (outline, bitmap): (Vec<_>, Vec<_>) = rust
        .fonts
        .iter()
        .partition(|(name, _)| !name.ends_with(".psf"));
    let names: Vec<&String> = bitmap.iter().map(|(name, _)| *name).collect();
    assert_eq!(names, legacy.fonts.keys().collect::<Vec<_>>());
    assert!(
        bitmap.iter().all(|(_, glyphs)| **glyphs == 256),
        "{:?}",
        rust.fonts
    );
    let outline: Vec<&str> = outline.iter().map(|(name, _)| name.as_str()).collect();
    let faces = ["-Bold", "-BoldOblique", "-Oblique", ""].map(|f| format!("DejaVuSansMono{f}.ttf"));
    assert_eq!(outline[..4], faces);
    assert_eq!(outline[4..], ["LICENSE.dejavu"]);
    // The keyboard layouts of the console on the display, compiled from the
    // XKB data of the machine that runs the test; unknown ones are left out.
    let layouts = rust.text("usr/share/zfsbootmenu/layouts/layouts");
    assert_eq!(layouts, "de\nus\n");
    for file in ["de.xkb", "us.xkb", "dead-keys"] {
        let path = format!("usr/share/zfsbootmenu/layouts/{file}");
        assert_eq!(rust.entries[&path], "file 644", "{path}");
    }
    assert!(
        rust.warnings
            .iter()
            .any(|warning| warning.contains("unable to compile every keyboard layout")),
        "{:?}",
        rust.warnings
    );
    // The recovery shell's line editor keeps characters beyond ASCII, and
    // they are characters where the machine has the C.UTF-8 character set.
    assert_eq!(
        rust.text("etc/inputrc"),
        "set input-meta on\nset output-meta on\nset convert-meta off\n"
    );
    let utf8 = std::path::Path::new("/usr/lib/locale/C.utf8/LC_CTYPE").exists();
    assert_eq!(rust.exists("usr/lib/locale/C.utf8/LC_CTYPE"), utf8);
    let profile = rust.text("etc/profile");
    assert_eq!(
        profile.contains("\nexport LC_CTYPE=C.UTF-8\n"),
        utf8,
        "{profile}"
    );
    // Programs on the console keep xterm's description, if the host has it.
    let xterm = ["/etc", "/lib", "/usr/share"]
        .map(|dir| format!("{dir}/terminfo/x/xterm-256color"))
        .into_iter()
        .find(|path| std::path::Path::new(path).is_file());
    let kept = "[ \"${ZBM_CONSOLE}\" = drm ] && export TERM=xterm-256color\n";
    assert_eq!(profile.ends_with(kept), xterm.is_some(), "{profile}");
    assert!(xterm.is_none_or(|path| rust.exists(path.trim_start_matches('/'))));
    // zbm is zluks, which also closes the LUKS mappings before booting, and
    // the menu, under both of its names.
    assert_eq!(rust.entries["bin/zluks"], "link /libexec/zbm");
    assert_eq!(rust.entries["bin/zfsbootmenu"], "link /libexec/zbm");
    assert_eq!(rust.entries["bin/zbm"], "file 755");
    zbm_wrapper_runs_the_menu_or_a_command(rust.text("bin/zbm"));
    let hook = rust.text(teardown);
    assert!(hook.starts_with("#!/bin/bash\n"), "{hook}");
    assert!(hook.contains("\n/bin/zluks close\nexit 0\n"), "{hook}");
    assert!(!hook.contains("luks_close_all"), "{hook}");
    assert_eq!(rust.entries[teardown], "file 755");
}

/// `/bin/zbm` (`wrapper`) runs the menu, with its options, or the command
/// it is given (`zbm debug crash`, as the supervisor's message says).
fn zbm_wrapper_runs_the_menu_or_a_command(wrapper: &str) {
    let dir = TempDir::new("zbm-wrapper");
    let zbm = dir.write_executable("zbm", "#!/bin/sh\necho \"$@\"\n");
    let script = wrapper.replace("/libexec/zbm", &zbm.to_string_lossy());
    for (args, called) in [
        (&[][..], "zfsbootmenu"),
        (&["--countdown"], "zfsbootmenu --countdown"),
        (&["debug", "crash"], "debug crash"),
        (&["report"], "report"),
    ] {
        let output = std::process::Command::new("sh")
            .args(["-c", &script, "zbm"])
            .args(args)
            .output()
            .expect("sh runs");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            format!("{called}\n")
        );
    }
}

mod fonts;
