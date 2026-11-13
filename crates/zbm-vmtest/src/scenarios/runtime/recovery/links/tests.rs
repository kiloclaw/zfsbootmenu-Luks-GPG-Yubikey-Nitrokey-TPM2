use pretty_assertions::assert_eq;

use super::*;
use crate::expect::regex;
use crate::scenarios::runtime::recovery::CHROOT_NOTICES;
use crate::testing::{Step, checks, cleanup, scripted, settings};

/// The question as the console shows it.
const QUESTION: &str = "\x1b[H\x1b[2J[ESCAPE] Enter r/w chroot\r\n[RETURN] Enter r/o chroot\r\n\
    \r\nEntering chroot in 10 seconds";

/// The chroot's notices, `/tmp` and `/dev` refused, and its prompt.
const REFUSED: &str = "\x1b[H\x1b[2J* tplain/crafted is mounted read-only\r\n\
    * /tmp is not mounted: it is a link to /etc\r\n\
    * /dev is not mounted: it is a link to ../../../../../../../etc/zfs\r\n\
    \r\ntplain/crafted / > ";

/// The notices of a chroot that mounts on the links, as before SB16.
const FOLLOWED: &str = "\x1b[H\x1b[2J* tplain/crafted is mounted read-only\r\n\
    * /tmp is mounted read/write\r\n\r\ntplain/crafted / > ";

/// What the shell and the recovery shell answer in and after the chroot.
#[derive(Clone, Copy)]
struct Answers {
    /// The mounts counted on `/etc` and `/etc/zfs`.
    on_etc: &'static str,
    /// `/proc` and `/sys` counted.
    kernel_fs: &'static str,
    /// The shell's arguments.
    cmdline: &'static str,
    /// The mount over `/`.
    overmount: &'static str,
    /// What is left mounted once it ended.
    left: &'static str,
    /// The filesystem's links read.
    links: &'static str,
}

/// The answers of a chroot that mounts nothing on a link and leaves
/// nothing mounted.
const KEPT: Answers = Answers {
    on_etc: "0\r\nRC=1\r\n",
    kernel_fs: "tplain/crafted / > 2\r\nRC=0\r\n",
    cmdline: "\x1b[0;33mtplain/crafted\x1b[0m / > /bin/sh RC=0\r\n",
    overmount: "tplain/crafted / > RC=0\r\n",
    left: "RC=1\r\n",
    links: "/etc\r\n../../../../../../../etc/zfs\r\nRC=0\r\n",
};

/// The answers of a chroot that followed the links, and left the
/// filesystem mounted below the mount over its `/`.
const BROKEN: Answers = Answers {
    on_etc: "1\r\nRC=0\r\n",
    kernel_fs: "tplain/crafted / > 2\r\nRC=0\r\n",
    cmdline: "tplain/crafted / > /bin/busybox sh RC=0\r\n",
    overmount: "tplain/crafted / > RC=0\r\n",
    left: "tplain/crafted /zfsbootmenu/environments/tplain/crafted/mnt zfs ro 0 0\r\n\
           tmpfs /etc tmpfs rw 0 0\r\nRC=0\r\n",
    links: "/etc\r\n../../../../../../../etc/zfs\r\nRC=0\r\n",
};

/// The script of [`crafted_chroot`]: `question` asked first if any, the
/// notices `notices`, then `answers`.
fn crafted_script(
    question: Option<&'static str>,
    notices: &'static str,
    answers: Answers,
) -> Vec<Step> {
    let mut script = vec![Step::Hear(CRAFTED_CHROOT)];
    if let Some(question) = question {
        script.extend([Step::Say(question), Step::Keys("\r")]);
    }
    script.extend([
        Step::Say(notices),
        Step::Hear("busybox grep -cE '^([^ ]+ ){4}/etc(/zfs)? ' /proc/1/mountinfo; echo RC=$?"),
        Step::Say(answers.on_etc),
        Step::Hear("busybox grep -cE ' /(proc|sys) ' /proc/mounts; echo RC=$?"),
        Step::Say(answers.kernel_fs),
        Step::Hear(r"busybox tr '\0' ' ' < /proc/$$/cmdline; echo RC=$?"),
        Step::Say(answers.cmdline),
        Step::Hear("busybox mount -t tmpfs x /; echo RC=$?"),
        Step::Say(answers.overmount),
        Step::Hear("exit"),
        Step::Say("ZFS-CHROOT-RC=0\r\nzbm# "),
        Step::Hear(
            "grep -E ' /zfsbootmenu/environments/tplain/crafted/| /etc(/zfs)? ' \
             /proc/self/mounts; echo RC=$?",
        ),
        Step::Say(answers.left),
        Step::Hear(
            "mkdir -p /tmp/c && mount -t zfs -o ro,zfsutil tplain/crafted /tmp/c; \
             echo RC=$?",
        ),
        Step::Say("RC=0\r\n"),
        Step::Hear("readlink /tmp/c/tmp /tmp/c/dev; echo RC=$?"),
        Step::Say(answers.links),
        Step::Hear("umount /tmp/c; echo RC=$?"),
        Step::Say("RC=0\r\n"),
    ]);
    script
}

