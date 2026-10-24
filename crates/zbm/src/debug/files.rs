//! `zbm debug config` and `zbm debug state`: what start-up was told and
//! what it recorded.
//!
//! - `config` prints every option of the kernel command line with the value
//!   in force and where it came from: the command line start-up saved
//!   (`zbm.cmdline`, under the option's name or an alias), or the default,
//!   with the warnings the options gave.
//! - `state` prints the files of the state directory: their names and
//!   sizes, and the contents of the small text ones. It leaves out the
//!   journal and the crash reports, which have reports of their own, and the
//!   key files cached from `org.zfsbootmenu:keysource` (`.keys`), which are
//!   secrets.

use std::path::Path;

use zbm_config::args::{Args as _, ZbmArgs};
use zbm_config::options::ZbmOptions;
use zbm_config::origin::Origin;
use zbm_config::spec::cmdline::ALL;
use zbm_platform::Files;

use crate::Io;

/// The command line start-up saved in the state directory.
const SAVED: &str = "zbm.cmdline";

/// The largest file `state` shows the contents of.
const SHOWN: u64 = 4096;

/// What `state` leaves out: reported elsewhere, or secret.
fn is_left_out(name: &str) -> bool {
    name == super::journal::JOURNAL
        || name == ".keys"
        || (name.starts_with("crash-") && name.ends_with(".txt"))
}

/// The lines of `zbm debug config` for the command line `kcl`.
pub fn config_lines(kcl: &str) -> Vec<String> {
    let args = ZbmArgs::from_kcl(kcl);
    let (_, origins, messages) = ZbmOptions::read(&args);
    let mut lines = Vec::new();
    for spec in ALL {
        let (value, origin) = match (origins.of(spec), args.find(spec)) {
            (Some(Origin::Invalid { name, value }), _) => (
                spec.default.unwrap_or("").to_owned(),
                format!("default: {name}={value} is not valid"),
            ),
            (_, Some(found)) => {
                let name = found.name;
                let value = found.value;
                let shown = if value.is_empty() {
                    "(set)".to_owned()
                } else {
                    value
                };
                let origin = if name == spec.name {
                    "command line".to_owned()
                } else {
                    format!("command line, as {name}")
                };
                (shown, origin)
            }
            _ => (
                spec.default.unwrap_or("(unset)").to_owned(),
                "default".to_owned(),
            ),
        };
        lines.push(format!("{:<26} {:<24} {origin}", spec.name, value));
    }
    for message in messages {
        lines.push(message.stderr_line());
    }
    lines
}

/// `zbm debug config`.
pub fn config(files: &dyn Files, base: &Path, io: &mut Io) -> i32 {
    let Ok(kcl) = files.read_to_string(&base.join(SAVED)) else {
        let _ = writeln!(
            io.stderr,
            "zbm debug: no saved command line in {}",
            base.display()
        );
        return 1;
    };
    let words: Vec<&str> = kcl.lines().filter(|line| !line.is_empty()).collect();
    let _ = writeln!(io.stdout, "command line: {}", words.join(" "));
    for line in config_lines(&kcl) {
        let _ = writeln!(io.stdout, "{line}");
    }
    0
}

/// The files below `dir`, by path relative to `base`, depth first, but
/// those [`is_left_out`].
fn walk(files: &dyn Files, base: &Path, dir: &Path, found: &mut Vec<String>) {
    let mut names: Vec<String> = files
        .read_dir(dir)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|name| name.into_string().ok())
        .filter(|name| !is_left_out(name))
        .collect();
    names.sort();
    for name in names {
        let path = dir.join(&name);
        if files.is_dir(&path) {
            walk(files, base, &path, found);
        } else if let Ok(relative) = path.strip_prefix(base) {
            found.push(relative.to_string_lossy().into_owned());
        }
    }
}

/// `zbm debug state`.
pub fn state(files: &dyn Files, base: &Path, io: &mut Io) -> i32 {
    let mut found = Vec::new();
    walk(files, base, base, &mut found);
    if found.is_empty() {
        let _ = writeln!(io.stderr, "zbm debug: nothing in {}", base.display());
        return 1;
    }
    for name in found {
        let path = base.join(&name);
        let size = files.metadata(&path).map_or(0, |metadata| metadata.len);
        let _ = writeln!(io.stdout, "== {name} ({size} bytes)");
        let text = (size <= SHOWN)
            .then(|| files.read_to_string(&path).ok())
            .flatten();
        if let Some(text) = text.filter(|text| !text.is_empty()) {
            let _ = write!(io.stdout, "{text}");
            if !text.ends_with('\n') {
                let _ = writeln!(io.stdout);
            }
        }
    }
    0
}

#[cfg(test)]
mod tests;
