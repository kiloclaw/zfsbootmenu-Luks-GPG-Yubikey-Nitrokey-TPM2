//! The runtime and ZFSBootMenu's own files: `zbm` and brush, the shell
//! libraries and tools of the module root, the help pages, the release
//! file and the hooks.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use super::{Entry, Fatal, Image};

/// What the Rust runtime is instead of the legacy runtime's files: `zbm`
/// is the start-up program, `zluks` and the menu.
const RUST_REPLACES: [&str; 4] = [
    "libexec/zfsbootmenu-init",
    "bin/zluks",
    "bin/zfsbootmenu",
    "bin/zbm",
];

/// `/bin/zbm` of the Rust runtime: the menu, or with a command (`zbm debug
/// crash`, `zbm report`) that command of `/libexec/zbm`, which lists them.
const BIN_ZBM: &str = r#"#!/bin/sh
case "${1:-}" in
  ""|-*) exec /libexec/zbm zfsbootmenu "$@" ;;
  *) exec /libexec/zbm "$@" ;;
esac
"#;

/// The teardown hook of the Rust runtime that closes the LUKS mappings.
const LUKS_CLOSE: &str = r"#!/bin/bash
## Close the LUKS mappings that ZFSBootMenu opened to import ZFS pools, with
## zluks: the pools on them are exported first, and closing them makes the
## kernel wipe the dm-crypt volume keys before the new kernel starts.
##
## This hook may be disabled with zbm.skip_hooks=80-luks-close.sh

command -v cryptsetup >/dev/null 2>&1 || exit 0
/bin/zluks close
exit 0
";

/// The early-setup hook of the Rust runtime that sizes the console font.
const CONSOLE_AUTOSIZE: &str = r"#!/bin/bash
## Size the console font to the display (zbm.autosize), with zbm: from the
## display's physical size where its EDID gives it, so that text is readable
## on high-density (HiDPI) and ordinary screens alike.
##
## This hook may be disabled with zbm.skip_hooks=30-console-autosize.sh

exec /libexec/zbm console-autosize
";

/// The stages of the hooks, in order.
pub const STAGES: [&str; 5] = [
    "early-setup.d",
    "setup.d",
    "load-key.d",
    "boot-sel.d",
    "teardown.d",
];

/// Installs `zbm` at `/libexec/zbm`, and brush, the Rust runtime's shell,
/// at `/usr/bin/brush` (`zfsbootmenu_brush`, by default beside `zbm`);
/// without brush the Rust runtime's shell is Bash. The legacy runtime does
/// not need `zbm`, but has it when the host does.
///
/// # Errors
///
/// When the Rust runtime has no `zbm`.
pub fn runtime(image: &mut Image) -> Result<(), Fatal> {
    let zbm = PathBuf::from(image.config.zbm());
    if !image.host.is_runnable(&zbm) {
        if image.is_rust() {
            return Err(format!(
                "zfsbootmenu_runtime=rust: zbm not found at {}",
                zbm.display()
            ));
        }
        image.plan.info(format!(
            "zbm not found at {}; the legacy runtime does not need it",
            zbm.display()
        ));
        return Ok(());
    }
    image.plan.binary(zbm.clone(), "/libexec/zbm");
    if !image.is_rust() {
        return Ok(());
    }
    let brush = image.config.get("zfsbootmenu_brush").map_or_else(
        || zbm.parent().unwrap_or(Path::new("/")).join("brush"),
        PathBuf::from,
    );
    if image.host.is_runnable(&brush) {
        image.plan.binary(brush, "/usr/bin/brush");
    } else {
        image.plan.warn(format!(
            "brush not found at {}; the shell of the Rust runtime is Bash",
            brush.display()
        ));
    }
    Ok(())
}

/// The shell libraries, start-up programs and tools of the module root
/// (`lib`, `libexec`, `bin`); the Rust runtime's links and `/bin/zbm`
/// where it replaces them, and the legacy runtime's `init.d`.
pub fn core(image: &mut Image) {
    let root = image.module_root();
    for dir in ["lib", "libexec", "bin"] {
        for name in image.host.names(&root.join(dir)) {
            let at = format!("{dir}/{name}");
            if image.is_rust() && RUST_REPLACES.contains(&at.as_str()) {
                continue;
            }
            image.install(&root.join(&at), &Path::new("/").join(&at));
        }
    }
    if image.is_rust() {
        image.plan.link("/libexec/zbm", "/bin/zluks");
        image.plan.link("/libexec/zbm", "/bin/zfsbootmenu");
        image.plan.write("/bin/zbm", 0o755, BIN_ZBM);
        return;
    }
    for name in image.host.names(&root.join("init.d")) {
        image.install(
            &root.join("init.d").join(&name),
            &Path::new("/libexec/init.d").join(&name),
        );
    }
}

