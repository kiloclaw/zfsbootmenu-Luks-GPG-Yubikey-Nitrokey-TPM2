//! `zbm image`: the plan of an image, as the builders' shims read it, and
//! the pruning of its graphics firmware.
//!
//! - `zbm image plan --builder dracut|mkinitcpio --config FILE --stage DIR`
//!   reads the builder's settings from `FILE` (`NAME<TAB>VALUE` lines),
//!   writes the files the plan makes into `DIR`, and prints the manifest.
//! - `zbm image prune-firmware ROOT DIRS NAMES` keeps, in the firmware
//!   directories `DIRS` (separated by spaces) of the image being built at
//!   `ROOT`, only the files `NAMES` names.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use zbm_display::layout::compile::{COMPOSE, XKB};
use zbm_platform::{Clock, Files, RealFiles, SystemClock, SystemRunner};

use crate::Io;
use zbm_host::image::{Builder, Config, Entry, Host, Plan, plan, prune_firmware};

/// The `PATH` of a builder that sets none.
const DEFAULT_PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

/// The usage.
const USAGE: &str =
    "Usage: zbm image plan --builder dracut|mkinitcpio --config <file> --stage <dir>
       zbm image prune-firmware <image root> <directories> <files>";

/// What `zbm image plan` is asked.
#[derive(Debug, Eq, PartialEq)]
struct Request {
    builder: Builder,
    config: PathBuf,
    stage: PathBuf,
}

/// The request of `args`, or `None` when they are wrong.
fn parse(args: &[String]) -> Option<Request> {
    let (mut builder, mut config, mut stage) = (None, None, None);
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let value = args.next()?;
        match arg.as_str() {
            "--builder" => builder = Some(Builder::parse(value)?),
            "--config" => config = Some(PathBuf::from(value)),
            "--stage" => stage = Some(PathBuf::from(value)),
            _ => return None,
        }
    }
    Some(Request {
        builder: builder?,
        config: config?,
        stage: stage?,
    })
}

/// Where the shim finds the file the plan writes as entry `index` at
/// `dest`, in the stage directory `stage`.
fn staged(stage: &Path, index: usize, dest: &Path) -> PathBuf {
    let name = dest.file_name().unwrap_or_default().to_string_lossy();
    stage.join(format!("{index:04}-{name}"))
}

/// Writes the files of `plan` into `stage`.
///
/// # Errors
///
/// When one cannot be written.
fn stage_files(files: &dyn Files, plan: &Plan, stage: &Path) -> std::io::Result<()> {
    files.create_dir_all(stage)?;
    for (index, entry) in plan.entries().iter().enumerate() {
        if let Entry::Write {
            dest,
            mode,
            contents,
        } = entry
        {
            let path = staged(stage, index, dest);
            files.write(&path, contents)?;
            files.set_mode(&path, *mode)?;
        }
    }
    Ok(())
}

/// The seconds since 1970.
fn now() -> i64 {
    SystemClock::new()
        .wall()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
        })
}

/// `zbm image plan`.
fn run_plan(request: &Request, files: &dyn Files, io: &mut Io) -> i32 {
    let text = match files.read_to_string(&request.config) {
        Ok(text) => text,
        Err(error) => {
            let _ = writeln!(
                io.stderr,
                "zbm image: {}: {error}",
                request.config.display()
            );
            return 1;
        }
    };
    let config = Config::parse(&text);
    let path = config.or("PATH", DEFAULT_PATH);
    let host = Host {
        files,
        runner: &SystemRunner,
        path: path
            .split(':')
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from)
            .collect(),
        scratch: PathBuf::from(format!("/tmp/zbmgpg.{}", std::process::id())),
        xkb: PathBuf::from(XKB),
        compose: PathBuf::from(COMPOSE),
        now: now(),
    };
    let plan = match plan(request.builder, &config, &host) {
        Ok(plan) => plan,
        Err(fatal) => {
            let _ = writeln!(io.stderr, "zbm image: {fatal}");
            return 1;
        }
    };
    if let Err(error) = stage_files(files, &plan, &request.stage) {
        let _ = writeln!(io.stderr, "zbm image: {}: {error}", request.stage.display());
        return 1;
    }
    let stage = request.stage.clone();
    let _ = write!(
        io.stdout,
        "{}",
        plan.manifest(&|index, dest| staged(&stage, index, dest))
    );
    0
}

