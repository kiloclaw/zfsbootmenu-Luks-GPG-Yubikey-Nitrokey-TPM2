//! `runtime-rust-resolv`: names resolve in a chroot with ZFSBootMenu's name
//! servers (ROADMAP.md SB15). One boot of the Rust image `auto-net`, built
//! with NetworkManager (`build-init.sh -p network-manager`, the way the
//! docs give users) and dracut's `network-manager` module, on a machine with
//! QEMU's user network and `ip=dhcp rd.neednet=1`, with d0 (`tplain`), whose
//! environment's `/etc/resolv.conf` is systemd-resolved's link into its
//! empty `/run` (`runtime-setup`):
//!
//! - before the countdown, an early-setup hook prints what start-up found:
//!   an address in QEMU's network `10.0.2.0/24`, NetworkManager finished,
//!   and the image's `/etc/resolv.conf` naming QEMU's name server
//!   `10.0.2.3`; each is a check of its own, so that an image without the
//!   file is named as such;
//! - the recovery shell's `zfs-chroot tplain/ROOT/be`, read-only: its
//!   notices say `/run` is in memory and the image's `/etc/resolv.conf` is
//!   mounted read-only on the link's target; in the chroot, the file names
//!   `10.0.2.3`, `/proc/mounts` shows it read-only, writing it fails on a
//!   read-only file system, and `busybox nslookup example.org` gets a reply
//!   from `10.0.2.3`: an address, or an answer such as NXDOMAIN or
//!   SERVFAIL, as QEMU forwards the question to this host's resolver; its
//!   output goes into the notes;
//! - after the chroot, nothing it mounted is left, and the environment's
//!   link and its empty `/run` are as they were.
//!
//! The menu's chroot is the same code; `runtime-rust-recovery` opens it.

use std::net::Ipv4Addr;

use super::recovery::{CHROOT_NOTICES, CHROOT_QUESTION, SHELL_CHROOT};
use super::setup::RESOLV_LINK;
use crate::error::Result;
use crate::expect::SCREEN;
use crate::images::{ImageSpec, zbm_image};
use crate::qemu::Machine;
use crate::record::Test;
use crate::settings::ZbmRuntime;
use crate::vm::{Sh, Vm};

/// An early-setup hook that prints the image's network as start-up finds
/// it: its first address, whether NetworkManager finished, and the first
/// name server of its `/etc/resolv.conf`.
const HOOK: &str = r#"#!/bin/sh
echo "ZBMTEST-NET addr=$(ip -4 -o addr show scope global | awk '{print $4}' | head -n1)" \
  "nm=$([ -e /tmp/nm.done ] && echo done)" \
  "resolv=$(grep -m1 '^nameserver' /etc/resolv.conf 2>/dev/null)"
"#;

/// The hook's line, to its end.
const NET_LINE: &str = r"ZBMTEST-NET ([^\r\n]*)\r*\n";

/// What the machine adds to the command line: the network by DHCP,
/// before start-up.
const NET_APPEND: &str = "ip=dhcp rd.neednet=1";

/// QEMU's name server on its user network.
const NAME_SERVER: &str = "10.0.2.3";

/// How long the boot may take to the hook's line, in seconds.
const PATIENCE: u64 = 1200;

/// The notice of the chroot's `/run` in memory.
const RUN_NOTICE: &str = "/run (in memory) is mounted read/write";

/// The notice of the image's name servers on the link's target.
const RESOLV_NOTICE: &str =
    "/etc/resolv.conf -> /run/systemd/resolve/stub-resolv.conf is mounted read-only";

/// The chroot shell's prompt, as the console's text shows it: busybox's
/// `sh` prints it before each command's output.
const CHROOT_PROMPT: &str = "tplain/ROOT/be / > ";

/// The link's target, where the image's file is mounted.
const STUB: &str = "/run/systemd/resolve/stub-resolv.conf";

/// What writing a file on a read-only mount prints.
const EROFS: &str = "Read-only file system";

/// The name looked up in the chroot.
const LOOKUP: &str = "busybox nslookup example.org";

/// Why a lookup without a reply fails: the name server is QEMU's, which
/// asks the host's.
const NO_REPLY: &str = "none: QEMU forwards it to this host's resolver, which did not answer";

