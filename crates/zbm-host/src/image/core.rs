//! The runtime and ZFSBootMenu's own files: `zbm` and brush, the shell
//! libraries and tools of the module root, the legacy help pages, the
//! release file and the hooks.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use zbm_types::LINKED;

use super::{Entry, Fatal, Image, library};

/// What a Rust image takes of the module root's `lib`, `libexec` and `bin`
/// besides the library, which it generates ([`library`]): the line reader
/// that three functions of the library still run as the legacy runtime did
/// (ADR 0018, point 22), until SB7. The rest is the legacy runtime's, and
/// stays out: its start-up programs, menu and tools, which `zbm` and its
/// links are, the `fzf` menu's library and screens, the completions, and
/// the LUKS libraries: `zbm` unlocks and closes the volumes itself.
const RUST_TAKES: [&str; 1] = ["libexec/zfsbootmenu-input"];

/// `/bin/zbm` of the Rust runtime: the menu, or with a command (`zbm debug
/// crash`, `zbm report`) that command of `/libexec/zbm`, which lists them;
/// `/libexec/zbm`'s own options too (`--help`, `--list`, `--version`), and
/// any other option the menu's.
const BIN_ZBM: &str = r#"#!/bin/sh
case "${1:-}" in
  -h|--help|--list|-V|--version) exec /libexec/zbm "$@" ;;
  ""|-*) exec /libexec/zbm zfsbootmenu "$@" ;;
  *) exec /libexec/zbm "$@" ;;
esac
"#;

/// `/libexec/zfsbootmenu-run-hooks` of the Rust runtime, which the legacy
/// shell code runs: the hooks of a stage with its built-in steps, so that
/// a boot by the library's legacy kernel loader closes the LUKS mappings
/// (ADR 0017).
const RUN_HOOKS: &str = r#"#!/bin/sh
exec /libexec/zbm run-hooks "$@"
"#;

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
/// (`lib`, `libexec`, `bin`), in images of the Rust runtime only those it
/// takes (`RUST_TAKES`) and the library; its links ([`LINKED`]) and
/// `/bin/zbm`; and the legacy runtime's `init.d`.
pub fn core(image: &mut Image) {
    let root = image.module_root();
    for dir in ["lib", "libexec", "bin"] {
        for name in image.host.names(&root.join(dir)) {
            let at = format!("{dir}/{name}");
            if image.is_rust() && !RUST_TAKES.contains(&at.as_str()) {
                continue;
            }
            image.install(&root.join(&at), &Path::new("/").join(&at));
        }
    }
    if image.is_rust() {
        for linked in LINKED {
            image
                .plan
                .link("/libexec/zbm", Path::new(linked.dir).join(linked.name));
        }
        library(image);
        image.plan.write("/bin/zbm", 0o755, BIN_ZBM);
        image
            .plan
            .write("/libexec/zfsbootmenu-run-hooks", 0o755, RUN_HOOKS);
        return;
    }
    for name in image.host.names(&root.join("init.d")) {
        image.install(
            &root.join("init.d").join(&name),
            &Path::new("/libexec/init.d").join(&name),
        );
    }
}

/// The library of the hook contract in a Rust image (ADR 0017, point 11):
/// the three files generated from the module root's, whose functions run
/// in `zbm zfunc`, and the module root's below [`library::LEGACY`], for
/// the functions it runs as the legacy runtime did. A file that cannot be
/// read is installed as it is.
fn library(image: &mut Image) {
    let lib = image.module_root().join("lib");
    let texts: Vec<(&str, Option<String>)> = library::FILES
        .iter()
        .filter(|file| image.host.is_file(&lib.join(file)))
        .map(|file| (*file, image.host.read(&lib.join(file))))
        .collect();
    let all: Vec<&str> = texts
        .iter()
        .filter_map(|(_, text)| text.as_deref())
        .collect();
    for (file, text) in &texts {
        let src = lib.join(file);
        let dest = Path::new("/lib").join(file);
        let Some(text) = text else {
            image.plan.warn(format!(
                "unable to read {}: it is installed as it is",
                src.display()
            ));
            image.install(&src, &dest);
            continue;
        };
        image.install(&src, &Path::new(library::LEGACY).join("lib").join(file));
        image.plan.write(dest, 0o644, library::generate(text, &all));
    }
}

/// The help pages of the legacy runtime, below `/usr/share/docs`, which
/// its help viewer shows. Images of the Rust runtime have none: `zbm help`
/// shows text from the code's own tables (ADR 0018, point 17).
pub fn docs(image: &mut Image) {
    if image.is_rust() {
        return;
    }
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

/// The built-in step that closes the LUKS volumes, by stage and file name.
const LUKS_CLOSE: (&str, &str) = ("teardown.d", "80-luks-close.sh");

/// The legacy LUKS library, which the legacy hook of [`LUKS_CLOSE`]
/// sources to close the volumes.
const LUKS_LIBRARY: &str = "/lib/zfsbootmenu-luks.sh";

/// The files of the module root's `lib`, `libexec` and `bin` that the image
/// does not have, at their paths in a legacy image: in an image of the Rust
/// runtime those it neither takes nor generates, writes or links to `zbm`.
fn left_out(image: &Image) -> Vec<String> {
    let root = image.module_root();
    ["lib", "libexec", "bin"]
        .into_iter()
        .flat_map(|dir| {
            let names = image.host.names(&root.join(dir));
            names.into_iter().map(move |name| format!("/{dir}/{name}"))
        })
        .filter(|path| !image.plan.has(Path::new(path)))
        .collect()
}

/// Whether `text` names the file `path` itself: not as part of a longer
/// path or file name.
fn is_named(text: &str, path: &str) -> bool {
    let is_name = |c: char| c.is_ascii_alphanumeric() || "_-./".contains(c);
    text.match_indices(path).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + path.len()..].chars().next();
        !before.is_some_and(is_name) && !after.is_some_and(is_name)
    })
}

