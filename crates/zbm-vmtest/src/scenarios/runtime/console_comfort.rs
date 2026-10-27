//! `runtime-rust-console-comfort`: the recovery shell on the console on
//! the display, made comfortable (ROADMAP.md, M3). The
//! image of `runtime-rust-console-start`, with the `secret-test` feature,
//! boots with a status line (`zbm.console_status=on`):
//!
//! - The status line names the keyboard layout and the time, and Caps Lock
//!   while it is on (C83).
//! - Ctrl+Alt+= makes the text larger: the programs get fewer columns, and
//!   `zbm console font` names the font; Ctrl+Alt+0 gives back the first
//!   size (C87).
//! - Shift+PgUp shows the lines that scrolled off, and where the screen is
//!   in the history (C52).
//! - A line that scrolled off is in the console's memory, in the cells of
//!   its terminal, which `zbm debug secrets --cells` searches; once a
//!   prompt without echo is open (`stty -echo`) and a key is typed, it is
//!   not (C52).

use std::path::Path;
use std::thread;
use std::time::Duration;

use crate::error::{Error, Result};
use crate::expect::regex;
use crate::images::{ImageSpec, zbm_image};
use crate::qemu::Machine;
use crate::record::Test;
use crate::vm::Vm;

use super::COUNTDOWN;
use super::colours::screendump;
use super::console::{Pace, check_shell, last_screen, machine_extra, wait_screen};
use super::console_start::names;
use super::drm::Monitor;

/// The command line: the console with its mirror and a status line.
const APPEND: &str = "console=ttyS0 console=tty1 loglevel=7 zbm.console=drm \
    zbm.console_mirror=/dev/ttyS1 zbm.timeout=60 zbm.console_status=on";

/// The line that scrolls off, unlike anything else in memory; the command
/// that writes it holds it only in parts.
const MARKER: &str = "zbmtest-Scroll-42x9Kq";
const WRITE_MARKER: &str = "echo zbmtest-Scroll-$((6*7))x9Kq\n";

/// Lines enough to scroll the marker off the screen, counted by the
/// shell: the image has no `seq`.
const ROWS: &str = "i=0; while test $i -lt 80; do i=$((i+1)); echo zbm-row-$i; done\n";

/// The pace of the VM.
const PACE: Pace = Pace {
    pause: std::time::Duration::from_secs(1),
    patience: 1200,
};

/// Boots the image and checks the status line, the font's size, the
/// scrollback and its wiping.
///
/// # Errors
///
/// When a step of the test cannot be done.
pub(super) fn run(test: &Test) -> Result<()> {
    let image = zbm_image(test.settings(), &spec())?;
    let mirror = test.dir().join("mirror-comfort.txt");
    let mut machine = Machine::boot(&image, APPEND).with_disks([super::disk(test.settings(), 0)?]);
    machine.extra.extend(machine_extra("virtio", &mirror));
    let mut vm = Vm::start(test, "virtio", &machine)?;
    let countdown = wait_screen(&mirror, COUNTDOWN, PACE.patience)?;
    test.check(countdown.is_some(), "the console shows the countdown");
    let monitor = Monitor {
        dump: screendump,
        key: sendkey,
    };
    check_shell(test, &mut vm, &mirror, &monitor, PACE)?;
    let comfort = Comfort {
        test,
        mirror: &mirror,
        key: sendkey,
        pause: PACE.pause,
    };
    comfort.check(&mut vm)?;
    vm.kill();
    Ok(())
}

/// The image of the test: the console's, with the drivers of a USB
/// keyboard and the search for secrets.
fn spec() -> ImageSpec {
    ImageSpec {
        variant: "console-comfort".into(),
        features: Some("zbm/console-mirror,zbm/secret-test".into()),
        ..super::console_start::spec()
    }
}

/// Presses `key` on the machine's keyboard for 50 milliseconds.
fn sendkey(vm: &Vm<'_>, key: &str) -> Result<()> {
    vm.send_key(key)
}

