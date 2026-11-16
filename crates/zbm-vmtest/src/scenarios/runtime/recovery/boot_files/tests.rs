use pretty_assertions::assert_eq;

use super::*;
use crate::expect::regex;
use crate::testing::{Step, checks, cleanup, scripted, settings};

/// The failed boot's prompt, once `zkexec` could not boot `vmlinuz-4`.
const NOT_LOADED_PROMPT: &str = "\x1b[H\x1b[2J\x1b[0;31mUnable to load kernel or initramfs into \
    memory\x1b[0m\r\n/zfsbootmenu/environments/tplain/crafted/mnt/boot/vmlinuz-4\r\n\
    Press [RETURN] to continue or wait 10 seconds";

/// What the recovery shell answers in and after `zkexec`'s search and
/// boot of `tplain/crafted`.
#[derive(Clone, Copy)]
struct Booted {
    /// `zkexec`, up to its prompt if it shows one.
    booted: &'static str,
    /// Whether `zkexec` shows the prompt of a failed load.
    prompted: bool,
    /// `zkexec` once its prompt was answered.
    ended: &'static str,
    /// The kernel log's lines of the refusal counted.
    logged: &'static str,
    /// The kernel list, through `cat -A`.
    list: &'static str,
    /// `select_kernel`, through `cat -A`.
    selected: &'static str,
    /// `vmlinuz-3`'s cached command line.
    kcl: &'static str,
    /// `test -e` of `vmlinuz-6`'s cached command line.
    fifo: &'static str,
    /// What is left mounted.
    left: &'static str,
}

/// The answers since SB20: the boot refused at the initramfs, the
/// environment's own lines and command line.
const RESOLVED: Booted = Booted {
    booted: NOT_LOADED_PROMPT,
    prompted: true,
    ended: "zkexec: unable to boot tplain/crafted: the kernel could not be executed\r\n\
            ZKEXEC-RC=1\r\nzbm# ",
    logged: "1\r\nRC=0\r\n",
    list: "tplain/crafted^I/boot/vmlinuz-3^I/boot/initramfs-3.img$\r\n\
           tplain/crafted^I/boot/vmlinuz-6^I/boot/initramfs-6.img$\r\nRC=0\r\n",
    selected: "tplain/crafted^I/boot/vmlinuz-6^I/boot/initramfs-6.img$\r\nRC=0\r\n",
    kcl: "crafted-own\r\nRC=0\r\n",
    fifo: "RC=1\r\n",
    left: "RC=1\r\n",
};

/// The script of [`crafted_boot`] with `answers`.
fn boot_script(answers: Booted) -> Vec<Step> {
    let mut script = vec![
        Step::Hear("zkexec tplain/crafted vmlinuz-4 initramfs-4.img; echo ZKEXEC-RC=$?"),
        Step::Say(answers.booted),
    ];
    if answers.prompted {
        script.extend([Step::Keys("\r"), Step::Say(answers.ended)]);
    }
    script.extend([
        Step::Hear("dmesg | grep -c 'initramfs-4.img.*is missing in the environment'; echo RC=$?"),
        Step::Say(answers.logged),
        Step::Hear("cat -A /zfsbootmenu/environments/tplain/crafted/kernels; echo RC=$?"),
        Step::Say(answers.list),
        Step::Hear("select_kernel tplain/crafted | cat -A; echo RC=$?"),
        Step::Say(answers.selected),
        Step::Hear(
            "cat /zfsbootmenu/environments/tplain/crafted/kcl/boot:vmlinuz-3.kcl; echo RC=$?",
        ),
        Step::Say(answers.kcl),
        Step::Hear(
            "test -e /zfsbootmenu/environments/tplain/crafted/kcl/boot:vmlinuz-6.kcl; \
             echo RC=$?",
        ),
        Step::Say(answers.fifo),
        Step::Hear(
            "grep -E ' /zfsbootmenu/environments/tplain/(mnt|crafted/)' /proc/self/mounts; \
             echo RC=$?",
        ),
        Step::Say(answers.left),
    ]);
    script
}

/// Plays `script` against [`crafted_boot`]; whether it ended well, and
/// the checks it recorded.
fn boot_checks(name: &str, script: Vec<Step>) -> (bool, Vec<String>) {
    let settings = settings(name);
    let test = Test::new(&settings, "runtime-rust-recovery").unwrap();
    let (mut vm, guest) = scripted(&test, script);

    let ended = crafted_boot(&test, &mut vm).is_ok();

    assert_eq!(guest.finish(), Vec::<String>::new());
    let found = checks(&test);
    cleanup(&settings);
    (ended, found)
}