/// `zbm image prune-firmware`.
fn run_prune(files: &dyn Files, args: &[String], io: &mut Io) -> i32 {
    let [root, dirs, names] = args else {
        let _ = writeln!(io.stderr, "{USAGE}");
        return crate::USAGE;
    };
    let words = |text: &str| {
        text.split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    match prune_firmware(files, Path::new(root), &words(dirs), &words(names)) {
        Ok(pruned) => {
            if !pruned.missing.is_empty() {
                let _ = writeln!(
                    io.stderr,
                    "graphics firmware not found, left out: {}",
                    pruned.missing.join(" ")
                );
            }
            let _ = writeln!(
                io.stdout,
                "kept {} firmware files, removed {}",
                pruned.kept, pruned.removed
            );
            0
        }
        Err(fatal) => {
            let _ = writeln!(io.stderr, "{fatal}");
            1
        }
    }
}

/// `zbm image`, on the build host's files.
pub fn run(args: &[String], io: &mut Io) -> i32 {
    run_with(&RealFiles, args, io)
}

/// `zbm image` on `files`.
fn run_with(files: &dyn Files, args: &[String], io: &mut Io) -> i32 {
    match args.split_first() {
        Some((command, rest)) if command == "plan" => {
            let Some(request) = parse(rest) else {
                let _ = writeln!(io.stderr, "{USAGE}");
                return crate::USAGE;
            };
            run_plan(&request, files, io)
        }
        Some((command, rest)) if command == "prune-firmware" => run_prune(files, rest, io),
        _ => {
            let _ = writeln!(io.stderr, "{USAGE}");
            crate::USAGE
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use zbm_testkit::fakes::FakeFiles;

    use super::*;

    fn call(files: &dyn Files, args: &[&str]) -> (i32, String, String) {
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let code = run_with(
            files,
            &args,
            &mut Io {
                stdout: &mut stdout,
                stderr: &mut stderr,
            },
        );
        (
            code,
            String::from_utf8(stdout).unwrap(),
            String::from_utf8(stderr).unwrap(),
        )
    }

    #[test]
    fn requests_are_read_and_wrong_ones_get_the_usage() {
        let args = |args: &[&str]| args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            parse(&args(&[
                "--builder",
                "dracut",
                "--config",
                "/c",
                "--stage",
                "/s"
            ])),
            Some(Request {
                builder: Builder::Dracut,
                config: "/c".into(),
                stage: "/s".into(),
            })
        );
        assert_eq!(
            parse(&args(&[
                "--builder",
                "make",
                "--config",
                "/c",
                "--stage",
                "/s"
            ])),
            None
        );
        assert_eq!(parse(&args(&["--builder", "dracut", "--config"])), None);
        assert_eq!(parse(&args(&["--other", "x"])), None);
        assert_eq!(parse(&args(&["--builder", "dracut"])), None);
        let files = FakeFiles::new();
        assert_eq!(call(&files, &[]).0, crate::USAGE);
        assert_eq!(call(&files, &["plan", "--builder"]).0, crate::USAGE);
        assert_eq!(call(&files, &["prune-firmware", "/x"]).0, crate::USAGE);
        let (code, _, stderr) = call(
            &files,
            &[
                "plan",
                "--builder",
                "dracut",
                "--config",
                "/none",
                "--stage",
                "/s",
            ],
        );
        assert_eq!(code, 1);
        assert!(stderr.contains("/none"), "{stderr}");
    }

    #[test]
    fn a_wrong_runtime_fails_the_plan() {
        let files = FakeFiles::new();
        files.file("/c", "zfsbootmenu_runtime\tperl\n");
        let (code, _, stderr) = call(
            &files,
            &[
                "plan",
                "--builder",
                "dracut",
                "--config",
                "/c",
                "--stage",
                "/s",
            ],
        );
        assert_eq!(code, 1);
        assert!(stderr.contains("unknown runtime"), "{stderr}");
    }

    #[test]
    fn firmware_is_pruned_and_the_count_said() {
        let files = FakeFiles::new();
        files
            .file("/img/usr/lib/firmware/i915/a.bin", "")
            .file("/img/usr/lib/firmware/i915/b.bin", "");
        let (code, stdout, stderr) = call(
            &files,
            &["prune-firmware", "/img", "i915", "i915/a.bin i915/c.bin"],
        );
        assert_eq!(code, 0, "{stderr}");
        assert_eq!(stdout, "kept 1 firmware files, removed 1\n");
        assert!(stderr.contains("i915/c.bin"));
        let (code, _, stderr) = call(&files, &["prune-firmware", "/img", "i915", "x"]);
        assert_eq!(code, 1);
        assert!(stderr.contains("linux-firmware"));
    }

    #[test]
    fn written_files_go_into_the_stage_and_the_manifest_names_them() {
        let files = FakeFiles::new();
        let mut plan = Plan::default();
        plan.write("/etc/zfsbootmenu/lock", 0o400, "hash\n");
        stage_files(&files, &plan, Path::new("/stage")).unwrap();
        assert_eq!(
            files.read(Path::new("/stage/0000-lock")).unwrap(),
            b"hash\n"
        );
        assert_eq!(
            files.metadata(Path::new("/stage/0000-lock")).unwrap().mode,
            0o400
        );
        assert!(now() > 0);
    }
}