/// The help pages, below `/usr/share/docs`.
pub fn docs(image: &mut Image) {
    let root = image.module_root();
    for (path, found) in image.host.walk(&root.join("help-files")) {
        if found != super::Found::File {
            continue;
        }
        if let Ok(relative) = path.strip_prefix(&root) {
            let dest = Path::new("/usr/share/docs").join(relative);
            image.install(&path, &dest);
        }
    }
}

/// The days from 1970-01-01 to the civil date, as (year, month, day).
fn civil(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let of_era = days.rem_euclid(146_097);
    let year_of_era = (of_era - of_era / 1460 + of_era / 36_524 - of_era / 146_096) / 365;
    let of_year = of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted = (5 * of_year + 2) / 153;
    let day = of_year - (153 * shifted + 2) / 5 + 1;
    let month = if shifted < 10 {
        shifted + 3
    } else {
        shifted - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// The day of `seconds` since 1970, in UTC, as `YYYY-MM-DD`.
pub fn day(seconds: i64) -> String {
    let (year, month, day) = civil(seconds.div_euclid(86_400));
    format!("{year:04}-{month:02}-{day:02}")
}

/// The release file, `/etc/zbm-release` with the build's day as its
/// `BUILD_ID` (`SOURCE_DATE_EPOCH`'s, when set, so that reproducible
/// builds give the same file), and `/etc/os-release` a link to it.
pub fn osver(image: &mut Image) {
    let release = image.module_root().join("zbm-release");
    let Some(mut text) = image.host.read(&release) else {
        return;
    };
    let seconds = image
        .config
        .get("SOURCE_DATE_EPOCH")
        .and_then(|epoch| epoch.trim().parse().ok())
        .unwrap_or(image.host.now);
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    let _ = writeln!(text, "BUILD_ID=\"{}\"", day(seconds));
    image.plan.write("/etc/zbm-release", 0o644, text);
    image.plan.link("zbm-release", "/etc/os-release");
}

/// Installs the executable files of `files` as hooks of `stage`.
fn hooks_of(image: &mut Image, stage: &str, files: &[PathBuf]) {
    if files.is_empty() {
        return;
    }
    let dir = Path::new("/libexec/hooks").join(stage);
    image.plan.add(Entry::Dir {
        dest: dir.clone(),
        mode: 0o755,
    });
    for file in files {
        if !image.host.is_runnable(file) {
            continue;
        }
        if let Some(name) = file.file_name() {
            image.install(file, &dir.join(name));
        }
    }
}

/// The files of the directory `dir`, as the shell's `dir/*` names them.
fn listed(image: &Image, dir: &Path) -> Vec<PathBuf> {
    let names = image.host.names(dir);
    if names.is_empty() {
        // An empty directory: the pattern stays, and names nothing to run.
        return vec![dir.join("*")];
    }
    names.into_iter().map(|name| dir.join(name)).collect()
}

/// The hooks: ZFSBootMenu's own first, so that the user's replace them;
/// the Rust runtime's own LUKS teardown and font sizing; the user's of
/// the deprecated options (`zfsbootmenu_early_setup`, `_setup`,
/// `_teardown`), then those of `zfsbootmenu_hook_root`.
pub fn hooks(image: &mut Image) {
    let root = image.module_root().join("hooks");
    for stage in STAGES {
        let dir = root.join(stage);
        if image.host.is_dir(&dir) {
            let files = listed(image, &dir);
            hooks_of(image, stage, &files);
        }
    }
    let replaced = [
        ("teardown.d/80-luks-close.sh", LUKS_CLOSE),
        ("early-setup.d/30-console-autosize.sh", CONSOLE_AUTOSIZE),
    ];
    for (hook, text) in replaced {
        let dest = Path::new("/libexec/hooks").join(hook);
        if image.is_rust() && image.plan.has(&dest) {
            image.plan.write(dest, 0o755, text);
        }
    }
    for (option, stage) in [
        ("zfsbootmenu_early_setup", "early-setup.d"),
        ("zfsbootmenu_setup", "setup.d"),
        ("zfsbootmenu_teardown", "teardown.d"),
    ] {
        let files: Vec<PathBuf> = image
            .config
            .words(option)
            .into_iter()
            .map(PathBuf::from)
            .collect();
        hooks_of(image, stage, &files);
    }
    let user = PathBuf::from(image.config.hook_root());
    for stage in STAGES {
        let dir = user.join(stage);
        if image.host.is_dir(&dir) {
            let files = listed(image, &dir);
            hooks_of(image, stage, &files);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_are_civil_dates() {
        assert_eq!(day(0), "1970-01-01");
        assert_eq!(day(951_782_400), "2000-02-29");
        assert_eq!(day(1_793_491_200), "2026-11-01");
        assert_eq!(day(-86_400), "1969-12-31");
    }
}