/// What the build says of the hook at `dest` that names the files `named`,
/// which the image does not have.
fn left_out_warning(dest: &Path, named: &[&str]) -> String {
    let list = named.join(", ");
    let luks_close = Path::new("/libexec/hooks")
        .join(LUKS_CLOSE.0)
        .join(LUKS_CLOSE.1);
    if dest == luks_close && named.contains(&LUKS_LIBRARY) {
        return format!(
            "hook {} takes the place of the built-in step that closes the LUKS \
             volumes, but closes none: images of the Rust runtime do not have \
             {list}, so the volumes stay open across kexec; delete it, or make \
             it run `zluks close`",
            dest.display()
        );
    }
    format!(
        "hook {} names {list}, which images of the Rust runtime do not have",
        dest.display()
    )
}

/// Whether `bytes` are those of a program rather than of a script: an ELF
/// file, or one with a NUL byte, which text does not have.
fn is_program(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x7fELF") || bytes.contains(&0)
}

/// Warns of each hook in an image of the Rust runtime that names a file of
/// the legacy runtime the image does not have ([`left_out`]), whichever
/// directory or option it came from. A hook that cannot be read, or is a
/// program, is not looked at; a script is read whatever its encoding.
fn warn_of_left_out(image: &mut Image) {
    if !image.is_rust() {
        return;
    }
    let left = left_out(image);
    let hooks: Vec<(PathBuf, PathBuf)> = image
        .plan
        .entries()
        .iter()
        .filter_map(|entry| match entry {
            Entry::File { src, dest, .. } if dest.starts_with("/libexec/hooks") => {
                Some((src.clone(), dest.clone()))
            }
            _ => None,
        })
        .collect();
    for (src, dest) in hooks {
        let bytes = image.host.files.read(&src).unwrap_or_default();
        if is_program(&bytes) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        let named: Vec<&str> = left
            .iter()
            .map(String::as_str)
            .filter(|path| is_named(&text, path))
            .collect();
        if !named.is_empty() {
            image.plan.warn(left_out_warning(&dest, &named));
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

/// Whether the hook `name` of `stage` is one the Rust runtime runs as a
/// built-in step (ADR 0017).
fn is_built_in(stage: &str, name: &str) -> bool {
    zbm_types::BUILT_IN_HOOKS
        .iter()
        .any(|(built_in, file)| built_in.as_str() == stage && *file == name)
}

/// The hooks: ZFSBootMenu's own first, so that the user's replace them,
/// but in images of the Rust runtime those it runs as built-in steps; the
/// user's of the deprecated options (`zfsbootmenu_early_setup`, `_setup`,
/// `_teardown`), then those of `zfsbootmenu_hook_root`. In images of the
/// Rust runtime, the build warns of the hooks that name a file of the
/// legacy runtime the image does not have.
pub fn hooks(image: &mut Image) {
    let root = image.module_root().join("hooks");
    for stage in STAGES {
        let dir = root.join(stage);
        if image.host.is_dir(&dir) {
            let mut files = listed(image, &dir);
            if image.is_rust() {
                files.retain(|file| {
                    let name = file.file_name().unwrap_or_default().to_string_lossy();
                    !is_built_in(stage, &name)
                });
            }
            hooks_of(image, stage, &files);
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
    warn_of_left_out(image);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_hooks_are_known_by_stage_and_name() {
        assert!(is_built_in("teardown.d", "80-luks-close.sh"));
        assert!(!is_built_in("early-setup.d", "80-luks-close.sh"));
        assert!(!is_built_in("teardown.d", "80-luks-close"));
        assert!(is_built_in(LUKS_CLOSE.0, LUKS_CLOSE.1));
    }

    #[test]
    fn a_program_is_an_elf_file_or_one_with_a_nul_byte() {
        assert!(is_program(b"\x7fELF\x02\x01"));
        assert!(is_program(b"#!/bin/sh\n\0"));
        assert!(!is_program(b"#!/bin/sh\n# \xdcbersetzung\n"));
        assert!(!is_program(b""));
    }

    #[test]
    fn a_path_is_named_whole_not_as_part_of_a_longer_one() {
        let path = "/lib/zfsbootmenu-luks.sh";
        for text in [
            "source /lib/zfsbootmenu-luks.sh",
            "  /lib/zfsbootmenu-luks.sh\n)",
            "[ -r \"/lib/zfsbootmenu-luks.sh\" ]",
            "/lib/zfsbootmenu-luks.sh",
        ] {
            assert!(is_named(text, path), "{text}");
        }
        for text in [
            "source /lib/zfsbootmenu-luks.sh.orig",
            "source /usr/lib/zfsbootmenu-luks.sh",
            "source ./lib/zfsbootmenu-luks.sh",
            "source /lib/zfsbootmenu-luks-keys.sh",
            "/lib/zfsbootmenu-luks.sh-old /lib/zfsbootmenu-luks.shx",
        ] {
            assert!(!is_named(text, path), "{text}");
        }
    }

    #[test]
    fn days_are_civil_dates() {
        assert_eq!(day(0), "1970-01-01");
        assert_eq!(day(951_782_400), "2000-02-29");
        assert_eq!(day(1_793_491_200), "2026-11-01");
        assert_eq!(day(-86_400), "1969-12-31");
    }
}
