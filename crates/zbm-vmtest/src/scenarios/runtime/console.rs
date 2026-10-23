//! `runtime-rust-console`: the console on the display, without the
//! kernel's (`zbm.console=drm`, ROADMAP.md #19, step 2). The Rust image is
//! built with the `virtio_gpu` and `evdev` drivers, and boots twice with
//! `tty1` as its control terminal; the console writes the text of its
//! screen to a second serial port (`zbm.console_mirror=/dev/ttyS1`), which
//! the test reads as it reads the serial console.
//!
//! - On a virtio GPU: the countdown shows on the console, in colour; Escape
//!   typed on QEMU's keyboard reaches it, and the recovery shell runs on
//!   the console's pseudo-terminal. Killing the console from that shell
//!   puts the runtime back on the kernel console, whose shell takes keys
//!   again and finds the console's start and end in the kernel log.
//! - On `simpledrm` (`vga=0x318`): the countdown runs out; the console
//!   hands the display back, and the boot environment boots.

use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use crate::error::{Error, Result};
use crate::expect::regex;
use crate::images::{Image, ImageSpec, zbm_image};
use crate::qemu::Machine;
use crate::record::Test;
use crate::settings::ZbmRuntime;
use crate::vm::Vm;

use super::COUNTDOWN;
use super::colours::{screen_colours, screendump};
use super::drm::Monitor;

/// The command line of both boots, before the timeout.
const APPEND: &str = "console=ttyS0 console=tty1 loglevel=7 zbm.console=drm \
    zbm.console_mirror=/dev/ttyS1";

/// What the console writes to its mirror once it handed the display back.
const RELEASED: &str = "zbm console: handed the display back";

/// The fewest colours of the countdown: the background, the text, and the
/// boot environment and the seconds in their colours.
const COUNTDOWN_COLOURS: usize = 4;

/// Typed in the recovery shell: the shell's terminal (the image has no
/// `tty`).
const TTY: &str = "readlink /proc/self/fd/0\n";

/// Typed in the recovery shell: ends the console.
const KILL: &str = "kill -9 $(cat /zfsbootmenu/console)\n";

/// Typed on the kernel console afterwards: a line on the serial console,
/// which the test reads.
const BACK: &str = "echo zbm-back > /dev/ttyS0\n";

/// Typed on the kernel console then: the kernel log, on the serial console.
const LOG: &str = "dmesg > /dev/ttyS0\n";

/// The console's message when it starts, in the kernel log and its mirror.
const STARTED: &str = r"console: on (\S+): (\d+x\d+) display, (\d+x\d+) cells";

/// The supervisor's message when the console ended.
const ENDED: &str = r"console on the display ended";

/// How the test waits: the guest gets `pause` to settle between steps,
/// and keys are typed 5 a `pause`; a boot may take `patience` seconds.
#[derive(Clone, Copy, Debug)]
pub(super) struct Pace {
    /// The pause between steps.
    pub(super) pause: Duration,
    /// The longest wait for the boot, in seconds.
    pub(super) patience: u64,
}

/// The pace of the VMs.
const PACE: Pace = Pace {
    pause: Duration::from_secs(1),
    patience: 1200,
};

/// Boots the image on both displays and checks the console on each.
///
/// # Errors
///
/// When a step of the test cannot be done.
pub(super) fn run(test: &Test) -> Result<()> {
    let image = zbm_image(test.settings(), &spec())?;
    let monitor = Monitor {
        dump: screendump,
        key: sendkey,
    };
    let mirror = mirror_path(test, "virtio");
    let mut vm = boot(test, &image, "virtio", &mirror)?;
    check_console(test, &mut vm, &mirror, &monitor, PACE)?;
    vm.kill();
    let mirror = mirror_path(test, "simpledrm");
    let mut vm = boot(test, &image, "simpledrm", &mirror)?;
    check_handover(test, &mut vm, &mirror, PACE)?;
    vm.kill();
    Ok(())
}

