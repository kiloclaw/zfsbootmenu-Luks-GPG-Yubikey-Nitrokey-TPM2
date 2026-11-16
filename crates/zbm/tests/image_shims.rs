//! Tests of the builders' shims (ROADMAP.md SB8, ADR 0006): the dracut
//! module (`packaging/dracut/module-setup.sh`) and the mkinitcpio hook
//! (`packaging/initcpio/install/zfsbootmenu`), run by Bash with stubs for
//! the builders' functions, install what `zbm image plan` plans. The build
//! host is this one, with stub programs, which answer nothing, for those it
//! lacks (`zfs`, `fzf` and the like), last on `PATH`.

#![allow(
    clippy::disallowed_methods,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "integration tests run the shims with Bash and fail loudly when they cannot"
)]

use std::path::{Path, PathBuf};

use zbm_platform::{Command, Input, Runner, Stream, SystemRunner};
use zbm_testkit::TempDir;

/// The checkout.
fn checkout() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The programs of the stub host: those every image needs, and a few more.
const PROGRAMS: &[&str] = &[
    "bash",
    "zfs",
    "zpool",
    "zdb",
    "lsblk",
    "hostid",
    "mount",
    "mount.zfs",
    "kexec",
    "mkdir",
    "tput",
    "head",
    "mktemp",
    "sort",
    "sed",
    "grep",
    "tail",
    "tac",
    "blkid",
    "awk",
    "ps",
    "env",
    "chmod",
    "chroot",
    "od",
    "stty",
    "insmod",
    "modinfo",
    "lsmod",
    "depmod",
    "dmesg",
    "fzf",
    "setsid",
    "cat",
    "column",
    "less",
    "ldconfig",
    "ldd",
];

/// Stubs for dracut's functions: each records its call in `$LOG`, and
/// those that install files copy them into `$initdir`.
const DRACUT: &str = r#"
log() { echo "$*" >> "$LOG"; }
inst_simple() {
  log "inst_simple $*"; mkdir -p "$(dirname "${initdir}$2")" && cp "$1" "${initdir}$2"
}
inst_binary() { log "inst_binary $*"; }
inst_library() { log "inst_library $*"; }
inst_rules() { log "inst_rules $*"; }
inst_hook() { log "inst_hook $1 $2 ${3##*/}"; }
instmods() { log "instmods $* (hostonly=${hostonly}; omit=${omit_drivers})"; }
mark_hostonly() { log "mark_hostonly $*"; }
dinfo() { log "dinfo: $*"; }
dwarning() { log "dwarning: $*"; }
dfatal() { log "dfatal: $*"; }
"#;

/// Stubs for mkinitcpio's functions; messages are formats, as there, and
/// `funcgrep` is mkinitcpio's own.
const MKINITCPIO: &str = r#"
log() { echo "$*" >> "$LOG"; }
add_file() {
  log "add_file $*"; mkdir -p "$(dirname "${BUILDROOT}$2")" && cp "$1" "${BUILDROOT}$2"
}
add_dir() { log "add_dir $*"; }
add_symlink() { log "add_symlink $*"; }
add_binary() { log "add_binary $*"; }
add_module() { log "add_module $*"; }
add_udev_rule() { log "add_udev_rule $*"; }
add_runscript() { log "add_runscript"; }
msg2() { local mesg="$1"; shift; log "msg2: $(printf "${mesg}" "$@")"; }
warning() { local mesg="$1"; shift; log "warning: $(printf "${mesg}" "$@")"; }
error() { local mesg="$1"; shift; log "error: $(printf "${mesg}" "$@")"; }
funcgrep() {
  awk -v funcmatch="$1" '
    /^[[:space:]]*[[:alnum:]_]+[[:space:]]*\([[:space:]]*\)/ {
      match($1, funcmatch)
      print substr($1, RSTART, RLENGTH)
    }' "$2"
}
"#;

/// The mkinitcpio hook.
const INITCPIO: &str = "packaging/initcpio/install/zfsbootmenu";

/// A build: the stub host, the image's root and the log of the calls.
struct Build {
    dir: TempDir,
}

impl Build {
    fn new() -> Build {
        let dir = TempDir::new("zbm-shim");
        for program in PROGRAMS {
            dir.write_executable(format!("bin/{program}"), "#!/bin/sh\nexit 0\n");
        }
        std::fs::create_dir_all(dir.join("image")).unwrap();
        Build { dir }
    }