/// QEMU's names of the keys that type `text`, with `&`, `*`, `=` and `+`.
fn keys(text: &str) -> Option<Vec<String>> {
    text.chars()
        .map(|c| match c {
            '&' => Some(vec!["shift-7".to_owned()]),
            '*' => Some(vec!["shift-8".to_owned()]),
            '=' => Some(vec!["equal".to_owned()]),
            '+' => Some(vec!["shift-equal".to_owned()]),
            _ => names(&c.to_string()),
        })
        .collect::<Option<Vec<_>>>()
        .map(|keys| keys.concat())
}

/// `bytes` in hexadecimal.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The search for the marker, as text and as the terminal's cells hold
/// it, written to the serial console with the console's process ID; the
/// command holds the marker only in hexadecimal.
fn search() -> String {
    format!(
        "(echo ZBMTEST-CONSOLE-$(cat /zfsbootmenu/console); \
         /libexec/zbm debug secrets --cells {}; echo ZBMTEST-SEARCH-END) > /dev/ttyS0 2>&1",
        hex(MARKER.as_bytes())
    )
}

/// The places the search found the marker in the console's memory, from
/// the serial console.
fn found_in_console(vm: &mut Vm<'_>) -> Result<Option<Vec<String>>> {
    // To the end of the line: a process ID read while its digits still
    // arrive would find nothing, and pass the check after the prompt.
    if vm.expect1(r"ZBMTEST-CONSOLE-(\d+)\r?\n", 120).is_err() {
        return Ok(None);
    }
    let pid = vm.group(1);
    vm.expect1("ZBMTEST-SEARCH-END", 600)?;
    Ok(Some(found_in(&vm.before(), &pid)))
}

/// The lines of the search's `output` that found a secret in the process
/// `pid`.
fn found_in(output: &str, pid: &str) -> Vec<String> {
    let process = format!("in process {pid} ");
    output
        .lines()
        .filter(|line| line.contains("found secret") && line.contains(&process))
        .map(|line| line.trim().to_owned())
        .collect()
}

/// The number of columns in `screen` after `echo zbm-NAME-$(stty size)`.
fn columns_in(screen: &str, name: &str) -> Option<u32> {
    regex(&format!(r"(?m)^zbm-{name}-\d+ (\d+)\s*$"))
        .ok()?
        .captures(screen)?
        .get(1)?
        .as_str()
        .parse()
        .ok()
}

/// The test, the console's mirror, how keys are pressed and the pause
/// between steps.
struct Comfort<'t> {
    test: &'t Test,
    mirror: &'t Path,
    key: fn(&Vm<'_>, &str) -> Result<()>,
    pause: Duration,
}

