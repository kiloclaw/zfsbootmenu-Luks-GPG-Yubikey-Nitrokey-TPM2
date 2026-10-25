//! No secret outlives its use (ROADMAP.md #1): after the Rust runtime has
//! unlocked a LUKS volume, no process, command line, environment or file in
//! memory holds the passphrase or the key file.
//!
//! Disk: s0, one LUKS2 volume with two key slots, a long passphrase
//! ([`PASSPHRASE`]) and a key file ([`KEY_FILE`]), both unlike anything else
//! in memory; it holds the pool `tsecret` with the test boot environment.
//!
//! Scenarios:
//!
//! - `setup`: creates the volume and the pool in the recovery shell;
//! - `passphrase`: the passphrase is typed at ZFSBootMenu's prompt, in the
//!   unlock dialog of the start-up view, which reads keys into buffers it
//!   wipes;
//! - `keyfile`: `zbm.luks_key` names the key file on a FAT USB stick: no
//!   prompt.
//!
//! The Rust image is built with the `secret-test` feature and a teardown
//! hook, which runs after the volume was unlocked, the pool imported and
//! the kernel loaded, just before `kexec`: `zbm debug secrets` searches the
//! machine for both secrets, given in hexadecimal. The machine boots with
//! `init_on_free=1`, which the kernel must confirm.

use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

use crate::boot::{dialogs, prompt};
use crate::disks::vfat_disk;
use crate::error::{Error, Result};
use crate::expect::regex;
use crate::images::{ImageSpec, zbm_image};
use crate::qemu::{Disk, Machine};
use crate::record::Test;
use crate::settings::{Settings, ZbmRuntime};
use crate::vm::Vm;

/// The disk group.
const GROUP: &str = "secrets";

/// The passphrase of the volume.
pub const PASSPHRASE: &str = "zbmtest-Passphrase-7Kq2Vx9Lm4Rt";

/// The key file of the volume's second key slot.
pub const KEY_FILE: &str = "zbmtest-KeyFile-3Hs8Wz1Pd6Nc";

/// The key derivation of the key slots: quick, as the test needs no
/// strength.
const PBKDF: &str = "--pbkdf pbkdf2 --pbkdf-force-iterations 1000";

/// The kernel command line of the scenarios, before the volume.
const APPEND: &str = "console=ttyS0 loglevel=7 zbm.timeout=3 init_on_free=1";

/// The disk s0.
fn disk(settings: &Settings, fresh: bool) -> Result<Disk> {
    let path = crate::disks::disk(settings, GROUP, 0, "384M", fresh)?;
    Ok(Disk::new(path, "s0"))
}

/// Where `setup` records the volume's UUID.
fn uuid_file(settings: &Settings) -> PathBuf {
    settings.disks(GROUP).join("uuid")
}

/// `bytes` in hexadecimal.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, byte| {
        write!(out, "{byte:02x}").ok();
        out
    })
}

/// The teardown hook that searches for both secrets.
fn hook() -> String {
    format!(
        "#!/bin/bash\n\
         echo ZBMTEST-SECRETS-BEGIN\n\
         /libexec/zbm debug secrets {} {}\n\
         echo \"ZBMTEST-SECRETS-END $?\"\n\
         echo \"ZBMTEST-MEMINIT $(dmesg | grep -m1 'mem auto-init')\"\n",
        hex(PASSPHRASE.as_bytes()),
        hex(KEY_FILE.as_bytes()),
    )
}

/// Runs `setup`, `passphrase` or `keyfile`.
///
/// # Errors
///
/// When a step of the test cannot be done.
pub fn run(test: &Test, scenario: &str) -> Result<()> {
    match scenario {
        "setup" => setup(test),
        "passphrase" | "keyfile" => {
            let uuid = fs::read_to_string(uuid_file(test.settings())).unwrap_or_default();
            if uuid.trim().is_empty() {
                test.check(false, "run secrets-setup first");
                return Ok(());
            }
            unlock(test, scenario, uuid.trim())
        }
        _ => Err(Error::Setup(format!("unknown scenario {scenario}"))),
    }
}