    /// Runs `script` after the shim `shim` and the stubs, with `settings`
    /// as shell code; the exit status and the log.
    fn run(&self, stubs: &str, shim: &str, settings: &str, script: &str) -> (bool, String) {
        let bin = self.dir.join("bin");
        let program = format!(
            "{stubs}\nsource {shim}\n{settings}\n{script}\n",
            shim = checkout().join(shim).display(),
        );
        let command = Command::new("bash")
            .arg("-c")
            .arg(program)
            .stdin(Input::Null)
            .stderr(Stream::Capture)
            .env("PATH", format!("/usr/bin:/bin:{}", bin.display()))
            .env("LOG", self.dir.join("log").display().to_string())
            .env("initdir", self.dir.join("image").display().to_string())
            .env("BUILDROOT", self.dir.join("image").display().to_string())
            .env("DRACUT_TMPDIR", self.dir.path().display().to_string())
            .env("KERNELVERSION", "none")
            .env("LC_ALL", "C");
        let output = SystemRunner.run(&command).unwrap();
        let log = std::fs::read_to_string(self.dir.join("log")).unwrap_or_default();
        (
            output.status.is_success(),
            format!("{log}{}", output.stderr_text()),
        )
    }

    /// The text of the image's file `path`.
    fn read(&self, path: &str) -> String {
        std::fs::read_to_string(self.dir.join("image").join(path)).unwrap_or_default()
    }
}

/// The settings every build of the tests has.
fn settings(extra: &str) -> String {
    format!(
        "zfsbootmenu_zbm={zbm}\nzfsbootmenu_module_root={root}\nzfsbootmenu_luks=no\n\
         zfsbootmenu_skip_gcc_s=yes\n{extra}",
        zbm = env!("CARGO_BIN_EXE_zbm"),
        root = checkout().join("legacy/zfsbootmenu").display(),
    )
}

#[test]
fn the_dracut_module_installs_the_plan() {
    let build = Build::new();
    let moddir = checkout().join("packaging/dracut");
    let extra = format!(
        "zfsbootmenu_runtime=rust\nzfsbootmenu_brush=/none/brush\n\
         zfsbootmenu_teardown=( /none/a /none/b )\nmoddir={}\n\
         omit_drivers='^drm$|^i915$|^nouveau$'\nzfsbootmenu_graphics_drivers=i915\n",
        moddir.display()
    );
    let (ok, log) = build.run(
        DRACUT,
        "packaging/dracut/module-setup.sh",
        &settings(&extra),
        "install && installkernel",
    );

    assert!(ok, "{log}");
    let zbm = env!("CARGO_BIN_EXE_zbm");
    assert!(
        log.contains(&format!("inst_binary {zbm} /libexec/zbm\n")),
        "{log}"
    );
    assert!(log.contains("inst_rules 90-zfs.rules\n"), "{log}");
    assert!(
        log.contains("inst_hook cmdline 95 zfsbootmenu-rust-cmdline.sh\n"),
        "{log}"
    );
    assert!(
        log.contains("inst_hook pre-mount 90 zfsbootmenu-rust-preinit.sh\n"),
        "{log}"
    );
    assert!(
        log.contains("instmods -c zfs (hostonly=; omit=^drm$|^i915$|^nouveau$)\n"),
        "{log}"
    );
    assert!(
        log.contains("instmods -c i915 (hostonly=; omit=^nouveau$)\n"),
        "{log}"
    );
    assert!(
        log.contains("dwarning: [zfsbootmenu] brush not found at /none/brush;"),
        "{log}"
    );
    assert!(
        build
            .read("etc/zfsbootmenu.conf")
            .contains("export ZBM_RUNTIME=\"rust\"\n")
    );
    assert!(build.read("bin/zbm").starts_with("#!/bin/sh\n"));
    assert_eq!(build.read("etc/zfsbootmenu/graphics-drivers"), "i915\n");
    let link = std::fs::read_link(build.dir.join("image/bin/zluks")).unwrap();
    assert_eq!(link, Path::new("/libexec/zbm"));
    // The settings reach zbm as they are: an array's elements, one each.
    let settings = std::fs::read_dir(build.dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("zbm-plan.")
        })
        .map(|dir| std::fs::read_to_string(dir.join("settings")).unwrap())
        .unwrap();
    assert!(
        settings.contains("zfsbootmenu_teardown[]\t/none/a\nzfsbootmenu_teardown[]\t/none/b\n")
    );
    assert!(
        settings.contains("zfsbootmenu_runtime\trust\n"),
        "{settings}"
    );
}

