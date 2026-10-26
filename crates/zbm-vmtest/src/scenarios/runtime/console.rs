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
//!   The console draws with the image's TrueType font, anti-aliased. It
//!   starts with the image's first keyboard layout, `us`; switched to `de`
//!   from the shell (`zbm console layout`, step 4), the keys of `y` and `z`
//!   swap and the dead circumflex composes `ê`, in `cat` and at the shell's
//!   prompt. Ctrl-C typed while `od -v /dev/zero` floods the console stops
//!   it. Handed
//!   back from the shell (`zbm console release`), the display is taken
//!   again (`zbm console reattach`), as after a failed `kexec`.
//! - On `simpledrm` (`vga=0x318`), with `zbm.console_font=psf`: the console
//!   draws with a console font; the countdown runs out, the console hands
//!   the display back, and the boot environment boots.

use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use crate::boot::prompt;
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

/// The fewest colours of the countdown drawn with the TrueType font: the
/// background, the text, the boot environment and the seconds in their
/// colours (4 with a console font), and the shades between them that
/// anti-aliasing gives.
const COUNTDOWN_COLOURS: usize = 8;

/// Typed in the recovery shell: the shell's terminal (the image has no
/// `tty`).
const TTY: &str = "readlink /proc/self/fd/0\n";

/// Typed in the recovery shell: hands the display back, as before `kexec`.
const RELEASE: &str = "/libexec/zbm console release\n";

/// Typed on the kernel console then: takes the display again, as after a
/// failed `kexec`; then a line the console's screen shows.
const REATTACH: &str = "/libexec/zbm console reattach\n";
const AGAIN: &str = "echo zbm-reattached\n";

/// Typed in the recovery shell, with a layout's name: switches the
/// console's keyboard layout.
const LAYOUT: &str = "/libexec/zbm console layout ";

/// Keys typed with the German layout: `cat`, then the keys of `y` and `z`,
/// the dead circumflex (left of `1`) and `e`, which the terminal echoes as
/// `zyê`, and `Ctrl-D`. `cat` rather than the shell's `echo`: the shell's
/// line editor, in the image's C locale, takes the bytes of `ê` for
/// Meta keys.
const GERMAN_KEYS: &[&str] = &[
    "c",
    "a",
    "t",
    "ret",
    "y",
    "z",
    "grave_accent",
    "e",
    "ret",
    "ctrl-d",
];

/// The same keys typed at the shell's prompt, after `echo`: its line editor
/// must keep `ê` (`/etc/inputrc`, and `C.UTF-8` where the image has it).
const ECHO_KEYS: &[&str] = &[
    "e",
    "c",
    "h",
    "o",
    "spc",
    "y",
    "z",
    "grave_accent",
    "e",
    "ret",
];

/// The console's message when it starts, in the kernel log and its mirror.
const STARTED: &str = r"console: on (\S+): (\d+x\d+) display, (\S+) (\d+x\d+) cells";

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
        conf: "add_drivers+=\" virtio_gpu evdev \"\nzfsbootmenu_keyboard_layouts=\"us de\"".into(),
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

/// QEMU's names of the keys that type `text` on the German layout, for
/// letters, digits, spaces, `/` and newlines; `None` for other characters.
pub(super) fn german_key_names(text: &str) -> Option<Vec<String>> {
    text.chars()
        .map(|c| match c {
            'y' => Some("z".to_owned()),
            'z' => Some("y".to_owned()),
            '/' => Some("shift-7".to_owned()),
            'a'..='x' | '0'..='9' | ' ' | '\n' => key_names(&c.to_string())?.pop(),
            _ => None,
        })
        .collect()
}

/// Presses the keys `names`, a key each `pause` / 5: QEMU holds each key a
/// while, and a key sent before the last one was let go can be lost.
fn press_keys(vm: &Vm<'_>, monitor: &Monitor, names: &[String], pause: Duration) -> Result<()> {
    for name in names {
        (monitor.key)(vm, name)?;
        thread::sleep(pause / 5);
    }
    Ok(())
}

/// Types `text` on the machine's keyboard, on the US layout.
fn type_text(vm: &Vm<'_>, monitor: &Monitor, text: &str, pause: Duration) -> Result<()> {
    let names = key_names(text).ok_or_else(|| Error::Setup(format!("no keys type {text:?}")))?;
    press_keys(vm, monitor, &names, pause)
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
        format!("{APPEND} zbm.timeout=5 zbm.console_font=psf vga=0x318")
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
        &format!("virtio: the console draws it in colour, anti-aliased ({colours:?} colours)"),
    );
    check_shell(test, vm, mirror, monitor, pace)?;
    fallback::check_flood(test, vm, monitor, pace)?;
    check_font(test, "virtio", mirror, ".ttf", pace)?;
    check_layouts(test, vm, mirror, monitor, pace)?;
    check_reattach(test, vm, mirror, monitor, pace)?;
    fallback::check_fallback(test, vm, mirror, monitor, pace)
}

/// The display handed back from the shell, then taken again, as after a
/// failed `kexec`: what is typed afterwards shows on the console's screen.
fn check_reattach(
    test: &Test,
    vm: &mut Vm<'_>,
    mirror: &Path,
    monitor: &Monitor,
    pace: Pace,
) -> Result<()> {
    type_text(vm, monitor, RELEASE, pace.pause)?;
    let released = wait_screen(mirror, RELEASED, pace.patience.min(30))?;
    test.check(
        released.is_some(),
        "virtio: zbm console release hands the display back",
    );
    thread::sleep(pace.pause * 2);
    type_text(vm, monitor, REATTACH, pace.pause)?;
    thread::sleep(pace.pause * 3);
    type_text(vm, monitor, AGAIN, pace.pause)?;
    let again = wait_screen(mirror, r"(?m)^zbm-reattached", pace.patience.min(30))?;
    test.check(
        again.is_some(),
        "virtio: zbm console reattach takes the display again",
    );
    if again.is_none() {
        test.note(&format!(
            "virtio: the console shows: {}",
            last_screen(mirror)
        ));
    }
    Ok(())
}