/// Why a lookup fails when the environment's busybox has no `nslookup`.
const NO_APPLET: &str = "none: the environment's busybox has no nslookup";

/// Why a lookup fails when another name server replied.
const OTHER_SERVER: &str =
    "another server replied: the chroot's /etc/resolv.conf is not the image's";

/// Where the chroot mounts `tplain`'s environment.
const MOUNT_POINT: &str = "/zfsbootmenu/environments/tplain/ROOT/be/mnt";

/// Boots with the network up, then resolves a name in the shell's chroot.
///
/// # Errors
///
/// When a step of the test cannot be done.
pub(super) fn run(test: &Test) -> Result<()> {
    let settings = test.settings();
    let image = zbm_image(settings, &spec())?;
    let append = format!("{} {NET_APPEND}", super::ESCAPE_APPEND);
    let machine = Machine {
        net: Some("hostname=zbmtest".into()),
        ..Machine::boot(&image, &append).with_disks([super::disk(settings, 0)?])
    };
    let mut vm = Vm::start(test, "resolv", &machine)?;
    network(test, &mut vm)?;
    super::escape_to_shell(&mut vm)?;
    chroot(test, &mut vm)?;
    after_chroot(test, &mut vm)?;
    vm.kill();
    Ok(())
}

/// The image: the Rust runtime, NetworkManager installed in the build's
/// container and dracut's module for it named, so that a build without it
/// fails, and the hook [`HOOK`].
fn spec() -> ImageSpec {
    ImageSpec {
        runtime: Some(ZbmRuntime::Rust),
        packages: vec!["network-manager".into()],
        conf: "add_dracutmodules+=\" network-manager \"".into(),
        files: vec![(
            "hooks/early-setup.d/05-zbmtest-net".into(),
            HOOK.as_bytes().to_vec(),
        )],
        ..ImageSpec::new("auto-net")
    }
}

/// The network as the hook printed it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Net {
    /// The first global IPv4 address, with its prefix length.
    addr: String,
    /// `done` once NetworkManager finished.
    nm: String,
    /// The first `nameserver` line of the image's `/etc/resolv.conf`.
    resolv: String,
}

impl Net {
    /// The fields of the hook's line, after `ZBMTEST-NET `; a field that is
    /// not there is empty.
    fn parse(line: &str) -> Net {
        let rest = line.trim().strip_prefix("addr=").unwrap_or_default();
        let (addr, rest) = rest.split_once(" nm=").unwrap_or((rest, ""));
        let (nm, resolv) = rest.split_once(" resolv=").unwrap_or((rest, ""));
        Net {
            addr: addr.trim().to_owned(),
            nm: nm.trim().to_owned(),
            resolv: resolv.trim().to_owned(),
        }
    }
}

/// Reads the hook's line, which comes before the countdown, and checks
/// that the network was up before start-up ran.
///
/// # Errors
///
/// When neither the line nor the countdown comes.
fn network(test: &Test, vm: &mut Vm<'_>) -> Result<()> {
    test.step("before the countdown: the network as start-up finds it");
    let patterns = [NET_LINE, super::COUNTDOWN, super::MENU_COUNTDOWN];
    let seen = vm.expect(&patterns, PATIENCE)? == 0;
    let net = if seen {
        Net::parse(&vm.group(1))
    } else {
        Net::default()
    };
    test.check(
        seen,
        "the early-setup hook prints the network before the countdown",
    );
    test.check(
        is_in_qemu_net(&net.addr),
        &format!(
            "the network is up before start-up: an address in 10.0.2.0/24 ({})",
            shown(&net.addr)
        ),
    );
    test.check(
        net.nm == "done",
        &format!(
            "NetworkManager finished before start-up ({})",
            shown(&net.nm)
        ),
    );
    test.check(
        net.resolv == format!("nameserver {NAME_SERVER}"),
        &format!(
            "the image's /etc/resolv.conf names QEMU's name server {NAME_SERVER} ({})",
            shown(&net.resolv)
        ),
    );
    Ok(())
}

/// `text`, or `none` when it is empty.
fn shown(text: &str) -> &str {
    if text.is_empty() { "none" } else { text }
}

