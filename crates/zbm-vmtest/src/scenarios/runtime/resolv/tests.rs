use pretty_assertions::assert_eq;

use super::*;
use crate::testing::{Step, checks, cleanup, scripted, settings};

/// Plays `script` against `steps`, on a terminal as wide as the recovery
/// shell's; the checks it recorded.
fn played(
    name: &str,
    script: Vec<Step>,
    steps: impl FnOnce(&Test, &mut Vm<'_>) -> Result<()>,
) -> Vec<String> {
    let settings = settings(name);
    let test = Test::new(&settings, "runtime-rust-resolv").unwrap();
    let (mut vm, guest) = scripted(&test, script);
    vm.console().set_columns(250);

    steps(&test, &mut vm).unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    let found = checks(&test);
    cleanup(&settings);
    found
}

#[test]
fn the_image_installs_network_manager_and_names_its_dracut_module() {
    let spec = spec();

    assert_eq!(spec.variant, "auto-net");
    assert_eq!(spec.runtime, Some(ZbmRuntime::Rust));
    assert_eq!(spec.packages, ["network-manager"]);
    assert_eq!(spec.conf, "add_dracutmodules+=\" network-manager \"");
    assert_eq!(spec.files[0].0, "hooks/early-setup.d/05-zbmtest-net");
    assert!(HOOK.starts_with("#!/bin/sh\necho \"ZBMTEST-NET addr="));
}

#[test]
fn the_hook_s_line_gives_each_field_and_empty_ones_stay_empty() {
    assert_eq!(
        Net::parse("addr=10.0.2.15/24 nm=done resolv=nameserver 10.0.2.3"),
        Net {
            addr: "10.0.2.15/24".into(),
            nm: "done".into(),
            resolv: "nameserver 10.0.2.3".into(),
        }
    );
    assert_eq!(Net::parse("addr= nm= resolv="), Net::default());
    assert_eq!(
        Net::parse("addr= nm=done resolv=\r"),
        Net {
            nm: "done".into(),
            ..Net::default()
        }
    );
    assert_eq!(Net::parse("something else"), Net::default());
}

#[test]
fn only_an_address_in_qemu_s_network_counts() {
    assert!(is_in_qemu_net("10.0.2.15/24"));
    assert!(is_in_qemu_net("10.0.2.15"));
    assert!(!is_in_qemu_net("10.0.3.15/24"));
    assert!(!is_in_qemu_net("192.168.1.2/24"));
    assert!(!is_in_qemu_net(""));
}

/// The boot up to the countdown, the hook printing `line`.
fn boot(line: &'static str) -> Vec<Step> {
    vec![
        Step::Say("[    1.0] e1000 0000:00:03.0 eth0: (PCI:33MHz:32-bit)\r\n"),
        Step::Say(line),
        Step::Say("Booting tplain/ROOT/be in 30 seconds\r\n"),
    ]
}

#[test]
fn the_network_up_before_start_up_passes_each_check() {
    let found = played(
        "runtime-resolv-net",
        boot("ZBMTEST-NET addr=10.0.2.15/24 nm=done resolv=nameserver 10.0.2.3\r\n"),
        network,
    );

    assert_eq!(
        found,
        [
            "ok   - the early-setup hook prints the network before the countdown",
            "ok   - the network is up before start-up: an address in 10.0.2.0/24 (10.0.2.15/24)",
            "ok   - NetworkManager finished before start-up (done)",
            "ok   - the image's /etc/resolv.conf names QEMU's name server 10.0.2.3 \
             (nameserver 10.0.2.3)",
        ]
    );
}

#[test]
fn an_image_without_resolv_conf_fails_that_check_by_name() {
    let found = played(
        "runtime-resolv-no-file",
        boot("ZBMTEST-NET addr=10.0.2.15/24 nm=done resolv=\r\n"),
        network,
    );

    assert_eq!(
        found[1..],
        [
            "ok   - the network is up before start-up: an address in 10.0.2.0/24 (10.0.2.15/24)",
            "ok   - NetworkManager finished before start-up (done)",
            "FAIL - the image's /etc/resolv.conf names QEMU's name server 10.0.2.3 (none)",
        ]
    );
}

#[test]
fn a_network_down_fails_its_address_and_network_manager_s_end() {
    let found = played(
        "runtime-resolv-down",
        boot("ZBMTEST-NET addr= nm= resolv=nameserver 10.0.2.3\r\n"),
        network,
    );

    assert_eq!(
        found[1..3],
        [
            "FAIL - the network is up before start-up: an address in 10.0.2.0/24 (none)",
            "FAIL - NetworkManager finished before start-up (none)",
        ]
    );
}

#[test]
fn a_countdown_without_the_hook_s_line_fails_every_check() {
    let found = played(
        "runtime-resolv-no-hook",
        vec![Step::Say("Booting tplain/ROOT/be in 30 seconds\r\n")],
        network,
    );

    assert_eq!(found.len(), 4);
    assert!(
        found.iter().all(|check| check.starts_with("FAIL - ")),
        "{found:?}"
    );
}

/// The question as the console shows it.
const QUESTION: &str = "\x1b[H\x1b[2J[ESCAPE] Enter r/w chroot\r\n[RETURN] Enter r/o chroot\r\n\
    \r\nEntering chroot in 10 seconds";

/// The chroot's notices with the name servers, in their order.
const NOTICES: &str = "\x1b[H\x1b[2J* tplain/ROOT/be is mounted read-only\r\n\
    * /run (in memory) is mounted read/write\r\n\
    * /etc/resolv.conf -> /run/systemd/resolve/stub-resolv.conf is mounted read-only\r\n\
    * /tmp is mounted read/write\r\n\r\ntplain/ROOT/be / > ";

/// The chroot's notices as an image without `/etc/resolv.conf` gives
/// them.
const NO_NAME_SERVERS: &str = "\x1b[H\x1b[2J* tplain/ROOT/be is mounted read-only\r\n\
    * /tmp is mounted read/write\r\n\r\ntplain/ROOT/be / > ";

/// What `grep stub-resolv /proc/mounts` prints in the chroot, the bind
/// read-only, after the prompt the shell drew once the last command ended.
const MOUNTED_RO: &str = "tplain/ROOT/be / > tmpfs /run/systemd/resolve/stub-resolv.conf tmpfs \
    ro,nosuid,nodev,relatime 0 0\r\nRC=0\r\n";

/// Writing `/etc/resolv.conf` in the chroot, on the read-only bind.
const WRITE_REFUSED: &str =
    "tplain/ROOT/be / > sh: can't create /etc/resolv.conf: Read-only file system\r\nRC=1\r\n";

/// `busybox nslookup example.org` with an address, after the prompt.
const ANSWERED: &str = "tplain/ROOT/be / > Server:\t\t10.0.2.3\r\nAddress:\t10.0.2.3:53\r\n\r\n\
    Non-authoritative answer:\r\nName:\texample.org\r\nAddress: 93.184.215.14\r\n\r\nRC=0\r\n";

/// `busybox nslookup example.org` answered by systemd-resolved's stub,
/// after the prompt.
const OTHER_ANSWER: &str = "tplain/ROOT/be / > Server:\t\t127.0.0.53\r\n\
    Address:\t127.0.0.53:53\r\n\r\nName:\texample.org\r\nAddress: 1.2.3.4\r\n\r\nRC=0\r\n";

/// The script of the shell's chroot: its notices are `notices`, the file
/// `resolv`, `/proc/mounts` `mounts`, the write ends with `written`, and
/// the lookup prints `looked_up`. The screen's match of the notices takes
/// the first prompt; each output after the first starts with the next.
fn chroot_script(
    notices: &'static str,
    resolv: &'static str,
    mounts: &'static str,
    written: &'static str,
    looked_up: &'static str,
) -> Vec<Step> {
    vec![
        Step::Hear(SHELL_CHROOT),
        Step::Say(QUESTION),
        Step::Keys("\r"),
        Step::Say(notices),
        Step::Hear("cat /etc/resolv.conf; echo RC=$?"),
        Step::Say(resolv),
        Step::Hear("grep stub-resolv /proc/mounts; echo RC=$?"),
        Step::Say(mounts),
        Step::Hear("echo x > /etc/resolv.conf; echo RC=$?"),
        Step::Say(written),
        Step::Hear("busybox nslookup example.org; echo RC=$?"),
        Step::Say(looked_up),
        Step::Hear("exit"),
        Step::Say("ZFS-CHROOT-RC=0\r\nzbm# "),
    ]
}

#[test]
fn the_chroot_binds_the_image_s_name_servers_read_only_and_a_name_resolves() {
    let found = played(
        "runtime-resolv-chroot",
        chroot_script(
            NOTICES,
            "nameserver 10.0.2.3\r\nRC=0\r\n",
            MOUNTED_RO,
            WRITE_REFUSED,
            ANSWERED,
        ),
        chroot,
    );

    assert_eq!(
        found,
        [
            "ok   - zfs-chroot asks first on a pool imported read-only",
            "ok   - the chroot says its /run is in memory, read/write: the link's target is \
             missing there",
            "ok   - the chroot says the image's /etc/resolv.conf is mounted read-only on the \
             link's target",
            "ok   - the notices come in order: /run, /etc/resolv.conf, then /tmp",
            "ok   - in the chroot, /etc/resolv.conf names 10.0.2.3 (0)",
            "ok   - in the chroot, /proc/mounts shows the link's target mounted read-only (0)",
            "ok   - in the chroot, writing /etc/resolv.conf fails: Read-only file system (1)",
            "ok   - in the chroot, busybox nslookup example.org gets a reply from 10.0.2.3 \
             (an address)",
            "ok   - zfs-chroot ends with 0 once its shell ended",
        ]
    );
}

#[test]
fn a_chroot_without_the_image_s_name_servers_fails_where_they_are_missing() {
    let found = played(
        "runtime-resolv-chroot-none",
        chroot_script(
            NO_NAME_SERVERS,
            "cat: can't open '/etc/resolv.conf': No such file or directory\r\nRC=1\r\n",
            "tplain/ROOT/be / > RC=1\r\n",
            "tplain/ROOT/be / > sh: can't create /etc/resolv.conf: nonexistent directory\r\n\
             RC=1\r\n",
            "tplain/ROOT/be / > ;; connection timed out; no servers could be reached\r\n\r\n\
             RC=1\r\n",
        ),
        chroot,
    );

    assert_eq!(
        found,
        [
            "ok   - zfs-chroot asks first on a pool imported read-only",
            "FAIL - the chroot says its /run is in memory, read/write: the link's target is \
             missing there",
            "FAIL - the chroot says the image's /etc/resolv.conf is mounted read-only on the \
             link's target",
            "FAIL - the notices come in order: /run, /etc/resolv.conf, then /tmp",
            "FAIL - in the chroot, /etc/resolv.conf names 10.0.2.3 (1)",
            "FAIL - in the chroot, /proc/mounts shows the link's target mounted read-only (1)",
            "FAIL - in the chroot, writing /etc/resolv.conf fails: Read-only file system (1)",
            "FAIL - in the chroot, busybox nslookup example.org gets a reply from 10.0.2.3 \
             (none: QEMU forwards it to this host's resolver, which did not answer)",
            "ok   - zfs-chroot ends with 0 once its shell ended",
        ]
    );
}

#[test]
fn the_chroot_s_checks_pass_with_the_commands_echoed_after_the_prompt() {
    let found = played(
        "runtime-resolv-chroot-echo",
        chroot_script(
            NOTICES,
            "cat /etc/resolv.conf; echo RC=$?\r\nnameserver 10.0.2.3\r\nRC=0\r\n",
            "tplain/ROOT/be / > grep stub-resolv /proc/mounts; echo RC=$?\r\n\
             tmpfs /run/systemd/resolve/stub-resolv.conf tmpfs ro,relatime 0 0\r\nRC=0\r\n",
            "tplain/ROOT/be / > echo x > /etc/resolv.conf; echo RC=$?\r\n\
             sh: can't create /etc/resolv.conf: Read-only file system\r\nRC=1\r\n",
            "tplain/ROOT/be / > busybox nslookup example.org; echo RC=$?\r\n\
             Server:\t\t10.0.2.3\r\nAddress:\t10.0.2.3:53\r\n\r\n\
             ** server can't find example.org: NXDOMAIN\r\n\r\nRC=1\r\n",
        ),
        chroot,
    );

    assert_eq!(
        found[4..8],
        [
            "ok   - in the chroot, /etc/resolv.conf names 10.0.2.3 (0)",
            "ok   - in the chroot, /proc/mounts shows the link's target mounted read-only (0)",
            "ok   - in the chroot, writing /etc/resolv.conf fails: Read-only file system (1)",
            "ok   - in the chroot, busybox nslookup example.org gets a reply from 10.0.2.3 \
             (NXDOMAIN)",
        ]
    );
}

#[test]
fn a_writable_bind_fails_the_mount_and_write_checks() {
    let found = played(
        "runtime-resolv-chroot-rw",
        chroot_script(
            NOTICES,
            "nameserver 10.0.2.3\r\nRC=0\r\n",
            "tplain/ROOT/be / > tmpfs /run/systemd/resolve/stub-resolv.conf tmpfs rw,relatime \
             0 0\r\nRC=0\r\n",
            "tplain/ROOT/be / > RC=0\r\n",
            "tplain/ROOT/be / > Server:\t\t10.0.2.3\r\nAddress:\t10.0.2.3:53\r\n\r\n\
             ** server can't find example.org: SERVFAIL\r\n\r\nRC=1\r\n",
        ),
        chroot,
    );

    assert_eq!(
        found[5..8],
        [
            "FAIL - in the chroot, /proc/mounts shows the link's target mounted read-only (0)",
            "FAIL - in the chroot, writing /etc/resolv.conf fails: Read-only file system (0)",
            "ok   - in the chroot, busybox nslookup example.org gets a reply from 10.0.2.3 \
             (SERVFAIL)",
        ]
    );
}

#[test]
fn a_busybox_without_nslookup_is_named_as_such() {
    let found = played(
        "runtime-resolv-no-applet",
        vec![
            Step::Hear("busybox nslookup example.org; echo RC=$?"),
            Step::Say("nslookup: applet not found\r\nRC=127\r\n"),
        ],
        lookup,
    );

    assert_eq!(
        found,
        [
            "FAIL - in the chroot, busybox nslookup example.org gets a reply from 10.0.2.3 \
          (none: the environment's busybox has no nslookup)"
        ]
    );
}

#[test]
fn a_reply_from_another_server_is_named_as_such() {
    let found = played(
        "runtime-resolv-other-server",
        vec![
            Step::Hear("busybox nslookup example.org; echo RC=$?"),
            Step::Say(OTHER_ANSWER),
        ],
        lookup,
    );

    assert_eq!(
        found,
        [
            "FAIL - in the chroot, busybox nslookup example.org gets a reply from 10.0.2.3 \
             (another server replied: the chroot's /etc/resolv.conf is not the image's)"
        ]
    );
}

#[test]
fn the_last_notice_is_the_one_the_chroot_s_wait_matches() {
    assert_eq!(tmp_notice(), "/tmp is mounted read/write");
    assert_eq!(CHROOT_NOTICES, format!("{SCREEN}{}", tmp_notice()));
}

#[test]
fn the_chroot_s_prompt_and_echo_leave_the_output() {
    assert_eq!(
        without_prompt(
            "tplain/ROOT/be / > grep x /proc/mounts; echo RC=$?\ntmpfs /x tmpfs ro 0 0\n\
             tplain/ROOT/be / > ",
            "grep x /proc/mounts"
        ),
        "tmpfs /x tmpfs ro 0 0"
    );
    assert_eq!(
        without_prompt("tplain/ROOT/be / > a line\nanother", "true"),
        "a line\nanother"
    );
}

#[test]
fn notices_out_of_order_fail_the_order_check() {
    let screen = "* /etc/resolv.conf -> x is mounted read-only\n* /run (in memory) is mounted \
                  read/write\n* /tmp is mounted read/write";

    assert!(!is_in_order(
        screen,
        &["/run (in memory)", "/etc/resolv.conf", "/tmp"]
    ));
    assert!(is_in_order(
        screen,
        &["/etc/resolv.conf", "/run (in memory)", "/tmp"]
    ));
    assert!(!is_in_order(screen, &["/etc/resolv.conf", "/nothing"]));
}

#[test]
fn every_line_of_the_stub_must_be_read_only() {
    assert!(is_read_only(
        "tmpfs /run/systemd/resolve/stub-resolv.conf tmpfs ro,nosuid 0 0"
    ));
    // Only the mount point's line counts, wherever it starts.
    assert!(is_read_only(
        "tplain/ROOT/be / > tmpfs /run/systemd/resolve/stub-resolv.conf tmpfs ro 0 0\n\
         grep stub-resolv /proc/mounts; echo RC=$?"
    ));
    assert!(!is_read_only(
        "x > tmpfs /run/systemd/resolve/stub-resolv.conf tmpfs rw,relatime 0 0"
    ));
    assert!(!is_read_only("tmpfs /run/systemd/resolve/stub-resolv.conf"));
    assert!(!is_read_only(
        "tmpfs /run/systemd/resolve/stub-resolv.conf tmpfs ro 0 0\n\
         tmpfs /run/systemd/resolve/stub-resolv.conf tmpfs rw 0 0"
    ));
    assert!(!is_read_only("proc /proc proc ro 0 0"));
    assert!(!is_read_only(""));
}

#[test]
fn any_reply_from_qemu_s_name_server_counts_and_only_that() {
    assert_eq!(
        reply(&without_prompt(ANSWERED, LOOKUP)).as_deref(),
        Ok("an address")
    );
    assert_eq!(
        reply(
            "Server:\t\t10.0.2.3\nAddress:\t10.0.2.3:53\n\n\
               ** server can't find example.org: NXDOMAIN\n"
        )
        .as_deref(),
        Ok("NXDOMAIN")
    );
    // Without a `Server:` line, as busybox may print only once it has a
    // reply.
    assert_eq!(
        reply("** server can't find example.org: SERVFAIL").as_deref(),
        Ok("SERVFAIL")
    );
    assert_eq!(
        reply(";; connection timed out; no servers could be reached"),
        Err(NO_REPLY)
    );
    // The `Address:` of the server alone is no answer.
    assert_eq!(
        reply("Server:\t\t10.0.2.3\nAddress:\t10.0.2.3:53\n"),
        Err(NO_REPLY)
    );
    // A reply from another server is not this chroot's.
    assert_eq!(
        reply(
            "Server:\t\t127.0.0.53\nAddress:\t127.0.0.53:53\n\n\
             Name:\texample.org\nAddress: 1.2.3.4"
        ),
        Err(OTHER_SERVER)
    );
    assert_eq!(reply("nslookup: applet not found"), Err(NO_APPLET));
}

/// The script of the checks after the chroot: the mounts left are
/// `left`, the link reads `link`, and `/run` holds `run`.
fn after_script(left: &'static str, link: &'static str, run: &'static str) -> Vec<Step> {
    vec![
        Step::Hear(
            "grep -E 'resolv|/zfsbootmenu/environments/tplain/ROOT/be/mnt' /proc/self/mounts; \
             echo RC=$?",
        ),
        Step::Say(left),
        Step::Hear(
            "mkdir -p /tmp/be && mount -t zfs -o ro,zfsutil tplain/ROOT/be /tmp/be; echo RC=$?",
        ),
        Step::Say("RC=0\r\n"),
        Step::Hear("readlink /tmp/be/etc/resolv.conf; echo RC=$?"),
        Step::Say(link),
        Step::Hear("ls -A /tmp/be/run; echo RC=$?"),
        Step::Say(run),
        Step::Hear("umount /tmp/be; echo RC=$?"),
        Step::Say("RC=0\r\n"),
    ]
}

#[test]
fn after_the_chroot_nothing_is_left_and_the_environment_is_unchanged() {
    let found = played(
        "runtime-resolv-after",
        after_script(
            "RC=1\r\n",
            "../run/systemd/resolve/stub-resolv.conf\r\nRC=0\r\n",
            "RC=0\r\n",
        ),
        after_chroot,
    );

    assert_eq!(
        found,
        [
            "ok   - after the chroot, nothing it mounted is left (1)",
            "ok   - the environment's /etc/resolv.conf is still its link (0)",
            "ok   - the environment's /run is still empty (0)",
        ]
    );
}

#[test]
fn a_mount_left_and_a_file_made_in_the_environment_fail() {
    let found = played(
        "runtime-resolv-after-left",
        after_script(
            "tmpfs /zfsbootmenu/environments/tplain/ROOT/be/mnt/run tmpfs rw 0 0\r\nRC=0\r\n",
            "/etc/static-resolv.conf\r\nRC=0\r\n",
            "systemd\r\nRC=0\r\n",
        ),
        after_chroot,
    );

    assert_eq!(
        found,
        [
            "FAIL - after the chroot, nothing it mounted is left (0)",
            "FAIL - the environment's /etc/resolv.conf is still its link (0)",
            "FAIL - the environment's /run is still empty (0)",
        ]
    );
}