#[test]
fn the_crafted_boot_is_refused_and_its_search_keeps_the_environment_s_own_files() {
    let (ended, found) = boot_checks("runtime-recovery-crafted-boot", boot_script(RESOLVED));

    assert!(ended);
    assert_eq!(
        found,
        [
            "ok   - zkexec's search of tplain/crafted ends: the FIFO vmlinuz-6.kcl does not hold \
             it",
            "ok   - zkexec mounts tplain/crafted for its boot and fails at its files: the prompt",
            "ok   - zkexec tplain/crafted ends with 1 (1)",
            "ok   - the kernel log says why: initramfs-4.img is missing in the environment (1)",
            "ok   - every line of tplain/crafted's kernel list names tplain/crafted (0)",
            "ok   - tplain/crafted's kernel list holds vmlinuz-3 and vmlinuz-6 alone: not \
             vmlinuz-4, whose initramfs is missing inside it, nor the newline name (2)",
            "ok   - the library's select_kernel tplain/crafted takes its vmlinuz-6",
            "ok   - vmlinuz-3's cached command line is tplain/crafted's own \
             /etc/zfsbootmenu.conf, crafted-own (0)",
            "ok   - the FIFO vmlinuz-6.kcl is not read: no command line is cached for it (1)",
            "ok   - a guard: nothing is mounted on tplain's mount point or below \
             /zfsbootmenu/environments/tplain/crafted (1)",
        ]
    );
}

#[test]
fn a_search_and_boot_that_follow_links_into_the_image_and_split_the_newline_name_fail() {
    // Before SB20, with no FIFO, which held the search then: the image's
    // /etc/hostid loaded as the initramfs, which the empty kernel fails,
    // the image's configuration cached, and the newline name's pieces.
    let answers = Booted {
        logged: "0\r\nRC=1\r\n",
        list: "tplain/crafted^I/boot/vmlinuz-3^I/boot/initramfs-3.img$\r\n\
               tplain/crafted^I/boot/vmlinuz-4^I/boot/initramfs-4.img$\r\n\
               tplain/crafted^I/boot/vmlinuz-6^I/boot/initramfs-6.img$\r\n\
               tplain/crafted^I/boot/vmlinuz-9$\r\n\
               tplain^Ix^I/boot/initramfs-9$\r\n\
               tplain^Ix^Iy.img$\r\nRC=0\r\n",
        selected: "tplain^Ix^Iy.img$\r\nRC=0\r\n",
        kcl: "zbm.timeout=30\r\nRC=0\r\n",
        fifo: "RC=0\r\n",
        ..RESOLVED
    };

    let (ended, found) = boot_checks(
        "runtime-recovery-crafted-boot-followed",
        boot_script(answers),
    );

    assert!(ended);
    assert_eq!(
        found,
        [
            "ok   - zkexec's search of tplain/crafted ends: the FIFO vmlinuz-6.kcl does not hold \
             it",
            "ok   - zkexec mounts tplain/crafted for its boot and fails at its files: the prompt",
            "ok   - zkexec tplain/crafted ends with 1 (1)",
            "FAIL - the kernel log says why: initramfs-4.img is missing in the environment (0)",
            "FAIL - every line of tplain/crafted's kernel list names tplain/crafted (0)",
            "FAIL - tplain/crafted's kernel list holds vmlinuz-3 and vmlinuz-6 alone: not \
             vmlinuz-4, whose initramfs is missing inside it, nor the newline name (6)",
            "FAIL - the library's select_kernel tplain/crafted takes its vmlinuz-6",
            "FAIL - vmlinuz-3's cached command line is tplain/crafted's own \
             /etc/zfsbootmenu.conf, crafted-own (0)",
            "FAIL - the FIFO vmlinuz-6.kcl is not read: no command line is cached for it (0)",
            "ok   - a guard: nothing is mounted on tplain's mount point or below \
             /zfsbootmenu/environments/tplain/crafted (1)",
        ]
    );
}

