//! `zbm keyboard-layouts`: the keyboard layouts of an image, which the image
//! build compiles from the build host's XKB data for the console on the
//! display (ROADMAP.md #19, step 4), and the console loads.
//!
//! Each layout becomes one self-contained keymap, `<name>.xkb`, beside
//! `dead-keys`, the compose rules of the dead keys, and `layouts`, their
//! names in the order asked for: the first is the console's own.

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use zbm_console::serve::{Switched, request_layout, status};

use zbm_display::layout::compile::{self, COMPOSE, XKB};
use zbm_display::layout::{Layout, Source};
use zbm_platform::{Files, RealFiles};

use crate::Io;

/// Where images keep their layouts.
pub const DIRECTORY: &str = zbm_app::LAYOUTS;

pub use zbm_host::image::keymaps::{Compiled, DEAD_KEYS, INDEX, compile_layouts};

/// The file of layout `name` in `directory`.
pub fn keymap_path(directory: &Path, name: &str) -> PathBuf {
    directory.join(format!("{name}.xkb"))
}

/// The layout `name` from `directory`, with the dead keys when the
/// directory has their rules.
///
/// # Errors
///
/// When `name` is not a layout name, the image does not have it, or its
/// keymap does not load.
pub fn load(files: &dyn Files, directory: &Path, name: &str) -> Result<Layout, String> {
    source(files, directory, name)?
        .layout()
        .map_err(|error| error.to_string())
}

/// What layout `name` is made from in `directory`: its keymap, and the
/// compose rules when the directory has them.
///
/// # Errors
///
/// When `name` is not a layout name or the image does not have it.
pub fn source(files: &dyn Files, directory: &Path, name: &str) -> Result<Source, String> {
    compile::split(name).map_err(|error| error.to_string())?;
    let keymap = files
        .read(&keymap_path(directory, name))
        .map_err(|error| format!("{name}: not in this image ({error})"))?;
    Ok(Source {
        name: name.to_owned(),
        keymap,
        rules: files.read(&directory.join(DEAD_KEYS)).ok(),
    })
}

/// The names of the layouts in `directory`, the console's own first; none
/// when the image has none.
pub fn available(files: &dyn Files, directory: &Path) -> Vec<String> {
    let index = files.read(&directory.join(INDEX)).unwrap_or_default();
    String::from_utf8_lossy(&index)
        .lines()
        .filter(|name| compile::split(name).is_ok())
        .map(str::to_owned)
        .collect()
}

/// The layout the console starts with, from `directory`: the one `asked`
/// names, or the image's first; `None`, the US layout, when the image has
/// none. What it cannot load goes to `warn`.
pub fn first(
    files: &dyn Files,
    directory: &Path,
    asked: Option<&str>,
    warn: &mut dyn FnMut(String),
) -> Option<Layout> {
    let available = available(files, directory);
    if let Some(name) = asked {
        match load(files, directory, name) {
            Ok(layout) => return Some(layout),
            Err(error) => warn(format!("{error}; the image's first layout instead")),
        }
    }
    let name = available.first()?;
    load(files, directory, name).map_err(warn).ok()
}

/// `zbm console layout [<name>]`: names the layout the console types with
/// and the image's others, or asks the console for `name`.
pub fn run_switch(args: &[String], io: &mut Io) -> i32 {
    let files = RealFiles;
    let directory = Path::new(DIRECTORY);
    let socket = Path::new(zbm_app::console::SOCKET);
    switch(&files, directory, socket, args, io)
}

/// How long `zbm console layout` waits for the console.
const SWITCH_TIMEOUT: Duration = Duration::from_secs(3);

/// `zbm console layout` on the layouts of `directory`, asking the console
/// listening at `socket`.
fn switch(files: &dyn Files, directory: &Path, socket: &Path, args: &[String], io: &mut Io) -> i32 {
    let available = available(files, directory);
    let name = match args {
        [] => {
            let status = status(socket, SWITCH_TIMEOUT).filter(|status| status.shown);
            let current = status.map_or(
                "none, no console reads the keyboards".to_owned(),
                |status| status.layout,
            );
            let _ = writeln!(io.stdout, "keyboard: {current}");
            let _ = writeln!(io.stdout, "layouts: {}", available.join(" "));
            return 0;
        }
        [name] => name,
        _ => {
            let _ = writeln!(io.stderr, "Usage: zbm console layout [<name>]");
            return crate::USAGE;
        }
    };
    if !available.contains(name) {
        let have = available.join(" ");
        let _ = writeln!(
            io.stderr,
            "zbm console: {name}: not in this image, which has: {have}"
        );
        return 1;
    }
    let outcome = request_layout(socket, name, SWITCH_TIMEOUT);
    let failure = match outcome {
        Ok(Switched::Done(now)) if now == *name => {
            let _ = writeln!(io.stdout, "keyboard: {now}");
            return 0;
        }
        Ok(Switched::Done(now)) => format!("{name} did not load; the keyboard stays {now}"),
        Ok(Switched::NoSuchLayout) => format!("{name} did not load; the keyboard keeps its layout"),
        Ok(Switched::NoConsole) => "no console on the display reads the keyboards".to_owned(),
        Ok(Switched::Late) => "the console did not answer".to_owned(),
        Err(error) => error.to_string(),
    };
    let _ = writeln!(io.stderr, "zbm console: {failure}");
    1
}

/// What `keyboard-layouts` was asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Request {
    xkb: PathBuf,
    compose: PathBuf,
    destination: PathBuf,
    layouts: Vec<String>,
}

/// The request of `args`, or `None` when they are wrong.
fn parse(args: &[String]) -> Option<Request> {
    let mut xkb = PathBuf::from(XKB);
    let mut compose = PathBuf::from(COMPOSE);
    let mut rest = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--xkb" => xkb = PathBuf::from(args.next()?),
            "--compose" => compose = PathBuf::from(args.next()?),
            option if option.starts_with("--") => return None,
            _ => rest.push(arg.clone()),
        }
    }
    let (destination, layouts) = rest.split_first()?;
    (!layouts.is_empty()).then(|| Request {
        xkb,
        compose,
        destination: PathBuf::from(destination),
        layouts: layouts.to_vec(),
    })
}

/// Compiles the layouts and the dead keys of `request`, saying what became
/// of each; returns whether every layout was compiled.
fn compile_all(files: &dyn Files, request: &Request, io: &mut Io) -> io::Result<bool> {
    files.create_dir_all(&request.destination)?;
    let compiled = compile_layouts(files, &request.xkb, &request.compose, &request.layouts);
    for (name, contents) in &compiled.files {
        files.write(&request.destination.join(name), contents)?;
    }
    for line in &compiled.done {
        let _ = writeln!(io.stdout, "{line}");
    }
    for line in compiled.failed.iter().chain(&compiled.no_dead_keys) {
        let _ = writeln!(io.stderr, "keyboard-layouts: {line}");
    }
    Ok(compiled.failed.is_empty())
}

/// `zbm keyboard-layouts [--xkb <dir>] [--compose <file>] <destination>
/// <layout>...`.
pub fn run(args: &[String], io: &mut Io) -> i32 {
    let Some(request) = parse(args) else {
        let _ = io
            .stderr
            .write_all(crate::table::KEYBOARD_LAYOUTS.usage.as_bytes());
        return crate::USAGE;
    };
    match compile_all(&RealFiles, &request, io) {
        Ok(true) => 0,
        Ok(false) => 1,
        Err(error) => {
            let _ = writeln!(io.stderr, "keyboard-layouts: {error}");
            1
        }
    }
}

#[cfg(test)]
mod tests;