/// Plays `script` against [`crafted_chroot`]; the checks it recorded.
fn crafted_checks(name: &str, script: Vec<Step>) -> Vec<String> {
    let settings = settings(name);
    let test = Test::new(&settings, "runtime-rust-recovery").unwrap();
    let (mut vm, guest) = scripted(&test, script);

    crafted_chroot(&test, &mut vm).unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    let found = checks(&test);
    cleanup(&settings);
    found
}

/// What [`crafted_chroot`] checks when all is well.
const ALL_WELL: [&str; 12] = [
    "ok   - the chroot says /tmp is not mounted: it is a link to /etc",
    "ok   - the chroot says /dev is not mounted: it is a link to ../../../../../../../etc/zfs",
    "ok   - the chroot's shell opens without /tmp and /dev",
    "ok   - the chroot does not say /tmp is mounted read/write",
    "ok   - in the chroot, nothing is mounted on the image's /etc or /etc/zfs (1)",
    "ok   - in the chroot, /proc and /sys are mounted (0)",
    "ok   - in the chroot, the shell is /bin/sh, found inside the root (0)",
    "ok   - in the chroot, a tmpfs mounts over its own / (0)",
    "ok   - zfs-chroot tplain/crafted ends with 0 once its shell ended",
    "ok   - after the chroot, nothing is mounted below \
     /zfsbootmenu/environments/tplain/crafted, or on /etc or /etc/zfs (1)",
    "ok   - tplain/crafted's tmp is still its link to /etc (0)",
    "ok   - tplain/crafted's dev is still its link to ../../../../../../../etc/zfs (0)",
];

#[test]
fn the_crafted_chroot_mounts_nothing_on_its_links_and_leaves_nothing_mounted() {
    let found = crafted_checks(
        "runtime-recovery-crafted",
        crafted_script(Some(QUESTION), REFUSED, KEPT),
    );

    assert_eq!(found, ALL_WELL);
}

#[test]
fn a_crafted_chroot_that_asks_nothing_is_checked_all_the_same() {
    let found = crafted_checks(
        "runtime-recovery-crafted-unasked",
        crafted_script(None, REFUSED, KEPT),
    );

    assert_eq!(found, ALL_WELL);
}

#[test]
fn a_chroot_that_follows_the_links_and_leaves_its_mounts_fails() {
    let found = crafted_checks(
        "runtime-recovery-crafted-followed",
        crafted_script(Some(QUESTION), FOLLOWED, BROKEN),
    );

    assert_eq!(
        found,
        [
            "FAIL - the chroot says /tmp is not mounted: it is a link to /etc",
            "FAIL - the chroot says /dev is not mounted: it is a link to \
             ../../../../../../../etc/zfs",
            "ok   - the chroot's shell opens without /tmp and /dev",
            "FAIL - the chroot does not say /tmp is mounted read/write",
            "FAIL - in the chroot, nothing is mounted on the image's /etc or /etc/zfs (0)",
            "ok   - in the chroot, /proc and /sys are mounted (0)",
            "FAIL - in the chroot, the shell is /bin/sh, found inside the root (0)",
            "ok   - in the chroot, a tmpfs mounts over its own / (0)",
            "ok   - zfs-chroot tplain/crafted ends with 0 once its shell ended",
            "FAIL - after the chroot, nothing is mounted below \
             /zfsbootmenu/environments/tplain/crafted, or on /etc or /etc/zfs (0)",
            "ok   - tplain/crafted's tmp is still its link to /etc (0)",
            "ok   - tplain/crafted's dev is still its link to ../../../../../../../etc/zfs (0)",
        ]
    );
}

#[test]
fn a_chroot_without_its_kernel_s_file_systems_or_its_links_fails() {
    let answers = Answers {
        on_etc: "busybox: /proc/1/mountinfo: No such file or directory\r\nRC=2\r\n",
        kernel_fs: "tplain/crafted / > 0\r\nRC=1\r\n",
        cmdline: "tplain/crafted / > busybox: can't open '/proc/1/cmdline'\r\nRC=1\r\n",
        overmount: "tplain/crafted / > mount: permission denied\r\nRC=1\r\n",
        links: "readlink: /tmp/c/tmp: Invalid argument\r\nRC=1\r\n",
        ..KEPT
    };

    let found = crafted_checks(
        "runtime-recovery-crafted-bare",
        crafted_script(Some(QUESTION), REFUSED, answers),
    );

    assert_eq!(
        found,
        [
            "ok   - the chroot says /tmp is not mounted: it is a link to /etc",
            "ok   - the chroot says /dev is not mounted: it is a link to \
             ../../../../../../../etc/zfs",
            "ok   - the chroot's shell opens without /tmp and /dev",
            "ok   - the chroot does not say /tmp is mounted read/write",
            "FAIL - in the chroot, nothing is mounted on the image's /etc or /etc/zfs (2)",
            "FAIL - in the chroot, /proc and /sys are mounted (1)",
            "FAIL - in the chroot, the shell is /bin/sh, found inside the root (1)",
            "FAIL - in the chroot, a tmpfs mounts over its own / (1)",
            "ok   - zfs-chroot tplain/crafted ends with 0 once its shell ended",
            "ok   - after the chroot, nothing is mounted below \
             /zfsbootmenu/environments/tplain/crafted, or on /etc or /etc/zfs (1)",
            "FAIL - tplain/crafted's tmp is still its link to /etc (1)",
            "FAIL - tplain/crafted's dev is still its link to ../../../../../../../etc/zfs (1)",
        ]
    );
}

