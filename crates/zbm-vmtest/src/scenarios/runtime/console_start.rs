//! `runtime-rust-console-start`: how the console on the display starts,
//! and what it shows (ROADMAP.md, M1). The Rust image of
//! `runtime-rust-console`, with the drivers of a USB keyboard, boots on a
//! virtio GPU whose kernel console runs at 800x600, below the display's
//! preferred mode (`video=Virtual-1:800x600`), and without a keyboard: the
//! machine's PS/2 keyboard is left out (`i8042.nokbd`), and a USB keyboard
//! is plugged in once the countdown shows.
//!
//! - Without a keyboard, the console starts: the countdown shows (C34).
//! - It keeps the mode the kernel console shows, 800x600 (C59).
//! - The keyboard plugged in later types: Escape opens the menu, which
//!   opens the recovery shell on the console (C34), which is brush.
//! - The cursor takes the shape asked for: a steady bar shows fewer pixels
//!   of the cursor's colour than a steady block (C16).
//! - With Num Lock, the keypad types digits (C35).
//! - What is written to `/dev/console` shows on the console (C78).
//! - Handed back and taken again, the console shows the screen it had
//!   (C90).
//!
//! And what M2 made of it, a terminal of its own:
//!
//! - Programs on it see `TERM=xterm-256color`, and `tput colors` says 256
//!   (C25).
//! - A 256-colour and a true colour bar show in their exact colours, and a
//!   Linux palette sequence changes colour 1 (C21), in a screenshot.
//! - Ctrl-Left moves back a word in brush's line editor (C26).

use std::fs;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use crate::error::{Error, Result};
use crate::expect::regex;
use crate::images::{ImageSpec, zbm_image};
use crate::qemu::Machine;
use crate::record::Test;
use crate::settings::ZbmRuntime;
use crate::vm::Vm;

use super::COUNTDOWN;
use super::colours::{parse_ppm, screendump};
use super::console::{Pace, check_shell, key_names, last_screen, machine_extra, wait_screen};
use super::drm::Monitor;

/// The command line: the console with its mirror, no PS/2 keyboard, and the
/// kernel console at 800x600.
const APPEND: &str = "console=ttyS0 console=tty1 loglevel=7 zbm.console=drm \
    zbm.console_mirror=/dev/ttyS1 zbm.timeout=60 i8042.nokbd video=Virtual-1:800x600";

/// The mode of the kernel console, which the console keeps.
const MODE: &str = "800x600";

/// The keyboard plugged in once the console runs.
const KEYBOARD: &str = "device_add usb-kbd,bus=xhci.0,id=latekbd";

/// The console's colour of text, and of its cursor.
const GREY: [u8; 3] = [0xaa, 0xaa, 0xaa];

/// Typed in the recovery shell: clears the screen and asks for a steady
/// block, then for a steady bar.
const BLOCK: &str = "printf '\\e[H\\e[2J\\e[2 q'\n";
const BAR: &str = "printf '\\e[H\\e[2J\\e[6 q'\n";

/// Typed in the recovery shell: brush names its version.
const BRUSH: &str = "echo zbm-brush-$BRUSH_VERSION\n";

/// Typed in the recovery shell: a line written to `/dev/console`, which
/// shows on the console only when it goes there; the typed line itself
/// shows `$(echo console)`.
const DEV_CONSOLE: &str = "echo zbm-$(echo console) > /dev/console\n";

/// Typed in the recovery shell: a line the console shows, then the
/// hand-over and the display taken again.
const KEPT: &str = "echo zbm-$(echo kept)\n";
const RELEASE: &str = "/libexec/zbm console release\n";
const REATTACH: &str = "/libexec/zbm console reattach\n";

/// Typed in the recovery shell: the terminal description and its colours.
const TERM: &str = "echo zbm-$TERM-$(tput colors)\n";

/// Typed in the recovery shell: a bar of colour 208 of the 256, of the
/// true colour (1, 2, 3), and of colour 1 after the Linux palette made it
/// (0x12, 0x34, 0x56), without the cursor.
const BAR_256: &str = "printf '\\e[H\\e[2J\\e[?25l\\e[48;5;208m    \\e[48;2;1;2;3m    \
    \\e]P1123456\\e[41m    \\e[0m'\n";

