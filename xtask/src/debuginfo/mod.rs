//! Debug files for crash reports: `split-debug` and `symbolize`.
//!
//! Release builds keep line tables and function names (`debug =
//! "line-tables-only"` in `Cargo.toml`) and a GNU build ID. `split-debug`
//! moves that information out of a binary into `<build-id>.debug`, so the
//! binary in the boot image is as small as a stripped one. `symbolize` reads
//! a crash report that the runtime wrote (`zbm debug crash` prints the latest
//! one), finds the debug file with the report's build ID, and prints each
//! frame of the crashed binary as a function, file and line, inlined calls
//! included, with `addr2line`.

mod elf;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::process::Program;

/// Where `symbolize` looks for debug files when no directory is given, and
/// where `split-debug` writes them by default.
pub const DEFAULT_DIR: &str = "target/debug-files";

/// The program that copies and strips ELF files; `OBJCOPY` overrides it.
fn objcopy() -> String {
    std::env::var("OBJCOPY").unwrap_or_else(|_| "objcopy".to_owned())
}

/// The program that maps addresses to lines; `ADDR2LINE` overrides it.
fn addr2line() -> String {
    std::env::var("ADDR2LINE").unwrap_or_else(|_| "addr2line".to_owned())
}

/// Reads a file, with the path in the error.
fn read(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|error| Error::io(format!("reading {}", path.display()), error))
}

/// Moves the debug information of `binary` to `<dir>/<build-id>.debug` and
/// strips the binary in place.
///
/// # Errors
///
/// When the binary has no build ID, or `objcopy` fails.
pub fn split(root: &Path, binary: &Path, dir: &Path) -> Result<()> {
    let id = elf::build_id(&read(binary)?)
        .ok_or_else(|| Error::Format(format!("{} has no GNU build ID", binary.display())))?;
    std::fs::create_dir_all(dir)
        .map_err(|error| Error::io(format!("creating {}", dir.display()), error))?;
    let debug = dir.join(format!("{id}.debug"));
    Program::new(&objcopy(), root)
        .args([
            "--only-keep-debug".as_ref(),
            binary.as_os_str(),
            debug.as_os_str(),
        ])
        .run()?;
    Program::new(&objcopy(), root)
        .args(["--strip-all".as_ref(), binary.as_os_str()])
        .run()?;
    println!(
        "split-debug: {} (build ID {id}): debug information in {}",
        binary.display(),
        debug.display()
    );
    Ok(())
}

/// What `symbolize` needs from a crash report.
#[derive(Debug, Default, Eq, PartialEq)]
struct Report {
    /// The lines before the frames, shown as they are.
    header: Vec<String>,
    /// The crashed binary.
    binary: String,
    /// Its build ID.
    build_id: Option<String>,
    /// The frames: number, offset, module.
    frames: Vec<(usize, u64, String)>,
}

/// Reads the crash report `text` (the format of `zbm_trace::crash::Report`).
fn parse_report(text: &str) -> Result<Report> {
    let mut lines = text.lines();
    if lines.next() != Some("ZFSBootMenu crash report") {
        return Err(Error::Format(
            "this is not a ZFSBootMenu crash report".to_owned(),
        ));
    }
    let mut report = Report {
        header: vec!["ZFSBootMenu crash report".to_owned()],
        ..Report::default()
    };
    for line in lines {
        if let Some(frame) = line.trim_start().strip_prefix('#') {
            let mut parts = frame.splitn(3, ' ');
            let number = parts.next().and_then(|number| number.parse().ok());
            let offset = parts
                .next()
                .and_then(|offset| offset.strip_prefix("0x"))
                .and_then(|offset| u64::from_str_radix(offset, 16).ok());
            if let (Some(number), Some(offset), Some(module)) = (number, offset, parts.next()) {
                report.frames.push((number, offset, module.to_owned()));
            }
            continue;
        }
        if let Some(binary) = line.strip_prefix("binary: ") {
            binary.clone_into(&mut report.binary);
        }
        if let Some(id) = line.strip_prefix("build-id: ") {
            report.build_id = Some(id.to_owned()).filter(|id| id != "unknown");
        }
        if line != "frames:" {
            report.header.push(line.to_owned());
        }
    }
    Ok(report)
}