/// The keyboard layouts: the console starts with the image's first, and
/// the shell switches it to `de` and back, typing the second time with
/// the German layout.
fn check_layouts(
    test: &Test,
    vm: &mut Vm<'_>,
    mirror: &Path,
    monitor: &Monitor,
    pace: Pace,
) -> Result<()> {
    let first = wait_screen(
        mirror,
        r"cells, \d+x\d+, keyboard us",
        pace.patience.min(10),
    )?;
    test.check(
        first.is_some(),
        "virtio: the console starts with the image's first keyboard layout, us",
    );
    type_text(vm, monitor, &format!("{LAYOUT}de\n"), pace.pause)?;
    let switched = wait_screen(mirror, "keyboard: de", pace.patience.min(30))?;
    test.check(
        switched.is_some(),
        "virtio: zbm console layout de switches the console to the German layout",
    );
    check_german(test, vm, mirror, monitor, pace)?;
    let back = format!("{LAYOUT}us\n");
    let names = german_key_names(&back).ok_or_else(|| Error::Setup(back.clone()))?;
    press_keys(vm, monitor, &names, pace.pause)?;
    let us = wait_screen(mirror, "keyboard: us", pace.patience.min(30))?;
    test.check(
        us.is_some(),
        "virtio: typed on the German layout, zbm console layout us switches back",
    );
    Ok(())
}

/// The font of the console's start, from its mirror, of a file ending in
/// `extension`: `.ttf` by default, `.psf` with `zbm.console_font=psf`.
pub(super) fn check_font(
    test: &Test,
    name: &str,
    mirror: &Path,
    extension: &str,
    pace: Pace,
) -> Result<()> {
    let started = wait_screen(mirror, STARTED, pace.patience.min(10))?;
    let shown = started.as_deref().unwrap_or("").trim().to_owned();
    test.check(
        started.is_some(),
        &format!("{name}: the console started on the display ({shown})"),
    );
    let font = regex(STARTED)?
        .captures(&shown)
        .and_then(|captures| captures.get(3))
        .map_or(String::new(), |font| font.as_str().to_owned());
    test.check(
        font.ends_with(extension),
        &format!("{name}: it draws with a {extension} font ({font})"),
    );
    Ok(())
}

/// Keys typed on the German layout, into `cat` and at the shell's prompt.
fn check_german(
    test: &Test,
    vm: &mut Vm<'_>,
    mirror: &Path,
    monitor: &Monitor,
    pace: Pace,
) -> Result<()> {
    let german: Vec<String> = GERMAN_KEYS.iter().map(|&name| name.to_owned()).collect();
    press_keys(vm, monitor, &german, pace.pause)?;
    let swapped = wait_screen(mirror, r"(?m)^zy", pace.patience.min(30))?;
    test.check(
        swapped.is_some(),
        "virtio: keys type on the German layout: the keys of y and z swap",
    );
    let composed = wait_screen(mirror, r"(?m)^zyê\s*$", pace.patience.min(10))?;
    test.check(
        composed.is_some(),
        "virtio: the dead circumflex then e types ê",
    );
    if composed.is_none() {
        test.note(&format!(
            "virtio: the console shows: {}",
            last_screen(mirror)
        ));
    }
    let at_prompt: Vec<String> = ECHO_KEYS.iter().map(|&name| name.to_owned()).collect();
    press_keys(vm, monitor, &at_prompt, pace.pause)?;
    let echoed = wait_screen(mirror, r"> echo zyê\s*\n\s*zyê\s*\n", pace.patience.min(30))?;
    test.check(
        echoed.is_some(),
        "virtio: the shell's line editor takes ê typed at its prompt",
    );
    if echoed.is_none() {
        test.note(&format!(
            "virtio: the console shows: {}",
            last_screen(mirror)
        ));
    }
    Ok(())
}

/// Escape typed during the countdown opens the menu on the console, and
/// `!` then Enter its recovery shell, which runs on the console's
/// pseudo-terminal and takes typed commands.
pub(super) fn check_shell(
    test: &Test,
    vm: &mut Vm<'_>,
    mirror: &Path,
    monitor: &Monitor,
    pace: Pace,
) -> Result<()> {
    thread::sleep(pace.pause * 2);
    (monitor.key)(vm, "esc")?;
    // The mirror holds whole screens: the pattern is matched on them.
    let panel = prompt::NEW_MENU.trim_start_matches(crate::expect::SCREEN);
    let menu = wait_screen(mirror, panel, pace.patience.min(60))?;
    test.check(
        menu.is_some(),
        "virtio: Escape typed on the keyboard opens the menu on the console",
    );
    thread::sleep(pace.pause * 2);
    (monitor.key)(vm, "shift-1")?;
    thread::sleep(pace.pause);
    (monitor.key)(vm, "ret")?;
    let shell = wait_screen(mirror, "emergency shell", pace.patience.min(60))?;
    test.check(
        shell.is_some(),
        "virtio: the menu opens the recovery shell on the console",
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
    check_font(test, "simpledrm", mirror, ".psf", pace)?;
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

mod fallback;

#[cfg(test)]
mod tests;