/// Creates the volume, its second key slot and the pool in the recovery
/// shell.
fn setup(test: &Test) -> Result<()> {
    let settings = test.settings();
    let image = zbm_image(settings, &ImageSpec::new("auto"))?;
    let machine = Machine::boot(&image, "console=ttyS0 zbm.luks=off zbm.retry_delay=3")
        .with_disks([disk(settings, true)?, super::payload(settings)?]);
    let mut vm = Vm::start(test, "setup", &machine)?;
    vm.recovery_shell()?;
    vm.send_functions()?;
    let device = "/dev/disk/by-id/virtio-s0";
    vm.must(&format!("mkluks '{PASSPHRASE}' '{PBKDF}' {device}"))?;
    vm.must(&format!(
        "printf '%s' '{KEY_FILE}' > /tmp/s.key && printf '%s' '{PASSPHRASE}' | \
         cryptsetup luksAddKey {PBKDF} --key-file=- {device} /tmp/s.key && rm /tmp/s.key"
    ))?;
    let (rc, _) = vm.sh(
        "zpool create -f -o ashift=12 -o compatibility=openzfs-2.4-linux -O compression=lz4 \
         -m none tsecret \"${V[0]}\"",
    )?;
    test.check(rc == 0, "zpool create tsecret on the LUKS volume");
    vm.must("mkbe tsecret")?;
    let (_, out) = vm.sh_show("echo UUID=${U[0]}")?;
    let uuid = regex(r"UUID=([0-9a-f-]{36})")?
        .captures(&out)
        .map(|captures| captures[1].to_owned());
    test.check(uuid.is_some(), "volume UUID recorded");
    let path = uuid_file(settings);
    fs::write(&path, uuid.unwrap_or_default())
        .map_err(Error::io(format!("writing {}", path.display())))?;
    vm.must("finish")?;
    vm.poweroff()
}

/// What the hook printed.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Search {
    /// The output of `zbm debug secrets`.
    output: String,
    /// Its exit status.
    status: String,
    /// The kernel's line about initialising memory.
    meminit: String,
}

/// Boots the Rust image, unlocks the volume as `scenario` says and checks
/// the hook's search.
fn unlock(test: &Test, scenario: &str, uuid: &str) -> Result<()> {
    let settings = test.settings();
    let spec = ImageSpec {
        runtime: Some(ZbmRuntime::Rust),
        features: Some("zbm/secret-test".into()),
        files: vec![(
            "hooks/teardown.d/10-zbmtest-secrets".into(),
            hook().into_bytes(),
        )],
        ..ImageSpec::new("auto-secrets")
    };
    let image = zbm_image(settings, &spec)?;
    let mut append = format!("{APPEND} zbm.luks=UUID={uuid}");
    let mut usb = Vec::new();
    if scenario == "keyfile" {
        append.push_str(" zbm.luks_key=/keys/s.key:LABEL=ZBMKEY");
        let stick = settings.disks(GROUP).join("stick.img");
        let files: [(&str, &[u8]); 1] = [("keys/s.key", KEY_FILE.as_bytes())];
        vfat_disk(&stick, "ZBMKEY", &files, "64M", 512)?;
        usb.push((stick, "usb0".to_owned()));
    }
    let machine = Machine {
        usb,
        ..Machine::boot(&image, &append).with_disks([disk(settings, false)?])
    };
    let mut vm = Vm::start(test, scenario, &machine)?;
    let patterns = [prompt::LUKS, dialogs::LUKS, "ZBMTEST-SECRETS-BEGIN"];
    let first = vm.expect(&patterns, 1800)?;
    let asked = first < 2;
    if scenario == "passphrase" {
        test.check(asked, "ZFSBootMenu asks for the passphrase");
        if asked {
            vm.sendline(PASSPHRASE)?;
            vm.expect1("ZBMTEST-SECRETS-BEGIN", 1800)?;
        }
    } else {
        test.check(
            !asked,
            "no passphrase prompt: the key file on the stick was used",
        );
    }
    let search = read_search(&mut vm)?;
    vm.kill();
    check_search(test, &search);
    Ok(())
}