/// The image of the test.
pub(super) fn spec() -> ImageSpec {
    ImageSpec {
        runtime: Some(ZbmRuntime::Rust),
        conf: "add_drivers+=\" virtio_gpu evdev \"".into(),
        ..ImageSpec::new("console")
    }
}

/// Where the mirror of the boot `name` is kept.
fn mirror_path(test: &Test, name: &str) -> PathBuf {
    test.dir().join(format!("mirror-{name}.txt"))
}

/// Presses `key` on the machine's keyboard for 50 milliseconds.
fn sendkey(vm: &Vm<'_>, key: &str) -> Result<()> {
    vm.monitor(&format!("sendkey {key} 50")).map(drop)
}

/// QEMU's names of the keys that type `text` on the US layout; `None` for
/// a character without one.
pub(super) fn key_names(text: &str) -> Option<Vec<String>> {
    text.chars()
        .map(|c| {
            let name = match c {
                'a'..='z' | '0'..='9' => c.to_string(),
                'A'..='Z' => format!("shift-{}", c.to_ascii_lowercase()),
                ' ' => "spc".into(),
                '\n' => "ret".into(),
                '-' => "minus".into(),
                '/' => "slash".into(),
                '.' => "dot".into(),
                ':' => "shift-semicolon".into(),
                '|' => "shift-backslash".into(),
                '>' => "shift-dot".into(),
                '$' => "shift-4".into(),
                '(' => "shift-9".into(),
                ')' => "shift-0".into(),
                _ => return None,
            };
            Some(name)
        })
        .collect()
}

/// Types `text` on the machine's keyboard, a key each `pause` / 5: QEMU
/// holds each key a while, and a key sent before the last one was let go
/// can be lost.
fn type_text(vm: &Vm<'_>, monitor: &Monitor, text: &str, pause: Duration) -> Result<()> {
    let names = key_names(text).ok_or_else(|| Error::Setup(format!("no keys type {text:?}")))?;
    for name in names {
        (monitor.key)(vm, &name)?;
        thread::sleep(pause / 5);
    }
    Ok(())
}

/// The last screen of the mirror at `path`, for the notes of a failure.
pub(super) fn last_screen(path: &Path) -> String {
    let text = fs::read(path).unwrap_or_default();
    let text = String::from_utf8_lossy(&text).replace('\r', "");
    let screen = text.rsplit('\x0c').find(|screen| !screen.trim().is_empty());
    let lines: Vec<&str> = screen
        .unwrap_or("")
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
pub(super) fn wait_screen(path: &Path, pattern: &str, seconds: u64) -> Result<Option<String>> {
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

/// Boots `image` on the display `name`, `virtio` or `simpledrm`, with the
/// mirror written to `mirror`.
fn boot<'t>(test: &'t Test, image: &Image, name: &str, mirror: &Path) -> Result<Vm<'t>> {
    let settings = test.settings();
    let append = if name == "simpledrm" {
        format!("{APPEND} zbm.timeout=5 vga=0x318")
    } else {
        format!("{APPEND} zbm.timeout=30")
    };
    let mut machine = Machine::boot(image, &append).with_disks([super::disk(settings, 0)?]);
    machine.extra.extend(machine_extra(name, mirror));
    Vm::start(test, name, &machine)
}