impl Comfort<'_> {
    /// Checks the status line, the font's size, the scrollback and its
    /// wiping.
    fn check(&self, vm: &mut Vm<'_>) -> Result<()> {
        self.check_status(vm)?;
        self.check_font(vm)?;
        self.check_scrollback(vm)?;
        self.check_secret(vm)
    }

    /// Presses `key`, then waits `pauses` pauses.
    fn press(&self, vm: &Vm<'_>, key: &str, pauses: u32) -> Result<()> {
        (self.key)(vm, key)?;
        thread::sleep(self.pause * pauses);
        Ok(())
    }

    /// Types `text`, a key each fifth of a pause.
    fn type_text(&self, vm: &Vm<'_>, text: &str) -> Result<()> {
        let keys = keys(text).ok_or_else(|| Error::Setup(format!("no keys type {text:?}")))?;
        for key in keys {
            (self.key)(vm, &key)?;
            thread::sleep(self.pause / 5);
        }
        Ok(())
    }

    /// Notes the console's last screen when `is_seen` is false.
    fn note_unless(&self, is_seen: bool) {
        if !is_seen {
            self.test
                .note(&format!("the console shows: {}", last_screen(self.mirror)));
        }
    }

    /// The status line names the layout and the time, and Caps Lock while
    /// it is on.
    fn check_status(&self, vm: &Vm<'_>) -> Result<()> {
        let status = wait_screen(self.mirror, r"(?m)\bus  \d\d:\d\d UTC\s*$", 30)?;
        self.test.check(
            status.is_some(),
            "the status line names the keyboard layout and the time",
        );
        self.note_unless(status.is_some());
        self.press(vm, "caps_lock", 0)?;
        let caps = wait_screen(self.mirror, r"(?m)\bus  Caps  \d\d:\d\d UTC\s*$", 30)?;
        self.test
            .check(caps.is_some(), "the status line says Caps Lock is on");
        self.note_unless(caps.is_some());
        self.press(vm, "caps_lock", 1)
    }

    /// The columns the programs see, after `echo zbm-NAME-$(stty size)`.
    fn columns(&self, vm: &Vm<'_>, name: &str) -> Result<Option<u32>> {
        self.type_text(vm, &format!("echo zbm-{name}-$(stty size)\n"))?;
        let pattern = format!(r"(?m)^zbm-{name}-\d+ \d+\s*$");
        let screen = wait_screen(self.mirror, &pattern, 30)?;
        Ok(screen.and_then(|screen| columns_in(&screen, name)))
    }

    /// Ctrl+Alt+= makes the text larger and gives the programs fewer
    /// columns, which `zbm console font` names; Ctrl+Alt+0 gives back the
    /// first size.
    fn check_font(&self, vm: &Vm<'_>) -> Result<()> {
        let first = self.columns(vm, "first")?;
        self.press(vm, "ctrl-alt-equal", 2)?;
        let larger = self.columns(vm, "larger")?;
        self.test.check(
            first
                .zip(larger)
                .is_some_and(|(first, larger)| larger < first),
            &format!("Ctrl+Alt+= makes the text larger: {first:?} then {larger:?} columns"),
        );
        self.type_text(vm, "/libexec/zbm console font\n")?;
        let named = match larger {
            Some(columns) => format!(r"(?m)^font: \S+ \d+x\d+, {columns}x\d+\s*$"),
            None => "(?m)^font: ".to_owned(),
        };
        let font = wait_screen(self.mirror, &named, 30)?;
        self.test.check(
            font.is_some(),
            "zbm console font names the font and the columns",
        );
        self.note_unless(font.is_some());
        self.press(vm, "ctrl-alt-0", 2)?;
        let back = self.columns(vm, "back")?;
        self.test.check(
            back.is_some() && back == first,
            &format!("Ctrl+Alt+0 gives back the first size: {back:?} columns"),
        );
        Ok(())
    }

    /// Shift+PgUp shows the lines that scrolled off, with where the screen
    /// is in the history.
    fn check_scrollback(&self, vm: &Vm<'_>) -> Result<()> {
        self.type_text(vm, ROWS)?;
        thread::sleep(self.pause * 3);
        self.press(vm, "shift-pgup", 0)?;
        let back = wait_screen(self.mirror, r"(?m)^zbm-row-\d+\s+\[\d+/\d+\]\s*$", 30)?;
        self.test.check(
            back.is_some(),
            "Shift+PgUp shows the lines that scrolled off, and the place in the history",
        );
        self.note_unless(back.is_some());
        self.press(vm, "shift-pgdn", 1)
    }

    /// A line that scrolled off is in the console's memory; once a prompt
    /// without echo is open and a key typed, it is not.
    fn check_secret(&self, vm: &mut Vm<'_>) -> Result<()> {
        self.type_text(vm, WRITE_MARKER)?;
        self.type_text(vm, ROWS)?;
        thread::sleep(self.pause * 3);
        self.type_text(vm, &format!("{}\n", search()))?;
        let before = found_in_console(vm)?;
        self.test.check(
            before.as_ref().is_some_and(|found| !found.is_empty()),
            &format!("the line that scrolled off is in the console's memory: {before:?}"),
        );
        let prompt = format!("({}) & stty -echo; sleep 30; stty echo\n", search_later());
        self.type_text(vm, &prompt)?;
        thread::sleep(self.pause * 3);
        // A key typed at the prompt wakes the console.
        self.press(vm, "spc", 0)?;
        let during = found_in_console(vm)?;
        self.test.check(
            during.as_ref().is_some_and(Vec::is_empty),
            &format!(
                "at a prompt without echo, the console's memory holds no scrollback: {during:?}"
            ),
        );
        Ok(())
    }
}