/// Whether `addr`, an IPv4 address with or without its prefix length, is in
/// QEMU's user network, `10.0.2.0/24`.
fn is_in_qemu_net(addr: &str) -> bool {
    addr.split('/')
        .next()
        .and_then(|ip| ip.parse::<Ipv4Addr>().ok())
        .is_some_and(|ip| matches!(ip.octets(), [10, 0, 2, _]))
}

/// The recovery shell's chroot of `tplain`'s environment, read-only: its
/// notices, the name servers in it, and `exit`, after which `zfs-chroot`
/// ends with 0.
///
/// # Errors
///
/// When a step cannot be done.
fn chroot(test: &Test, vm: &mut Vm<'_>) -> Result<()> {
    test.step("zfs-chroot tplain/ROOT/be: the image's name servers in the chroot");
    vm.sendline(SHELL_CHROOT)?;
    let asked = vm.expect1(CHROOT_QUESTION, 120).is_ok();
    test.check(
        asked && vm.console().screen().contains("Enter r/o chroot"),
        "zfs-chroot asks first on a pool imported read-only",
    );
    test.step("Enter: the chroot, read-only");
    vm.send("\r")?;
    let noticed = vm.expect1(CHROOT_NOTICES, 120).is_ok();
    let screen = vm.console().screen();
    test.check(
        noticed && screen.contains(RUN_NOTICE),
        "the chroot says its /run is in memory, read/write: the link's target is missing there",
    );
    test.check(
        noticed && screen.contains(RESOLV_NOTICE),
        "the chroot says the image's /etc/resolv.conf is mounted read-only on the link's target",
    );
    test.check(
        is_in_order(&screen, &[RUN_NOTICE, RESOLV_NOTICE, tmp_notice()]),
        "the notices come in order: /run, /etc/resolv.conf, then /tmp",
    );
    in_the_chroot(test, vm)?;
    test.step("exit: back to the recovery shell");
    vm.sendline("exit")?;
    let ended = vm.expect1("ZFS-CHROOT-RC=0", 120).is_ok();
    test.check(ended, "zfs-chroot ends with 0 once its shell ended");
    Ok(())
}

/// The last notice, which [`CHROOT_NOTICES`] waits for on the screen.
fn tmp_notice() -> &'static str {
    CHROOT_NOTICES
        .strip_prefix(SCREEN)
        .unwrap_or(CHROOT_NOTICES)
}

/// Whether each of `notices` is on `screen`, each after the one before.
fn is_in_order(screen: &str, notices: &[&str]) -> bool {
    let places: Option<Vec<usize>> = notices.iter().map(|notice| screen.find(notice)).collect();
    places.is_some_and(|places| places.is_sorted())
}

/// In the chroot's shell: the file, its mount, a write and a lookup.
///
/// # Errors
///
/// When a command does not end in time.
fn in_the_chroot(test: &Test, vm: &mut Vm<'_>) -> Result<()> {
    let (code, said) = in_chroot(vm, "cat /etc/resolv.conf", Sh::default().show())?;
    test.check(
        code == 0 && said.contains(&format!("nameserver {NAME_SERVER}")),
        &format!("in the chroot, /etc/resolv.conf names {NAME_SERVER} ({code})"),
    );
    let (code, said) = in_chroot(vm, "grep stub-resolv /proc/mounts", Sh::default().show())?;
    test.check(
        code == 0 && is_read_only(&said),
        &format!("in the chroot, /proc/mounts shows the link's target mounted read-only ({code})"),
    );
    let (code, said) = in_chroot(vm, "echo x > /etc/resolv.conf", Sh::default().show())?;
    test.check(
        code != 0 && said.contains(EROFS),
        &format!("in the chroot, writing /etc/resolv.conf fails: {EROFS} ({code})"),
    );
    lookup(test, vm)
}

/// Runs `command` in the chroot's shell as `options` say: its exit status
/// and output, without the shell's prompt and the command as echoed.
///
/// # Errors
///
/// When the command does not end in time.
fn in_chroot(vm: &mut Vm<'_>, command: &str, options: Sh) -> Result<(i32, String)> {
    let (code, said) = vm.sh_with(command, options)?;
    Ok((code, without_prompt(&said, command)))
}