/// QEMU's arguments for the display `name` and the mirror's serial port.
pub(super) fn machine_extra(name: &str, mirror: &Path) -> Vec<String> {
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

/// The countdown, the keys, the recovery shell on the console, and the
/// kernel console once the console is gone, at `pace`.
pub(super) fn check_console(
    test: &Test,
    vm: &mut Vm<'_>,
    mirror: &Path,
    monitor: &Monitor,
    pace: Pace,
) -> Result<()> {
    let countdown = wait_screen(mirror, COUNTDOWN, pace.patience)?;
    test.check(
        countdown.is_some(),
        "virtio: the countdown shows on the console",
    );
    let path = test.dir().join("virtio-countdown.ppm");
    (monitor.dump)(vm, &path)?;
    let colours = fs::read(&path).ok().and_then(|ppm| screen_colours(&ppm));
    test.check(
        colours.is_some_and(|count| count >= COUNTDOWN_COLOURS),
        &format!("virtio: the console draws it in colour ({colours:?} colours)"),
    );
    check_shell(test, vm, mirror, monitor, pace)?;
    let started = wait_screen(mirror, STARTED, pace.patience.min(10))?;
    let shown = started.as_deref().unwrap_or("").trim().to_owned();
    test.check(
        started.is_some(),
        &format!("virtio: the console started on the display ({shown})"),
    );
    check_fallback(test, vm, mirror, monitor, pace)
}

/// The console killed from its shell: the kernel console takes keys again,
/// and the supervisor's message is in the kernel log.
fn check_fallback(
    test: &Test,
    vm: &mut Vm<'_>,
    mirror: &Path,
    monitor: &Monitor,
    pace: Pace,
) -> Result<()> {
    type_text(vm, monitor, KILL, pace.pause)?;
    thread::sleep(pace.pause * 5);
    let path = test.dir().join("virtio-fallback.ppm");
    (monitor.dump)(vm, &path)?;
    let colours = fs::read(&path).ok().and_then(|ppm| screen_colours(&ppm));
    let screen = last_screen(mirror);
    test.note(&format!(
        "virtio: after the kill, {colours:?} colours on the display; the console showed: {screen}"
    ));
    type_text(vm, monitor, BACK, pace.pause)?;
    let back = vm.expect(&["zbm-back"], pace.patience.min(30)).is_ok();
    test.check(
        back,
        "virtio: once the console ended, a shell on the kernel console takes keys",
    );
    type_text(vm, monitor, LOG, pace.pause)?;
    let ended = back && vm.expect(&[ENDED], pace.patience.min(30)).is_ok();
    test.check(
        ended,
        "virtio: the supervisor logged that the console ended",
    );
    Ok(())
}

/// Escape typed during the countdown opens the recovery shell on the
/// console, which runs on its pseudo-terminal and takes typed commands.
fn check_shell(
    test: &Test,
    vm: &mut Vm<'_>,
    mirror: &Path,
    monitor: &Monitor,
    pace: Pace,
) -> Result<()> {
    thread::sleep(pace.pause * 2);
    (monitor.key)(vm, "esc")?;
    let shell = wait_screen(mirror, "emergency shell", pace.patience.min(60))?;
    test.check(
        shell.is_some(),
        "virtio: Escape typed on the keyboard opens the recovery shell on the console",
    );
    if shell.is_none() {
        test.note(&format!(
            "virtio: the console shows: {}",
            last_screen(mirror)
        ));
    }
    thread::sleep(pace.pause * 3);
    type_text(vm, monitor, TTY, pace.pause)?;
    let tty = wait_screen(mirror, r"/dev/pts/\d+", pace.patience.min(60))?;
    test.check(
        tty.is_some(),
        "virtio: the shell runs on the console's pseudo-terminal and takes typed commands",
    );
    if tty.is_none() {
        test.note(&format!(
            "virtio: the console shows: {}",
            last_screen(mirror)
        ));
    }
    Ok(())
}

/// The countdown runs out: the console hands the display back, and the boot
/// environment boots; the boot may take `pace.patience` seconds.
pub(super) fn check_handover(
    test: &Test,
    vm: &mut Vm<'_>,
    mirror: &Path,
    pace: Pace,
) -> Result<()> {
    let countdown = wait_screen(mirror, COUNTDOWN, pace.patience)?;
    test.check(
        countdown.is_some(),
        "simpledrm: the countdown shows on the console",
    );
    let released = wait_screen(mirror, RELEASED, pace.patience.min(120))?;
    test.check(
        released.is_some(),
        "simpledrm: the console hands the display back before kexec",
    );
    let booted = vm.expect(&["BE-INIT-DONE"], pace.patience).is_ok();
    test.check(
        booted,
        "simpledrm: the boot environment boots after the hand-over",
    );
    Ok(())
}

#[cfg(test)]
mod tests;