#[test]
fn nothing_runs_in_a_crafted_chroot_whose_shell_did_not_open() {
    let found = crafted_checks(
        "runtime-recovery-crafted-shut",
        vec![
            Step::Hear(CRAFTED_CHROOT),
            Step::Say(QUESTION),
            Step::Keys("\r"),
            Step::Say("zfs-chroot: no shell runs in tplain/crafted\r\nZFS-CHROOT-RC=1\r\nzbm# "),
        ],
    );

    assert_eq!(
        found,
        [
            "FAIL - the chroot says /tmp is not mounted: it is a link to /etc",
            "FAIL - the chroot says /dev is not mounted: it is a link to \
             ../../../../../../../etc/zfs",
            "FAIL - the chroot's shell opens without /tmp and /dev",
            "FAIL - the chroot does not say /tmp is mounted read/write",
        ]
    );
}

#[test]
fn a_crafted_chroot_whose_prompt_is_not_seen_ends_the_run() {
    let settings = settings("runtime-recovery-crafted-unseen");
    let test = Test::new(&settings, "runtime-rust-recovery").unwrap();
    let (mut vm, guest) = scripted(
        &test,
        vec![
            Step::Hear(CRAFTED_CHROOT),
            Step::Say(QUESTION),
            Step::Keys("\r"),
            Step::Say(
                "\x1b[H\x1b[2J* tplain/crafted is mounted read-only\r\n\
                 * /tmp is not mounted: it is a link to /etc\r\n\r\ncrafted # ",
            ),
        ],
    );

    let ended = crafted_chroot(&test, &mut vm);

    assert!(ended.is_err());
    assert_eq!(guest.finish(), Vec::<String>::new());
    assert_eq!(
        checks(&test),
        [
            "ok   - the chroot says /tmp is not mounted: it is a link to /etc",
            "FAIL - the chroot says /dev is not mounted: it is a link to \
             ../../../../../../../etc/zfs",
            "FAIL - the chroot's shell opens without /tmp and /dev",
            "FAIL - the chroot does not say /tmp is mounted read/write",
        ]
    );
    cleanup(&settings);
}

#[test]
fn only_an_exit_status_shows_that_the_chroot_ended() {
    let ended = regex(on_screen(CHROOT_ENDED)).unwrap();

    assert!(ended.is_match("ZFS-CHROOT-RC=1\r\nzbm# "));
    assert!(!ended.is_match(CRAFTED_CHROOT));
}

#[test]
fn the_anchors_are_what_the_chroot_says_of_the_crafted_filesystem() {
    assert_eq!(
        CRAFTED_REFUSED,
        format!("{SCREEN}/tmp is not mounted: it is a link to {TMP_LINK}")
    );
    assert_eq!(
        DEV_REFUSED,
        format!("/dev is not mounted: it is a link to {DEV_LINK}")
    );
    assert_eq!(CRAFTED_PROMPT, format!("{SCREEN}{CRAFTED} / > "));
    assert_eq!(
        CRAFTED_CHROOT,
        format!("zfs-chroot {CRAFTED}; echo ZFS-CHROOT-RC=$?")
    );
    assert_eq!(on_screen(CHROOT_NOTICES), TMP_MOUNTED);
    assert_eq!(
        CRAFTED_STATE,
        format!("/zfsbootmenu/environments/{CRAFTED}")
    );
}

#[test]
fn the_shell_s_arguments_name_bin_sh_after_the_prompt() {
    assert!(is_sh("\x1b[0;33mtplain/crafted\x1b[0m / > /bin/sh "));
    assert!(is_sh("tplain/ROOT/be / > /bin/sh -i "));
    assert!(is_sh("/bin/sh"));
    assert!(!is_sh("tplain/ROOT/be / > /bin/busybox sh "));
    assert!(!is_sh("tplain/ROOT/be / > /bin/shell "));
    assert!(!is_sh(""));
}

#[test]
fn an_answer_is_a_whole_line_after_the_prompt() {
    assert!(has_answer("tplain/crafted / > 2\nRC", "2"));
    assert!(has_answer("2", "2"));
    assert!(!has_answer("tplain/crafted / > 12", "2"));
    assert!(!has_answer("tplain/crafted / > ", "2"));
}