/// The search, a few seconds after the prompt opened and a key was typed.
fn search_later() -> String {
    format!("sleep 8; {}", search())
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::testing::{Step, checks, cleanup, scripted, settings};

    /// Presses nothing: the screens are in the mirror already.
    fn no_key(_vm: &Vm<'_>, _key: &str) -> Result<()> {
        Ok(())
    }

    #[test]
    fn the_checks_pass_on_the_screens_and_searches_they_expect() {
        let settings = settings("runtime-console-comfort");
        let test = Test::new(&settings, "runtime-rust-console-comfort").unwrap();
        let mirror = test.dir().join("mirror-comfort.txt");
        let screen = "zbm-row-60                              [12/80]\n\
                      zbm-first-25 80\nzbm-larger-20 64\nfont: DejaVu 12x24, 64x20\n\
                      zbm-back-25 80\n\
                      us  Caps  12:00 UTC\n\
                      \x0c\n\
                      $\n                                us  12:00 UTC\n";
        std::fs::write(&mirror, screen).unwrap();
        let script = vec![
            Step::Say("ZBMTEST-CONSOLE-42\r\n"),
            Step::Say("found secret 0 in process 42 (zbm) memory 0-40 [heap]\r\n"),
            Step::Say("found secret 0 in process 420 (sh) memory 0-40\r\n"),
            Step::Say("ZBMTEST-SEARCH-END\r\n"),
            Step::Pause(300),
            Step::Say("found secret 0 in process 420 (sh) memory 0-40\r\n"),
            Step::Say("ZBMTEST-CONSOLE-42\r\nZBMTEST-SEARCH-END\r\n"),
        ];
        let (mut vm, guest) = scripted(&test, script);
        let comfort = Comfort {
            test: &test,
            mirror: &mirror,
            key: no_key,
            pause: Duration::ZERO,
        };

        comfort.check(&mut vm).unwrap();

        assert_eq!(guest.finish(), Vec::<String>::new());
        let found = checks(&test);
        cleanup(&settings);
        assert_eq!(found.len(), 8, "{found:#?}");
        assert!(
            found.iter().all(|line| line.starts_with("ok")),
            "{found:#?}"
        );
    }

    #[test]
    fn the_console_s_process_id_is_read_whole() {
        let settings = settings("runtime-console-comfort-pid");
        let test = Test::new(&settings, "runtime-rust-console-comfort").unwrap();
        let script = vec![
            Step::Say("ZBMTEST-CONSOLE-4"),
            Step::Pause(300),
            Step::Say("2\r\n"),
            Step::Say("found secret 0 in process 42 (zbm) memory 0-40 [heap]\r\n"),
            Step::Say("ZBMTEST-SEARCH-END\r\n"),
        ];
        let (mut vm, guest) = scripted(&test, script);

        let found = found_in_console(&mut vm).unwrap();

        assert_eq!(guest.finish(), Vec::<String>::new());
        cleanup(&settings);
        assert_eq!(
            found,
            Some(vec![
                "found secret 0 in process 42 (zbm) memory 0-40 [heap]".to_owned()
            ])
        );
    }

    #[test]
    fn the_search_s_findings_and_the_columns_are_read() {
        let output = "found secret 0 in process 7 (zbm) memory 1-2\n\
                      found secret 0 in process 77 (sh) memory 1-2\nother\n";
        assert_eq!(
            found_in(output, "7"),
            ["found secret 0 in process 7 (zbm) memory 1-2"]
        );
        assert_eq!(found_in(output, "8"), Vec::<String>::new());
        assert_eq!(columns_in("$\nzbm-x-25 80  \n", "x"), Some(80));
        assert_eq!(columns_in("zbm-x-25 80\n", "y"), None);
        let spec = spec();
        assert_eq!(spec.variant, "console-comfort");
        assert_eq!(
            spec.features.as_deref(),
            Some("zbm/console-mirror,zbm/secret-test")
        );
    }

    #[test]
    fn the_commands_have_keys_and_hold_the_marker_only_in_parts() {
        for text in [WRITE_MARKER, ROWS, &search(), &search_later()] {
            assert!(keys(text).is_some(), "{text}");
            assert!(!text.contains(MARKER), "{text}");
        }
        assert!(search().contains(&hex(MARKER.as_bytes())));
        assert_eq!(
            keys("&*=+").unwrap(),
            ["shift-7", "shift-8", "equal", "shift-equal"]
        );
        assert!(!ROWS.contains("seq"), "the image has no seq");
    }
}