/// `said` without [`CHROOT_PROMPT`] at the start of its lines, and without
/// the line of `command` as the shell echoed it.
fn without_prompt(said: &str, command: &str) -> String {
    let sent = format!("{command}; echo RC=$?");
    let lines: Vec<&str> = said
        .lines()
        .map(|line| line.strip_prefix(CHROOT_PROMPT).unwrap_or(line))
        .filter(|line| !line.trim().is_empty() && line.trim_end() != sent)
        .collect();
    lines.join("\n")
}

/// Whether every line of `mounts` whose mount point is [`STUB`] is mounted
/// read-only, and there is one.
fn is_read_only(mounts: &str) -> bool {
    let options: Vec<&str> = mounts
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let at = fields.iter().position(|field| *field == STUB)?;
            Some(fields.get(at + 2).copied().unwrap_or_default())
        })
        .collect();
    !options.is_empty()
        && options
            .iter()
            .all(|options| options.split(',').next() == Some("ro"))
}

/// Looks up a name in the chroot, which must get a reply from QEMU's name
/// server, whatever it says; its output goes into the notes.
///
/// # Errors
///
/// When the command does not end in time.
fn lookup(test: &Test, vm: &mut Vm<'_>) -> Result<()> {
    let (code, said) = in_chroot(vm, LOOKUP, Sh::default().timeout(120))?;
    test.note(&format!("{LOOKUP} ({code}):\n{said}"));
    let answer = reply(&said);
    let outcome = match &answer {
        Ok(answer) => answer.as_str(),
        Err(why) => why,
    };
    test.check(
        answer.is_ok(),
        &format!("in the chroot, {LOOKUP} gets a reply from {NAME_SERVER} ({outcome})"),
    );
    Ok(())
}

/// What `busybox nslookup` printed of a reply from [`NAME_SERVER`]: `an
/// address`, or the answer of a server that found none, such as
/// `NXDOMAIN` or `SERVFAIL`; or why there is none: no `nslookup`
/// ([`NO_APPLET`]), a reply from another server ([`OTHER_SERVER`]), or no
/// reply ([`NO_REPLY`]).
fn reply(said: &str) -> std::result::Result<String, &'static str> {
    if said.contains("applet not found") {
        return Err(NO_APPLET);
    }
    let lines: Vec<&str> = said.lines().map(str::trim).collect();
    let server = lines.iter().find_map(|line| line.strip_prefix("Server:"));
    if server.is_some_and(|server| server.trim() != NAME_SERVER) {
        return Err(OTHER_SERVER);
    }
    let found_none = lines
        .iter()
        .find_map(|line| line.strip_prefix("** server can't find "))
        .and_then(|rest| rest.rsplit_once(": "))
        .map(|(_, answer)| answer.trim().to_owned());
    if let Some(answer) = found_none {
        return Ok(answer);
    }
    let answered = lines
        .iter()
        .skip_while(|line| !line.starts_with("Name:"))
        .skip(1)
        .any(|line| line.starts_with("Address"));
    if answered {
        Ok("an address".to_owned())
    } else {
        Err(NO_REPLY)
    }
}

/// In the recovery shell, after the chroot: nothing it mounted is left,
/// and the environment, mounted read-only on `/tmp/be` meanwhile, still
/// has its link and an empty `/run`.
///
/// # Errors
///
/// When a command does not end in time, or the environment cannot be
/// mounted or unmounted.
fn after_chroot(test: &Test, vm: &mut Vm<'_>) -> Result<()> {
    test.step("after the chroot: its mounts gone, the environment as it was");
    let (code, _) = vm.sh_show(&format!("grep -E 'resolv|{MOUNT_POINT}' /proc/self/mounts"))?;
    test.check(
        code == 1,
        &format!("after the chroot, nothing it mounted is left ({code})"),
    );
    vm.must("mkdir -p /tmp/be && mount -t zfs -o ro,zfsutil tplain/ROOT/be /tmp/be")?;
    let (code, said) = vm.sh_show("readlink /tmp/be/etc/resolv.conf")?;
    test.check(
        code == 0 && said.lines().any(|line| line.trim() == RESOLV_LINK),
        &format!("the environment's /etc/resolv.conf is still its link ({code})"),
    );
    let (code, said) = vm.sh_show("ls -A /tmp/be/run")?;
    test.check(
        code == 0 && said.trim().is_empty(),
        &format!("the environment's /run is still empty ({code})"),
    );
    vm.must("umount /tmp/be").map(drop)
}

#[cfg(test)]
mod tests;