/// Reads what the hook printed after its first line.
fn read_search(vm: &mut Vm<'_>) -> Result<Search> {
    vm.expect1(r"ZBMTEST-SECRETS-END (\d+)", 600)?;
    let output = vm.before().trim().to_owned();
    let status = vm.group(1);
    vm.expect1(r"ZBMTEST-MEMINIT ([^\r\n]*)", 60)?;
    Ok(Search {
        output,
        status,
        meminit: vm.group(1),
    })
}

/// Checks the search.
fn check_search(test: &Test, search: &Search) {
    test.note(&search.output);
    test.note(&format!("kernel: {}", search.meminit.trim()));
    let summary = regex(r"searched (\d+) processes, (\d+) mappings and (\d+) files")
        .ok()
        .and_then(|summary| summary.captures(&search.output))
        .map(|captures| {
            let count = |n: usize| captures[n].parse::<u64>().unwrap_or(0);
            (count(1), count(2), count(3))
        });
    test.check(
        summary
            .is_some_and(|(processes, regions, files)| processes > 0 && regions > 0 && files > 0),
        &format!("zbm debug secrets searched the machine ({summary:?})"),
    );
    let found: Vec<&str> = search
        .output
        .lines()
        .filter(|line| line.contains("found secret"))
        .collect();
    test.check(
        found.is_empty() && search.status == "0",
        &format!(
            "no process memory, command line, environment or file in memory holds the \
             passphrase or the key file ({})",
            if found.is_empty() {
                "none found".to_owned()
            } else {
                found.join("; ")
            }
        ),
    );
    test.check(
        search.meminit.contains("heap free:on"),
        "the kernel zeroes freed memory (init_on_free=1)",
    );
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::testing::{Step, checks, cleanup, scripted, settings};

    #[test]
    fn the_hook_names_the_secrets_only_in_hexadecimal() {
        let hook = hook();
        assert!(!hook.contains(PASSPHRASE) && !hook.contains(KEY_FILE));
        assert!(hook.contains(&hex(PASSPHRASE.as_bytes())), "{hook}");
        assert_eq!(hex(b"pw"), "7077");
    }

    #[test]
    fn the_search_is_read_from_the_console() {
        let settings = settings("secrets-read");
        let test = Test::new(&settings, "secrets-passphrase").unwrap();
        let script = vec![Step::Say(
            "found secret 1 in file /run/k\r\nsearched 9 processes, 300 mappings and 800 \
             files; found 1; secrets kept as Secret\r\nZBMTEST-SECRETS-END 1\r\n\
             ZBMTEST-MEMINIT [0.0] mem auto-init: stack:off, heap alloc:on, heap free:on\r\n",
        )];
        let (mut vm, guest) = scripted(&test, script);

        let search = read_search(&mut vm).unwrap();

        guest.finish();
        assert_eq!(search.status, "1");
        assert!(
            search.output.starts_with("found secret 1 in file /run/k"),
            "{search:?}"
        );
        assert!(search.meminit.ends_with("heap free:on"), "{search:?}");
        cleanup(&settings);
    }

    #[test]
    fn a_secret_found_anywhere_fails_the_test() {
        let settings = settings("secrets-checks");
        let test = Test::new(&settings, "secrets-passphrase").unwrap();
        let clean = Search {
            output: "searched 9 processes, 300 mappings and 800 files; found 0".to_owned(),
            status: "0".to_owned(),
            meminit: "mem auto-init: heap free:on".to_owned(),
        };
        check_search(&test, &clean);
        let found = Search {
            output: format!("found secret 0 in process 1 (zbm) memory\n{}", clean.output),
            status: "1".to_owned(),
            meminit: "mem auto-init: heap free:off".to_owned(),
        };
        check_search(&test, &found);
        check_search(&test, &Search::default());

        let results: Vec<bool> = checks(&test)
            .iter()
            .map(|line| line.starts_with("ok"))
            .collect();
        assert_eq!(
            results,
            [true, true, true, true, false, false, false, false, false]
        );
        cleanup(&settings);
    }
}