/// Typed in the recovery shell: the palette and the cursor back.
const RESTORE: &str = "printf '\\e]R\\e[?25h\\e[H\\e[2J'\n";

/// The colours of [`BAR_256`].
const BAR_COLOURS: [(&str, [u8; 3]); 3] = [
    ("colour 208", [255, 135, 0]),
    ("true colour", [1, 2, 3]),
    ("colour 1 of the Linux palette", [0x12, 0x34, 0x56]),
];

/// What the console writes to its mirror once it handed the display back.
const RELEASED: &str = "zbm console: handed the display back";

/// The pace of the VM.
const PACE: Pace = Pace {
    pause: Duration::from_secs(1),
    patience: 1200,
};

/// Boots the image without a keyboard and checks the console's start.
///
/// # Errors
///
/// When a step of the test cannot be done.
pub(super) fn run(test: &Test) -> Result<()> {
    let image = zbm_image(test.settings(), &spec())?;
    let mirror = test.dir().join("mirror-start.txt");
    let mut machine = Machine::boot(&image, APPEND).with_disks([super::disk(test.settings(), 0)?]);
    machine.extra.extend(machine_extra("virtio", &mirror));
    let mut vm = Vm::start(test, "virtio", &machine)?;
    let countdown = wait_screen(&mirror, COUNTDOWN, PACE.patience)?;
    test.check(
        countdown.is_some(),
        "without a keyboard, the console starts and shows the countdown",
    );
    check_mode(test, &mirror)?;
    let reply = vm.monitor(KEYBOARD)?;
    if !reply.is_empty() && reply != "OK" {
        test.note(&format!("monitor: device_add: {reply}"));
    }
    thread::sleep(PACE.pause * 5);
    let monitor = Monitor {
        dump: screendump,
        key: sendkey,
    };
    check_shell(test, &mut vm, &mirror, &monitor, PACE)?;
    type_text(&vm, BRUSH)?;
    let brush = wait_screen(&mirror, r"(?m)^zbm-brush-\d+\.\d+\.\d+\s*$", 30)?;
    test.check(brush.is_some(), "the recovery shell is brush");
    check_cursor(test, &vm)?;
    check_keypad(test, &vm, &mirror)?;
    type_text(&vm, DEV_CONSOLE)?;
    let shown = wait_screen(&mirror, r"(?m)^zbm-console\s*$", 30)?;
    test.check(
        shown.is_some(),
        "a line written to /dev/console shows on the console",
    );
    check_terminal(test, &vm, &mirror)?;
    check_word_left(test, &vm, &mirror)?;
    check_colours(test, &vm)?;
    check_kept(test, &vm, &mirror)?;
    vm.kill();
    Ok(())
}

/// The image of the test: the console's, with the drivers of a USB
/// keyboard.
pub(super) fn spec() -> ImageSpec {
    ImageSpec {
        runtime: Some(ZbmRuntime::Rust),
        conf: "add_drivers+=\" virtio_gpu evdev xhci_pci xhci_hcd usbhid hid_generic \"".into(),
        ..ImageSpec::new("console-start")
    }
}

/// Presses `key` on the machine's keyboard for 50 milliseconds.
fn sendkey(vm: &Vm<'_>, key: &str) -> Result<()> {
    vm.send_key(key)
}

/// QEMU's names of the keys that type `text` on the US layout, with the
/// characters of `printf`'s escapes and of variable names.
pub(super) fn names(text: &str) -> Option<Vec<String>> {
    text.chars()
        .map(|c| match c {
            '\'' => Some("apostrophe".to_owned()),
            '\\' => Some("backslash".to_owned()),
            '[' => Some("bracket_left".to_owned()),
            ']' => Some("bracket_right".to_owned()),
            ';' => Some("semicolon".to_owned()),
            '?' => Some("shift-slash".to_owned()),
            '_' => Some("shift-minus".to_owned()),
            _ => key_names(&c.to_string())?.pop(),
        })
        .collect()
}