#[test]
fn a_boot_without_its_prompt_an_empty_list_or_a_mount_left_fails() {
    let answers = Booted {
        booted: "zkexec: unable to mount tplain/crafted\r\nZKEXEC-RC=2\r\nzbm# ",
        prompted: false,
        logged: "grep: (standard input): binary file matches\r\nRC=0\r\n",
        list: "cat: /zfsbootmenu/environments/tplain/crafted/kernels: No such file or \
               directory\r\nRC=1\r\n",
        selected: "RC=0\r\n",
        kcl: "cat: boot:vmlinuz-3.kcl: No such file or directory\r\nRC=1\r\n",
        left: "tplain /zfsbootmenu/environments/tplain/mnt zfs ro 0 0\r\nRC=0\r\n",
        ..RESOLVED
    };

    let (ended, found) = boot_checks(
        "runtime-recovery-crafted-boot-unmounted",
        boot_script(answers),
    );

    assert!(ended);
    assert_eq!(
        found,
        [
            "ok   - zkexec's search of tplain/crafted ends: the FIFO vmlinuz-6.kcl does not hold \
             it",
            "FAIL - zkexec mounts tplain/crafted for its boot and fails at its files: the prompt",
            "FAIL - zkexec tplain/crafted ends with 1 (2)",
            "FAIL - the kernel log says why: initramfs-4.img is missing in the environment (0)",
            "FAIL - every line of tplain/crafted's kernel list names tplain/crafted (1)",
            "FAIL - tplain/crafted's kernel list holds vmlinuz-3 and vmlinuz-6 alone: not \
             vmlinuz-4, whose initramfs is missing inside it, nor the newline name (0)",
            "FAIL - the library's select_kernel tplain/crafted takes its vmlinuz-6",
            "FAIL - vmlinuz-3's cached command line is tplain/crafted's own \
             /etc/zfsbootmenu.conf, crafted-own (1)",
            "ok   - the FIFO vmlinuz-6.kcl is not read: no command line is cached for it (1)",
            "FAIL - a guard: nothing is mounted on tplain's mount point or below \
             /zfsbootmenu/environments/tplain/crafted (0)",
        ]
    );
}

#[test]
fn a_search_held_by_the_fifo_fails_and_ends_the_run() {
    let script = vec![Step::Hear(
        "zkexec tplain/crafted vmlinuz-4 initramfs-4.img; echo ZKEXEC-RC=$?",
    )];

    let (ended, found) = boot_checks("runtime-recovery-crafted-boot-held", script);

    assert!(!ended);
    assert_eq!(
        found,
        ["FAIL - zkexec's search of tplain/crafted ends: the FIFO vmlinuz-6.kcl does not hold it"]
    );
}

#[test]
fn only_the_lines_cat_ended_are_listed() {
    let said = "tplain/crafted^I/boot/vmlinuz-3^I/boot/initramfs-3.img$\r\n\
                cat: x: No such file or directory\r\n  tplain^Ix^Iy.img$ \r\n";

    assert_eq!(
        listed(said),
        [
            "tplain/crafted^I/boot/vmlinuz-3^I/boot/initramfs-3.img$",
            "tplain^Ix^Iy.img$",
        ]
    );
    assert_eq!(listed("RC"), Vec::<&str>::new());
}

#[test]
fn a_count_is_the_last_number_said_or_none() {
    assert_eq!(count("2\r\n"), 2);
    assert_eq!(count("grep: x: binary file matches\r\n3\r\n"), 3);
    assert_eq!(count("4\r\ngrep: x\r\n"), 4);
    assert_eq!(count("grep: x\r\n"), 0);
    assert_eq!(count(""), 0);
}

#[test]
fn the_steps_name_the_crafted_filesystem_s_paths() {
    assert_eq!(
        ZKEXEC_LINKED,
        format!("zkexec {CRAFTED} vmlinuz-4 initramfs-4.img; echo ZKEXEC-RC=$?")
    );
    assert_eq!(KERNEL_LIST, format!("cat -A {CRAFTED_STATE}/kernels"));
    assert_eq!(SELECTED, format!("select_kernel {CRAFTED} | cat -A"));
    assert_eq!(
        CACHED_KCL,
        format!("cat {CRAFTED_STATE}/kcl/boot:vmlinuz-3.kcl")
    );
    assert_eq!(
        FIFO_KCL,
        format!("test -e {CRAFTED_STATE}/kcl/boot:vmlinuz-6.kcl")
    );
    assert!(LISTED.iter().all(|line| line.starts_with(OWN_LINE)));
    assert!(LISTED[1].starts_with(SELECTED_LINE));
    assert_eq!(OWN_LINE, format!("{CRAFTED}^I"));
    assert!(!regex(NOT_LOADED).unwrap().is_match(ZKEXEC_LINKED));
    assert!(!regex(ZKEXEC_ENDED).unwrap().is_match(ZKEXEC_LINKED));
}
