//! The console on the display, as the tests drive it: the image built with
//! the console's mirror, QEMU's arguments for the display and the mirror's
//! serial port, keys pressed on QEMU's keyboard through its monitor, and
//! the screens read from the mirror, which holds each whole screen the
//! console draws, after a form feed.

use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use crate::error::{Error, Result};
use crate::expect::regex;
use crate::images::ImageSpec;
use crate::record::Test;
use crate::settings::ZbmRuntime;
use crate::vm::Vm;

/// How the test waits: the guest gets `pause` to settle between steps,
/// and keys are typed 5 a `pause`; a boot may take `patience` seconds.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Pace {
    /// The pause between steps.
    pub(crate) pause: Duration,
    /// The longest wait for the boot, in seconds.
    pub(crate) patience: u64,
}

/// What the test does through QEMU's monitor: screenshots and keys.
pub(crate) struct Monitor {
    /// Writes a screenshot of the machine to a path.
    pub(crate) dump: fn(&Vm<'_>, &Path) -> Result<()>,
    /// Presses a key, by QEMU's name (`down`, `ret`).
    pub(crate) key: fn(&Vm<'_>, &str) -> Result<()>,
}

/// The keyboard layouts of the image, in its order: `us` first, which the
/// console starts with, and `de` second, which `runtime-rust-console`
/// switches to; then the others of `menu-keys`, which Super+Space reaches
/// in this order.
pub(crate) const LAYOUTS: [&str; 6] = ["us", "de", "fr", "gb", "ru", "us(dvorak)"];

/// The image of the test.
pub(crate) fn spec() -> ImageSpec {
    ImageSpec {
        runtime: Some(ZbmRuntime::Rust),
        conf: format!(
            "add_drivers+=\" virtio_gpu evdev \"\nzfsbootmenu_keyboard_layouts=\"{}\"",
            LAYOUTS.join(" ")
        ),
        features: Some("zbm/console-mirror".into()),
        ..ImageSpec::new("console")
    }
}

/// Where the mirror of the boot `name` is kept.
pub(crate) fn mirror_path(test: &Test, name: &str) -> PathBuf {
    test.dir().join(format!("mirror-{name}.txt"))
}

/// Presses `key` on the machine's keyboard for 50 milliseconds.
pub(crate) fn sendkey(vm: &Vm<'_>, key: &str) -> Result<()> {
    vm.send_key(key)
}

/// QEMU's names of the keys that type `text` on the US layout; `None` for
/// a character without one.
pub(crate) fn key_names(text: &str) -> Option<Vec<String>> {
    text.chars()
        .map(|c| {
            let name = match c {
                'a'..='z' | '0'..='9' => c.to_string(),
                'A'..='Z' => format!("shift-{}", c.to_ascii_lowercase()),
                ' ' => "spc".into(),
                '\n' => "ret".into(),
                '-' => "minus".into(),
                '_' => "shift-minus".into(),
                '=' => "equal".into(),
                '/' => "slash".into(),
                '.' => "dot".into(),
                ':' => "shift-semicolon".into(),
                '|' => "shift-backslash".into(),
                '>' => "shift-dot".into(),
                '@' => "shift-2".into(),
                '$' => "shift-4".into(),
                '(' => "shift-9".into(),
                ')' => "shift-0".into(),
                _ => return None,
            };
            Some(name)
        })
        .collect()
}

/// Presses the keys `names`, a key each `pause` / 5: QEMU holds each key a
/// while, and a key sent before the last one was let go can be lost.
pub(crate) fn press_keys(
    vm: &Vm<'_>,
    monitor: &Monitor,
    names: &[String],
    pause: Duration,
) -> Result<()> {
    for name in names {
        (monitor.key)(vm, name)?;
        thread::sleep(pause / 5);
    }
    Ok(())
}

/// Types `text` on the machine's keyboard, on the US layout.
pub(crate) fn type_text(vm: &Vm<'_>, monitor: &Monitor, text: &str, pause: Duration) -> Result<()> {
    let names = key_names(text).ok_or_else(|| Error::Setup(format!("no keys type {text:?}")))?;
    press_keys(vm, monitor, &names, pause)
}

/// The last screen the mirror at `path` holds, as drawn; empty when none.
pub(crate) fn last_drawn(path: &Path) -> String {
    let text = fs::read(path).unwrap_or_default();
    let text = String::from_utf8_lossy(&text).replace('\r', "");
    let screen = text.rsplit('\x0c').find(|screen| !screen.trim().is_empty());
    screen.unwrap_or_default().to_owned()
}

/// The last screen of the mirror at `path`, for the notes of a failure.
pub(crate) fn last_screen(path: &Path) -> String {
    let screen = last_drawn(path);
    let lines: Vec<&str> = screen
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    if lines.is_empty() {
        "(nothing)".to_owned()
    } else {
        lines.join(" | ")
    }
}

/// Waits at most `seconds` until a screen in the mirror at `path` matches
/// `pattern`; returns that screen.
pub(crate) fn wait_screen(path: &Path, pattern: &str, seconds: u64) -> Result<Option<String>> {
    let pattern = regex(pattern)?;
    let start = Instant::now();
    loop {
        let text = fs::read(path).unwrap_or_default();
        let text = String::from_utf8_lossy(&text).replace('\r', "");
        if let Some(screen) = text.split('\x0c').find(|screen| pattern.is_match(screen)) {
            return Ok(Some(screen.to_owned()));
        }
        if start.elapsed() >= Duration::from_secs(seconds) {
            return Ok(None);
        }
        thread::sleep(Duration::from_millis(200));
    }
}

/// QEMU's arguments for the display `name` and the mirror's serial port.
pub(crate) fn machine_extra(name: &str, mirror: &Path) -> Vec<String> {
    let mut extra: Vec<String> = Vec::new();
    if name == "virtio" {
        extra.extend(["-vga", "none", "-device", "virtio-vga"].map(String::from));
    }
    extra.extend([
        "-chardev".into(),
        format!("file,id=mirror,path={}", mirror.display()),
        "-serial".into(),
        "chardev:mirror".into(),
    ]);
    extra
}

#[cfg(test)]
mod tests;