/// Types `text`, a key each 200 milliseconds.
fn type_text(vm: &Vm<'_>, text: &str) -> Result<()> {
    let keys = names(text).ok_or_else(|| Error::Setup(format!("no keys type {text:?}")))?;
    for key in keys {
        sendkey(vm, &key)?;
        thread::sleep(PACE.pause / 5);
    }
    Ok(())
}

/// The console's start names the kernel console's mode.
fn check_mode(test: &Test, mirror: &Path) -> Result<()> {
    let started = wait_screen(mirror, r"console: on \S+: \d+x\d+ display", 30)?;
    let display = regex(r"(\d+x\d+) display")?
        .captures(started.as_deref().unwrap_or(""))
        .and_then(|captures| captures.get(1))
        .map_or(String::new(), |mode| mode.as_str().to_owned());
    test.check(
        display == MODE,
        &format!("the console keeps the kernel console's mode, {MODE} ({display})"),
    );
    Ok(())
}

/// How many pixels of a PPM image are `colour`; `None` when it is not one.
pub(super) fn count_pixels(ppm: &[u8], colour: [u8; 3]) -> Option<usize> {
    let (_, _, pixels) = parse_ppm(ppm)?;
    Some(
        pixels
            .chunks_exact(3)
            .filter(|pixel| *pixel == colour)
            .count(),
    )
}

/// A steady bar shows fewer pixels of the cursor's colour than a steady
/// block, on screens that are the same otherwise.
fn check_cursor(test: &Test, vm: &Vm<'_>) -> Result<()> {
    let mut counts = Vec::new();
    for (name, command) in [("block", BLOCK), ("bar", BAR)] {
        type_text(vm, command)?;
        thread::sleep(PACE.pause * 3);
        let path = test.dir().join(format!("cursor-{name}.ppm"));
        screendump(vm, &path)?;
        counts.push(
            fs::read(&path)
                .ok()
                .and_then(|ppm| count_pixels(&ppm, GREY)),
        );
    }
    let (block, bar) = (
        counts.first().copied().flatten(),
        counts.get(1).copied().flatten(),
    );
    test.check(
        block.zip(bar).is_some_and(|(block, bar)| block > bar),
        &format!("the cursor takes the shape asked for: block {block:?}, bar {bar:?} grey pixels"),
    );
    Ok(())
}

/// With Num Lock, the keys 4 and 2 of the keypad type `42`.
fn check_keypad(test: &Test, vm: &Vm<'_>, mirror: &Path) -> Result<()> {
    sendkey(vm, "num_lock")?;
    thread::sleep(PACE.pause);
    type_text(vm, "echo zbm-")?;
    for key in ["kp_4", "kp_2", "ret"] {
        sendkey(vm, key)?;
        thread::sleep(PACE.pause / 5);
    }
    let typed = wait_screen(mirror, r"(?m)^zbm-42\s*$", 30)?;
    test.check(typed.is_some(), "with Num Lock, the keypad types digits");
    if typed.is_none() {
        test.note(&format!("the console shows: {}", last_screen(mirror)));
    }
    Ok(())
}

/// Programs see the console's own terminal description, of 256 colours.
fn check_terminal(test: &Test, vm: &Vm<'_>, mirror: &Path) -> Result<()> {
    type_text(vm, TERM)?;
    let shown = wait_screen(mirror, r"(?m)^zbm-xterm-256color-256\s*$", 30)?;
    test.check(
        shown.is_some(),
        "programs on the console see TERM=xterm-256color and 256 colours",
    );
    if shown.is_none() {
        test.note(&format!("the console shows: {}", last_screen(mirror)));
    }
    Ok(())
}

/// Ctrl-Left moves back over the word typed last: `echo second`, then
/// `first-` typed before `second`.
fn check_word_left(test: &Test, vm: &Vm<'_>, mirror: &Path) -> Result<()> {
    type_text(vm, "echo second")?;
    sendkey(vm, "ctrl-left")?;
    thread::sleep(PACE.pause / 5);
    type_text(vm, "first-\n")?;
    let shown = wait_screen(mirror, r"(?m)^first-second\s*$", 30)?;
    test.check(
        shown.is_some(),
        "Ctrl-Left moves back a word in the recovery shell's line editor",
    );
    if shown.is_none() {
        test.note(&format!("the console shows: {}", last_screen(mirror)));
    }
    Ok(())
}