#[test]
fn the_dracut_module_stops_when_the_plan_fails() {
    let build = Build::new();
    let (ok, log) = build.run(
        DRACUT,
        "packaging/dracut/module-setup.sh",
        &settings("zfsbootmenu_runtime=perl\n"),
        "install",
    );
    assert!(!ok);
    assert!(log.contains("unknown runtime"), "{log}");
    assert!(
        log.contains("dfatal: Unable to plan the ZFSBootMenu image"),
        "{log}"
    );
    let (ok, log) = build.run(
        DRACUT,
        "packaging/dracut/module-setup.sh",
        "zfsbootmenu_zbm=/nonexistent/zbm\n",
        "install",
    );
    assert!(!ok);
    assert!(
        log.contains("dfatal: zbm, which plans every ZFSBootMenu image, is not at"),
        "{log}"
    );
}

#[test]
fn the_mkinitcpio_hook_installs_the_plan() {
    let build = Build::new();
    let (ok, log) = build.run(
        MKINITCPIO,
        INITCPIO,
        &settings("zfsbootmenu_runtime=rust\n"),
        "build",
    );

    assert!(ok, "{log}");
    let zbm = env!("CARGO_BIN_EXE_zbm");
    assert!(
        log.contains(&format!("add_binary {zbm} /libexec/zbm\n")),
        "{log}"
    );
    assert!(log.contains("add_module zfs\n"), "{log}");
    assert!(log.contains("add_udev_rule 90-zfs.rules\n"), "{log}");
    assert!(
        log.contains("add_symlink /bin/zluks /libexec/zbm\n"),
        "{log}"
    );
    assert!(log.ends_with("add_runscript\n"), "{log}");
    // The Rust runtime's run hook starts zbm itself, without the entry point.
    assert!(
        !build
            .dir
            .join("image/libexec/zfsbootmenu-initcpio")
            .exists()
    );
    assert!(
        build
            .read("etc/zfsbootmenu.conf")
            .contains("ZBM_BUILDSTYLE=\"mkinitcpio\"")
    );

    let legacy = Build::new();
    let (ok, log) = legacy.run(MKINITCPIO, INITCPIO, &settings(""), "build");
    assert!(ok, "{log}");
    assert!(
        legacy
            .read("libexec/zfsbootmenu-initcpio")
            .starts_with("#!/bin/bash\n")
    );
}

/// A script that builds the mkinitcpio hook `times` times, under
/// mkinitcpio's ERR trap, sourcing the hook before each build as
/// mkinitcpio does for each entry of its hook list.
fn initcpio_builds(times: usize) -> String {
    let shim = checkout().join(INITCPIO);
    let build = format!("unset -f build; source {}; build\n", shim.display());
    format!(
        "set -E\ntrap 'log \"ERR: $BASH_COMMAND\"' ERR\n{}",
        build.repeat(times)
    )
}

/// The settings of a Rust image whose hook list is `hooks`.
fn initcpio_settings(hooks: &str, extra: &str) -> String {
    settings(&format!(
        "zfsbootmenu_runtime=rust\n_hooks=({hooks})\n{extra}"
    ))
}

/// The number of times the log shows the plan installed.
fn plans_installed(log: &str) -> usize {
    let zbm = env!("CARGO_BIN_EXE_zbm");
    log.matches(&format!("add_binary {zbm} /libexec/zbm\n"))
        .count()
}

#[test]
fn a_hook_listed_after_zfsbootmenu_runs_before_it() {
    let build = Build::new();
    let (ok, log) = build.run(
        MKINITCPIO,
        INITCPIO,
        &initcpio_settings("base udev zfsbootmenu net zfsbootmenu", ""),
        &initcpio_builds(2),
    );

    assert!(ok, "{log}");
    // The first entry is skipped; the last one installs the plan and the
    // run hook, so init runs net first.
    assert!(
        log.starts_with(
            "msg2: zfsbootmenu is listed again; it is installed at its last entry, \
             after every other hook\n"
        ),
        "{log}"
    );
    assert_eq!(plans_installed(&log), 1, "{log}");
    assert_eq!(log.matches("add_runscript\n").count(), 1, "{log}");
    assert!(log.ends_with("add_runscript\n"), "{log}");
    assert!(!log.contains("ERR: "), "{log}");
    assert!(!log.contains("never run"), "{log}");
}

