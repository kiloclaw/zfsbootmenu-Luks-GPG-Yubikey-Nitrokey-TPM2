//! `runtime-rust-pilot`: the pilots of `REFACTOR-ROADMAP.md` (R0) in the
//! Rust image, on the serial console of the recovery shell: the menu with
//! `TERM=vt100` and `TERM=linux`, and `zluks --pilot open` on a LUKS volume
//! formatted in the VM.

use std::thread;
use std::time::{Duration, Instant};

use crate::error::{Error, Result};
use crate::qemu::Machine;
use crate::record::Test;
use crate::settings::ZbmRuntime;
use crate::vm::{Sh, Vm};

/// The key of the pilot's LUKS volume.
const PILOT_KEY: &str = "zbm-pilot-key-1234";

/// The frame statistics the menu pilot prints.
const STATS: &str = r"(\d+) frames, mean ([\d.]+) ms, slowest ([\d.]+) ms\r*\n";

/// Runs the pilots.
///
/// # Errors
///
/// When a step of the test cannot be done.
pub(super) fn run(test: &Test) -> Result<()> {
    let settings = test.settings();
    let image = super::image(settings, "auto", ZbmRuntime::Rust, None)?;
    let machine =
        Machine::boot(&image, super::ESCAPE_APPEND).with_disks([super::disk(settings, 0)?]);
    let mut vm = Vm::start(test, "boot", &machine)?;
    super::escape_countdown(&mut vm)?;
    vm.shell_ready()?;
    menu(&mut vm, "vt100", Some("80x24"))?;
    menu(&mut vm, "linux", None)?;
    unlock(&mut vm)?;
    vm.kill();
    Ok(())
}

/// The size of the serial log so far, in bytes, after what the VM sent in
/// the last second.
fn serial_bytes(vm: &mut Vm<'_>) -> u64 {
    thread::sleep(Duration::from_secs(1));
    vm.console().bytes_read()
}

/// The menu pilot on the serial console with `TERM=term`; types `be` and
/// chooses the match.
fn menu(vm: &mut Vm<'_>, term: &str, size: Option<&str>) -> Result<()> {
    let test = vm.test();
    let option = size.map_or_else(String::new, |size| format!(" --size {size}"));
    vm.sendline(&format!(
        "TERM={term} /libexec/zbm zbm-menu-pilot --stats{option}"
    ))?;
    // Ratatui moves the cursor over blank cells instead of writing spaces, so
    // the screen is matched word by word, and compared without spaces.
    let index = vm.expect(&["environments", r"zbm-menu-pilot: [^\r\n]*"], 120)?;
    if index == 1 {
        test.check(
            false,
            &format!("TERM={term}: the menu starts ({})", vm.group(0)),
        );
        return Ok(());
    }
    vm.expect1(r"\[Ctrl-U\]", 60)?;
    let screen = vm.before().replace(' ', "");
    test.check(
        screen.contains("tplain/ROOT/be"),
        &format!("TERM={term}: the boot environment is listed"),
    );
    let ascii = term == "vt100";
    let borders = if ascii { "ASCII" } else { "line-drawing" };
    test.check(
        screen.contains("+Bootenvironments") == ascii,
        &format!("TERM={term}: {borders} borders"),
    );
    choose(vm, term)?;
    vm.sh_with("true", Sh::default().quiet()).map(drop)
}

/// Types the filter `be` in the menu pilot, chooses the match and records
/// the frame statistics.
fn choose(vm: &mut Vm<'_>, term: &str) -> Result<()> {
    let test = vm.test();
    thread::sleep(Duration::from_secs(1));
    let before = serial_bytes(vm);
    for key in ["b", "e"] {
        vm.send(key)?;
        thread::sleep(Duration::from_secs(1));
    }
    let typed = serial_bytes(vm).saturating_sub(before);
    let start = Instant::now();
    vm.send("\r")?;
    vm.expect1(r"chosen: (\S+)\r*\n", 60)?;
    let chosen = vm.group(1);
    vm.expect1(STATS, 60)?;
    let (frames, mean, slowest) = stats(&vm.group(1), &vm.group(2), &vm.group(3))?;
    test.check(
        chosen == "tplain/ROOT/be",
        &format!("TERM={term}: Enter chooses tplain/ROOT/be ({chosen})"),
    );
    test.note(&format!(
        "TERM={term}: {frames} frames, mean {mean} ms, slowest {slowest} ms; \
         two filter keys took {typed} serial bytes; Enter to answer {:.2} s",
        start.elapsed().as_secs_f64()
    ));
    Ok(())
}