/// The bar of [`BAR_256`] shows each of its colours exactly.
fn check_colours(test: &Test, vm: &Vm<'_>) -> Result<()> {
    type_text(vm, BAR_256)?;
    thread::sleep(PACE.pause * 3);
    let path = test.dir().join("colours-256.ppm");
    screendump(vm, &path)?;
    let ppm = fs::read(&path).unwrap_or_default();
    for (name, colour) in BAR_COLOURS {
        let count = count_pixels(&ppm, colour);
        test.check(
            count.is_some_and(|count| count > 0),
            &format!("the console draws {name} exactly ({count:?} pixels)"),
        );
    }
    type_text(vm, RESTORE)
}

/// The first screen after the last line `marker` of the mirror's `text`
/// that matches `pattern`.
pub(super) fn screen_after(text: &str, marker: &str, pattern: &regex::Regex) -> Option<String> {
    let (_, after) = text.rsplit_once(marker)?;
    after
        .split('\x0c')
        .find(|screen| pattern.is_match(screen))
        .map(str::to_owned)
}

/// A line shown, the display handed back and taken again: the console
/// shows the line again, without being typed anything.
fn check_kept(test: &Test, vm: &Vm<'_>, mirror: &Path) -> Result<()> {
    type_text(vm, KEPT)?;
    let shown = wait_screen(mirror, r"(?m)^zbm-kept\s*$", 30)?;
    type_text(vm, RELEASE)?;
    let released = wait_screen(mirror, RELEASED, 30)?;
    test.check(
        shown.is_some() && released.is_some(),
        "a line shown, zbm console release hands the display back",
    );
    thread::sleep(PACE.pause * 2);
    type_text(vm, REATTACH)?;
    let pattern = regex(r"(?m)^zbm-kept\s*$")?;
    let start = Instant::now();
    let kept = loop {
        let text = String::from_utf8_lossy(&fs::read(mirror).unwrap_or_default()).replace('\r', "");
        let found = screen_after(&text, RELEASED, &pattern);
        if found.is_some() || start.elapsed() >= Duration::from_secs(30) {
            break found;
        }
        thread::sleep(Duration::from_millis(200));
    };
    test.check(
        kept.is_some(),
        "taken again after a failed kexec, the console shows the screen it had",
    );
    if kept.is_none() {
        test.note(&format!("the console shows: {}", last_screen(mirror)));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn printf_s_escapes_have_keys() {
        let keys = names("printf '\\e[2 q'\n").unwrap();
        assert_eq!(keys[7], "apostrophe");
        assert_eq!(keys[8], "backslash");
        assert_eq!(keys[10], "bracket_left");
        assert_eq!(keys.last().unwrap(), "ret");
        assert!(names("€").is_none());
        for text in [
            BRUSH,
            BLOCK,
            BAR,
            DEV_CONSOLE,
            KEPT,
            RELEASE,
            REATTACH,
            TERM,
            BAR_256,
            RESTORE,
        ] {
            assert!(names(text).is_some(), "{text}");
        }
    }

    #[test]
    fn pixels_of_a_colour_are_counted() {
        let mut ppm = b"P6\n2 1\n255\n".to_vec();
        ppm.extend([0xaa, 0xaa, 0xaa, 0, 0, 0]);
        assert_eq!(count_pixels(&ppm, GREY), Some(1));
        assert_eq!(count_pixels(b"P5\n", GREY), None);
    }

    #[test]
    fn only_screens_after_the_marker_count() {
        let pattern = regex("(?m)^kept").unwrap();
        let text = "\x0ckept\n\x0cmarker\n\x0cother\n\x0ckept\n";
        assert_eq!(
            screen_after(text, "marker", &pattern).as_deref(),
            Some("kept\n")
        );
        assert_eq!(screen_after("\x0ckept\n", "marker", &pattern), None);
    }
}