#[test]
fn a_single_entry_installs_as_before() {
    let build = Build::new();
    let (ok, log) = build.run(
        MKINITCPIO,
        INITCPIO,
        &initcpio_settings("udev net zfsbootmenu", ""),
        &initcpio_builds(1),
    );

    assert!(ok, "{log}");
    assert_eq!(plans_installed(&log), 1, "{log}");
    assert!(log.ends_with("add_runscript\n"), "{log}");
    assert!(!log.contains("listed again"), "{log}");
    assert!(!log.contains("ERR: "), "{log}");
    assert!(!log.contains("never run"), "{log}");
}

#[test]
fn a_run_hook_after_the_last_entry_is_warned_of() {
    let build = Build::new();
    // mkinitcpio's run scripts: net and dropbear have run hooks, early
    // has only an early hook, and autodetect has no run script.
    build
        .dir
        .write_executable("hooks/net", "#!/usr/bin/ash\nrun_hook() {\n  :\n}\n");
    build.dir.write_executable(
        "hooks/dropbear",
        "#!/usr/bin/ash\nrun_hook ()\n{\n  :\n}\nrun_cleanuphook () {\n  :\n}\n",
    );
    build
        .dir
        .write_executable("hooks/early", "#!/usr/bin/ash\nrun_earlyhook() {\n  :\n}\n");
    let hooks = format!("_d_hooks={}\n", build.dir.join("hooks").display());
    let (ok, log) = build.run(
        MKINITCPIO,
        INITCPIO,
        &initcpio_settings("udev zfsbootmenu net autodetect early dropbear", &hooks),
        &initcpio_builds(1),
    );

    assert!(ok, "{log}");
    assert_eq!(plans_installed(&log), 1, "{log}");
    // Only the hooks whose run hooks never run are named.
    assert!(
        log.ends_with(
            "add_runscript\nwarning: hooks after zfsbootmenu never run: net dropbear; \
             list them before it\n"
        ),
        "{log}"
    );
    assert!(!log.contains("ERR: "), "{log}");
}

#[test]
fn an_entry_count_from_the_environment_does_not_move_the_install() {
    for preset in [
        "zbm_build_root=/elsewhere zbm_build_entry=99",
        "zbm_build_root=/elsewhere zbm_build_entry=1x",
        "zbm_build_root=\"${BUILDROOT}\" zbm_build_entry=1x",
        "zbm_build_root=\"${BUILDROOT}\" zbm_build_entry=08",
        // mkinitcpio -d DIR makes $BUILDROOT known beforehand.
        "zbm_build_root=\"${BUILDROOT}\" zbm_build_entry=1",
    ] {
        let build = Build::new();
        let (ok, log) = build.run(
            MKINITCPIO,
            INITCPIO,
            &initcpio_settings("zfsbootmenu net zfsbootmenu", &format!("export {preset}\n")),
            &initcpio_builds(2),
        );

        assert!(ok, "{preset}: {log}");
        assert!(
            log.starts_with("msg2: zfsbootmenu is listed again"),
            "{preset}: {log}"
        );
        assert_eq!(plans_installed(&log), 1, "{preset}: {log}");
        assert_eq!(log.matches("add_runscript\n").count(), 1, "{preset}: {log}");
        assert!(log.ends_with("add_runscript\n"), "{preset}: {log}");
        assert!(!log.contains("ERR: "), "{preset}: {log}");
    }
}

#[test]
fn a_hard_link_to_a_file_the_image_lacks_stops_the_build() {
    let line = "hardlink /usr/bin/coreutils /usr/bin/head";
    for (stubs, shim, failed) in [
        (
            DRACUT,
            "packaging/dracut/module-setup.sh",
            "dfatal: failed to link",
        ),
        (MKINITCPIO, INITCPIO, "error: failed to link"),
    ] {
        let build = Build::new();
        let (ok, log) = build.run(stubs, shim, "", &format!("zbm_install_line {line}"));
        assert!(!ok, "{shim}: {log}");
        assert!(
            log.contains(&format!("{failed} '/usr/bin/head' to '/usr/bin/coreutils'")),
            "{shim}: {log}"
        );

        // With the file in the image first, the link is made.
        let image = build.dir.join("image/usr/bin");
        std::fs::create_dir_all(&image).unwrap();
        std::fs::write(image.join("coreutils"), "uutils").unwrap();
        let (ok, log) = build.run(stubs, shim, "", &format!("zbm_install_line {line}"));
        assert!(ok, "{shim}: {log}");
        assert_eq!(build.read("usr/bin/head"), "uutils");
    }
}