/// Groups the output of `addr2line -a -p` by address: each group is the
/// lines of one address, innermost function first.
fn parse_addr2line(output: &str) -> Vec<Vec<String>> {
    let mut groups: Vec<Vec<String>> = Vec::new();
    for line in output.lines() {
        if let Some((address, rest)) = line.split_once(": ")
            && address.starts_with("0x")
        {
            groups.push(vec![rest.to_owned()]);
        } else if let Some(group) = groups.last_mut() {
            group.push(line.trim().to_owned());
        }
    }
    groups
}

/// Finds the debug file for `build_id` in `dirs`.
fn find_debug_file(dirs: &[PathBuf], build_id: &str) -> Result<PathBuf> {
    dirs.iter()
        .map(|dir| dir.join(format!("{build_id}.debug")))
        .find(|path| path.is_file())
        .ok_or_else(|| {
            let searched: Vec<String> = dirs.iter().map(|dir| dir.display().to_string()).collect();
            Error::Format(format!(
                "no debug file {build_id}.debug in {}; \
                 pass --debug-dir with the build's debug files",
                searched.join(", ")
            ))
        })
}

/// Prints the crash report at `path` with the frames of its binary resolved
/// from the debug files in `dirs`.
///
/// # Errors
///
/// As [`symbolized`].
pub fn symbolize(root: &Path, path: &Path, dirs: &[PathBuf]) -> Result<()> {
    print!("{}", symbolized(root, path, dirs)?);
    Ok(())
}

/// The crash report at `path` with the frames of its binary resolved from
/// the debug files in `dirs`.
///
/// # Errors
///
/// When the report cannot be read, has no build ID, no debug file has its
/// build ID, or `addr2line` fails.
pub fn symbolized(root: &Path, path: &Path, dirs: &[PathBuf]) -> Result<String> {
    let text = String::from_utf8_lossy(&read(path)?).into_owned();
    let report = parse_report(&text)?;
    let build_id = report
        .build_id
        .as_deref()
        .ok_or_else(|| Error::Format("the report names no build ID".to_owned()))?;
    let debug = find_debug_file(dirs, build_id)?;
    let bias = elf::load_bias(&read(&debug)?)
        .ok_or_else(|| Error::Format(format!("{} is not an ELF file", debug.display())))?;
    let own: Vec<&(usize, u64, String)> = report
        .frames
        .iter()
        .filter(|(_, _, module)| *module == report.binary)
        .collect();
    // A return address is just after its call; one byte back is inside it.
    let addresses: Vec<String> = own
        .iter()
        .map(|(_, offset, _)| format!("{:#x}", offset.wrapping_add(bias).wrapping_sub(1)))
        .collect();
    let resolved = if addresses.is_empty() {
        Vec::new()
    } else {
        parse_addr2line(
            &Program::new(&addr2line(), root)
                .args(["-a", "-C", "-f", "-i", "-p", "-e"])
                .args([debug.as_os_str()])
                .args(&addresses)
                .output()?,
        )
    };
    Ok(render(&report, &resolved))
}

/// The symbolized report: the header, then each frame with its functions.
fn render(report: &Report, resolved: &[Vec<String>]) -> String {
    let mut text = String::new();
    for line in &report.header {
        let _ = writeln!(text, "{line}");
    }
    text.push_str("frames:\n");
    let mut functions = resolved.iter();
    for (number, offset, module) in &report.frames {
        let _ = write!(text, "  #{number:<3} {offset:#010x} ");
        if *module != report.binary {
            let _ = writeln!(text, "in {module}");
            continue;
        }
        let lines = functions.next().map(Vec::as_slice).unwrap_or_default();
        let mut lines = lines.iter();
        let _ = writeln!(text, "{}", lines.next().map_or("??", String::as_str));
        for inlined in lines {
            let _ = writeln!(text, "         {inlined}");
        }
    }
    text
}

#[cfg(test)]
mod tests;