/// The frame statistics as Python writes them: the frames as an integer,
/// the times as floats.
fn stats(frames: &str, mean: &str, slowest: &str) -> Result<(u64, String, String)> {
    let bad = || Error::Setup(format!("bad frame statistics: {frames} {mean} {slowest}"));
    let float = |text: &str| text.parse::<f64>().map(py_float).map_err(|_| bad());
    let frames = frames.parse::<u64>().map_err(|_| bad())?;
    Ok((frames, float(mean)?, float(slowest)?))
}

/// A float as Python's `str` writes it in the range the statistics use:
/// the shortest digits that read back the same, with at least one
/// decimal.
fn py_float(value: f64) -> String {
    let text = value.to_string();
    if text.contains(['.', 'e', 'N', 'i']) {
        text
    } else {
        format!("{text}.0")
    }
}

/// Formats a LUKS volume on a loop device and writes the key files.
fn format_command() -> String {
    format!(
        "if command -v truncate >/dev/null; then truncate -s 32M /tmp/pilot.img; \
         elif command -v fallocate >/dev/null; then fallocate -l 32M /tmp/pilot.img; \
         else head -c 33554432 /dev/zero > /tmp/pilot.img; fi && \
         dev=$(losetup -f --show /tmp/pilot.img) && \
         printf %s {PILOT_KEY} | cryptsetup luksFormat --batch-mode --type luks2 --pbkdf pbkdf2 \
         --pbkdf-force-iterations 1000 --key-file=- \"$dev\" && \
         printf %s {PILOT_KEY} > /tmp/pilot.key && printf wrong > /tmp/wrong.key && echo DEV=$dev"
    )
}

/// `zluks --pilot open` with the key file `key`.
fn open_command(key: &str) -> String {
    format!(
        "zluks --pilot open \"$dev\" zbm-pilot --key-file /tmp/{key}.key \
         --journal /tmp/pilot.jsonl"
    )
}

/// `zluks --pilot open` on a LUKS volume formatted in the VM.
fn unlock(vm: &mut Vm<'_>) -> Result<()> {
    let test = vm.test();
    let (rc, out) = vm.sh_show("command -v losetup && command -v cryptsetup")?;
    if rc != 0 {
        test.note(&format!(
            "pilot 1 skipped: the image lacks losetup or cryptsetup ({out})"
        ));
        return Ok(());
    }
    // The image has no dd; make the 32 MiB file with whichever tool it has.
    let (rc, _) = vm.sh_show(&format_command())?;
    test.check(rc == 0, "a LUKS volume was formatted on a loop device");
    if rc != 0 {
        return Ok(());
    }
    let (rc, out) = vm.sh_show(&open_command("wrong"))?;
    test.check(
        rc == 1 && out.contains("no key slot of"),
        "a wrong key file is refused, with the reason",
    );
    let (rc, out) = vm.sh_show(&open_command("pilot"))?;
    test.check(
        rc == 0 && out.contains("as /dev/mapper/zbm-pilot (key file)"),
        "zluks --pilot open opens the volume with the key file",
    );
    let (rc, _) = vm.sh("test -b /dev/mapper/zbm-pilot")?;
    test.check(rc == 0, "/dev/mapper/zbm-pilot exists");
    let (_, out) = vm.sh_show("cat /tmp/pilot.jsonl")?;
    check_journal(test, &out);
    vm.sh("cryptsetup close zbm-pilot; losetup -d \"$dev\"")
        .map(drop)
}

/// Checks the journal of the two attempts.
fn check_journal(test: &Test, journal: &str) {
    test.check(
        journal.contains(r#""stdin":"secret""#),
        "the journal records that the key went to standard input",
    );
    test.check(
        !journal.contains(PILOT_KEY),
        "the journal does not contain the key",
    );
    test.check(
        journal.contains(r#""message":"refused""#) && journal.contains(r#""message":"opened""#),
        "the journal records the refused and the successful attempt",
    );
}

#[cfg(test)]
mod tests;
