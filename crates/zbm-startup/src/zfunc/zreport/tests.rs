//! The system report on a fake machine, and against the legacy function on
//! the same machine.

use pretty_assertions::assert_eq;
use zbm_platform::color::decolorize;
use zbm_platform::exec::{Output, Status};
use zbm_testkit::fakes::FakeSystem;

use super::*;

/// What `zfs version` prints.
const ZFS_VERSION: &str = "zfs-2.2.6-1\nzfs-kmod-2.2.6-1\n";

/// What `zpool list` prints.
const ZPOOL_LIST: &str = "NAME   SIZE  ALLOC   FREE  CKPOINT  EXPANDSZ   FRAG    CAP  DEDUP    \
    HEALTH  ALTROOT\ntank  9.50G  1.20G  8.30G        -         -     0%    12%  1.00x    \
    ONLINE  -\n";

/// What `zfs list` prints of the report's columns.
const ZFS_LIST: &str = "NAME          MOUNTPOINT  CANMOUNT  ENCROOT  KEYSTATUS  KEYLOCATION  \
    ORG.ZFSBOOTMENU:KEYSOURCE\ntank          none        off       -        -          none   \
    -\ntank/ROOT/be  /           noauto    -        -          none         -\n";

/// The `zfs list` of the report.
const ZFS_LIST_COMMAND: &str = "zfs list -o \
    name,mountpoint,canmount,encroot,keystatus,keylocation,org.zfsbootmenu:keysource";

/// What `modinfo -F FIELD MODULE` prints, by `FIELD MODULE`.
const MODINFO: [(&str, &str); 4] = [
    ("filename spl", "/lib/modules/6.6.0-zbm/extra/spl.ko.zst"),
    ("version spl", "2.2.6-1"),
    ("filename zfs", "/lib/modules/6.6.0-zbm/extra/zfs.ko.zst"),
    ("version zfs", "2.2.6-1"),
];

/// What `uname -srm` prints, from these files of `/proc/sys/kernel`.
const UNAME: [(&str, &str); 3] = [
    ("ostype", "Linux"),
    ("osrelease", "6.6.0-zbm"),
    ("arch", "x86_64"),
];

/// The saved command line.
const CMDLINE: &str = "quiet\nzbm.timeout=5\nroot=zfs:AUTO\n";

/// A file of a machine: its path, its text and its mode; a path that ends
/// with `/` is a directory.
type Entry = (&'static str, &'static str, u32);

/// The files every machine has.
fn common() -> Vec<Entry> {
    vec![("/zfsbootmenu/zbm.cmdline", CMDLINE, 0o644)]
}

/// A machine with `entries`, whose programs answer as [`ZFS_VERSION`],
/// [`ZPOOL_LIST`], [`ZFS_LIST`] and [`MODINFO`] say.
fn machine(entries: &[Entry]) -> FakeSystem {
    machine_listing(entries, 0)
}

/// A [`machine`] whose `zfs list` exits `status`, printing nothing when
/// it fails.
fn machine_listing(entries: &[Entry], status: u8) -> FakeSystem {
    let fake = FakeSystem::new();
    for (name, value) in UNAME {
        fake.files
            .file(format!("/proc/sys/kernel/{name}"), format!("{value}\n"));
    }
    for (path, text, mode) in common().iter().chain(entries) {
        if path.ends_with('/') {
            fake.files.dir(path);
        } else {
            fake.files.file_with_mode(path, text, *mode);
        }
    }
    fake.runner.respond("zfs version", 0, ZFS_VERSION);
    fake.runner.respond("zpool list", 0, ZPOOL_LIST);
    let listed = if status == 0 { ZFS_LIST } else { "" };
    fake.runner.respond(ZFS_LIST_COMMAND, status, listed);
    for (args, said) in MODINFO {
        fake.runner
            .respond(&format!("modinfo -F {args}"), 0, &format!("{said}\n"));
    }
    fake
}

/// What `zreport` prints on `fake`, and its status.
fn printed(fake: &FakeSystem) -> (i32, String) {
    let state = State::new(&fake.files, "/zfsbootmenu");
    let mut out = Vec::new();
    let status = zreport(&fake.system(), &state, &[], &mut out);
    (status, String::from_utf8(out).unwrap())
}

/// ADR 0018, point 17, and P13: the report lists the built-in steps with
/// their state, which the legacy function could not, a step a hook ran
/// once in place of as replaced, and escapes the names of hooks, the
/// command line and what the programs print, which the legacy function
/// printed raw.
#[test]
fn zreport_lists_a_built_in_step_and_escapes_a_name() {
    let fake = machine(&[
        ("/libexec/hooks/early-setup.d/10-\x1b]0;x\x07.sh", "", 0o755),
        (
            "/libexec/hooks/early-setup.d/30-console-autosize.sh.completed",
            "",
            0,
        ),
        ("/zfsbootmenu/zbm.cmdline", "quiet\nzbm.x=\x1b[2J\n", 0o644),
        (
            "/zfsbootmenu/steps",
            "skipped 90-xhci-unbind.sh\nran 05-screen-readers.sh\n\
             ran 10-generic-efi-init.sh\nran 30-console-autosize.sh\n",
            0o644,
        ),
    ]);
    fake.runner.first(|command| {
        let listed = "NAME\ntank/\x1b[31mred\n";
        (command.display() == ZFS_LIST_COMMAND).then(|| Ok(Output::new(Status::exited(0), listed)))
    });

    let (status, text) = printed(&fake);

    assert_eq!(status, 0);
    let steps = [
        "\x1b[0;32mearly-setup.d/05-screen-readers.sh (run once, completed)\x1b[0m",
        "\x1b[0;32mearly-setup.d/10-generic-efi-init.sh (run once, completed)\x1b[0m",
        "\x1b[0;32mearly-setup.d/20-dracut-console-init.sh\x1b[0m",
        "\x1b[0;31mearly-setup.d/30-console-autosize.sh (replaced by a hook)\x1b[0m",
        "\x1b[0;32mteardown.d/80-luks-close.sh\x1b[0m",
        "\x1b[0;31mteardown.d/90-xhci-unbind.sh (skipped by zbm.skip_hooks)\x1b[0m",
    ];
    for step in steps {
        assert!(text.contains(&format!("* {step}\n")), "{step}\n{text}");
    }
    assert!(
        text.contains("\x1b[0;33m\n>> Built-in steps\n\x1b[0m"),
        "{text}"
    );
    assert!(
        text.contains("* \x1b[0;32m/libexec/hooks/early-setup.d/10-\\x1b]0;x\\x07.sh\x1b[0m\n"),
        "{text}"
    );
    assert!(text.contains("\x1b[0mquiet zbm.x=\\x1b[2J\n"), "{text}");
    assert!(text.contains("\ntank/\\x1b[31mred\n"), "{text}");
    assert!(!text.contains("\x1b]0") && !text.contains("\x1b[2J") && !text.contains("\x1b[31m"));
}

/// The report lists the seven properties of every dataset the legacy
/// function listed, headed, as `zfs list` prints them, after the pools.
#[test]
fn zreport_prints_the_seven_columns_of_every_dataset() {
    let fake = machine(&[]);

    let (_, text) = printed(&fake);

    let datasets = format!("\x1b[0;33m\n>> ZFS datasets\n\x1b[0m{ZFS_LIST}");
    assert!(text.ends_with(&datasets), "{text}");
    let pools = format!("\x1b[0;33m\n>> Imported zpools\n\x1b[0m{ZPOOL_LIST}");
    assert!(text.contains(&format!("{pools}{datasets}")), "{text}");
    let lines = fake.runner.lines();
    assert!(lines.contains(&ZFS_LIST_COMMAND.to_owned()), "{lines:?}");
    assert!(
        text.contains(
            "\n/lib/modules/6.6.0-zbm/extra/zfs.ko.zst: 2.2.6-1\n\
             \x1b[0;33m\n>> ZFS version\n\x1b[0mzfs-2.2.6-1\n"
        ),
        "{text}"
    );
}

/// A writer whose bytes a test keeps, shared with the fake programs.
#[derive(Clone, Default)]
struct Shared(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl Shared {
    /// What was written so far.
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}

impl Write for Shared {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The report's status is that of its last command, `zfs list`, as the
/// legacy function's was; each section is printed before its program
/// runs, so that the program's messages follow its title.
#[test]
fn zreport_exits_as_zfs_list_and_prints_each_title_first() {
    let fake = machine(&[]);
    let out = Shared::default();
    let seen = Shared::default();
    let (printed, said) = (out.clone(), seen.clone());
    fake.runner.first(move |command| {
        let line = command.display();
        let printed = decolorize(&printed.text());
        let last = printed.lines().rfind(|line| !line.is_empty());
        let _ = writeln!(said.clone(), "{line}: {}", last.unwrap_or(""));
        (line == ZFS_LIST_COMMAND).then(|| Ok(Output::new(Status::exited(1), "")))
    });
    let state = State::new(&fake.files, "/zfsbootmenu");

    let status = zreport(&fake.system(), &state, &[], &mut out.clone());

    assert_eq!(status, 1);
    let seen = seen.text();
    let expected = [
        "modinfo -F filename spl: >> ZFS/SPL module information",
        "modinfo -F version spl: >> ZFS/SPL module information",
        "modinfo -F filename zfs: /lib/modules/6.6.0-zbm/extra/spl.ko.zst: 2.2.6-1",
        "modinfo -F version zfs: /lib/modules/6.6.0-zbm/extra/spl.ko.zst: 2.2.6-1",
        "zfs version: >> ZFS version",
        "zpool list: >> Imported zpools",
        &format!("{ZFS_LIST_COMMAND}: >> ZFS datasets"),
    ];
    assert_eq!(seen.lines().collect::<Vec<_>>(), expected);
    assert!(decolorize(&out.text()).ends_with("\n>> ZFS datasets\n"));
}

/// The release line names the initramfs builder from mkinitcpio's
/// `/VERSION` first, then from dracut's release files, the legacy one
/// first; a release that ends with `dev` is red.
#[test]
fn the_release_line_names_the_builder_and_the_kernel() {
    let release = "PRETTY_NAME=\"ZFSBootMenu\"\nVERSION=\"3.1.0-dev\"\n";
    let fake = machine(&[
        ("/etc/zbm-release", release, 0o644),
        ("/etc/initrd-release", "DRACUT_VERSION=059\n", 0o644),
        ("/usr/lib/initrd-release", "VERSION_ID=105\n", 0o644),
    ]);

    let (_, text) = printed(&fake);

    let line = "\x1b[0;33mZFS\x1b[0m\x1b[0;37mBootMenu\x1b[0m \x1b[0;31m3.1.0-dev\x1b[0m \
                with Dracut 059 and Linux 6.6.0-zbm x86_64\n";
    assert!(text.contains(&format!("\x1b[0m{line}")), "{text}");
    fake.files.file("/VERSION", " v38 \n");
    fake.files.file("/proc/sys/kernel/arch", "");
    let (_, text) = printed(&fake);
    let builder = format!(
        "with mkinitcpio v38 and Linux 6.6.0-zbm {}\n",
        std::env::consts::ARCH
    );
    assert!(text.contains(&builder), "{text}");
}

/// A writer that fails with `kind`.
struct Failing(std::io::ErrorKind);

impl Write for Failing {
    fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
        Err(self.0.into())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A report that cannot be printed is status 1, but for a reader that went
/// away, as in `zreport | head`, which is 0.
#[test]
fn a_report_that_cannot_be_printed_is_status_1() {
    let fake = machine(&[]);
    let state = State::new(&fake.files, "/zfsbootmenu");
    let run = |kind| zreport(&fake.system(), &state, &[], &mut Failing(kind));

    assert_eq!(run(std::io::ErrorKind::StorageFull), 1);
    assert_eq!(run(std::io::ErrorKind::BrokenPipe), 0);
}

/// The machines of the parity test: files below the root, and the exit
/// status of `zfs list`, which fails without the ZFS module.
const MACHINES: [(&[Entry], u8); 5] = [
    (
        &[
            (
                "/etc/zbm-release",
                "NAME=\"ZFSBootMenu\"\nPRETTY_NAME=\"ZFSBootMenu\"\nVERSION=\"3.0.1\"\n",
                0o644,
            ),
            ("/VERSION", "v38\n", 0o644),
            ("/libexec/hooks/early-setup.d/10-early.sh", "", 0o755),
            ("/libexec/hooks/setup.d/20-off.sh", "", 0o644),
            ("/libexec/hooks/setup.d/.hidden.sh", "", 0o755),
            ("/libexec/hooks/boot-sel.d/sub/", "", 0o755),
            ("/libexec/hooks/teardown.d/30-once.sh.completed", "", 0),
            ("/libexec/hooks/teardown.d/90-xhci-unbind.sh", "", 0o755),
            ("/libexec/hooks/notes.txt", "", 0o644),
        ],
        0,
    ),
    (
        &[
            (
                "/etc/zbm-release",
                "PRETTY_NAME=\"My Menu\"\nVERSION=\"3.1.0-dev\"\n",
                0o644,
            ),
            ("/etc/initrd-release", "DRACUT_VERSION=059\n", 0o644),
            ("/libexec/hooks/load-key.d/a b.sh", "", 0o700),
        ],
        0,
    ),
    (&[("/usr/lib/initrd-release", "VERSION_ID=105\n", 0o644)], 0),
    (
        &[
            ("/etc/initrd-release", "NAME=dracut\n", 0o644),
            ("/usr/lib/initrd-release", "VERSION_ID=105\n", 0o644),
        ],
        0,
    ),
    (&[], 1),
];

/// The legacy `zreport` with its paths below `$ROOT`.
fn zreport_function_text() -> String {
    use zbm_testkit::legacy::path;

    let library = std::fs::read_to_string(path("zfsbootmenu/lib/zfsbootmenu-core.sh")).unwrap();
    let start = library.find("\nzreport() {\n").unwrap() + 1;
    let end = start + library[start..].find("\n}\n").unwrap() + 3;
    let mut function = library[start..end].to_owned();
    for (from, to) in [
        ("/etc/zbm-release", "\"${ROOT}\"/etc/zbm-release"),
        ("\"/VERSION\"", "\"${ROOT}/VERSION\""),
        ("< /VERSION", "< \"${ROOT}\"/VERSION"),
        ("/etc/initrd-release", "\"${ROOT}\"/etc/initrd-release"),
        (
            "/usr/lib/initrd-release",
            "\"${ROOT}\"/usr/lib/initrd-release",
        ),
        ("/libexec/hooks/*.d/*", "\"${ROOT}\"/libexec/hooks/*.d/*"),
    ] {
        assert!(function.contains(from), "{from}");
        function = function.replace(from, to);
    }
    function
}

/// Runs the legacy `zreport` on a machine of `entries` built below a
/// temporary root, with fake `zfs`, `zpool`, `modinfo` and `uname`, and a
/// `zfs list` that exits `status`: its status and what it printed, the
/// root's path taken out.
fn report_of_the_legacy_function(entries: &[Entry], status: u8) -> (i32, String) {
    use std::os::unix::fs::PermissionsExt as _;
    use zbm_testkit::TempDir;
    use zbm_testkit::legacy::{Bash, LOG_STUBS};

    let dir = TempDir::new("zbm-zreport");
    let root = dir.join("root");
    for (path, text, mode) in common().iter().chain(entries) {
        let full = root.join(path.trim_start_matches('/'));
        if path.ends_with('/') {
            std::fs::create_dir_all(&full).unwrap();
        } else {
            dir.write(&full, text);
            std::fs::set_permissions(&full, std::fs::Permissions::from_mode(*mode)).unwrap();
        }
    }
    let modinfo = MODINFO
        .iter()
        .fold(String::new(), |mut cases, (args, said)| {
            let _ = writeln!(cases, "  \"{args}\") echo '{said}' ;;");
            cases
        });
    let zfs = format!(
        "case \"$*\" in\n  version) printf '%s' \"$ZFS_VERSION\" ;;\n  \
         \"{}\") [ \"$ZFS_LIST_STATUS\" = 0 ] || exit \"$ZFS_LIST_STATUS\"\n    \
         printf '%s' \"$ZFS_LIST\" ;;\nesac\n",
        &ZFS_LIST_COMMAND[4..]
    );
    let programs = [
        ("zfs", zfs),
        (
            "zpool",
            "[ \"$*\" = list ] && printf '%s' \"$ZPOOL_LIST\"\n".to_owned(),
        ),
        ("modinfo", format!("case \"$2 $3\" in\n{modinfo}esac\n")),
        (
            "uname",
            "[ \"$*\" = -srm ] && echo 'Linux 6.6.0-zbm x86_64'\n".to_owned(),
        ),
    ];
    for (name, script) in programs {
        dir.write_executable(format!("bin/{name}"), format!("#!/bin/sh\n{script}"));
    }
    let search = std::env::var("PATH").unwrap_or_default();
    let output = Bash::new()
        .code(LOG_STUBS)
        .code(
            r#"source() {
                case "$1" in
                  /lib/*) builtin source "$LEGACY/zfsbootmenu/lib/${1#/lib/}" ;;
                  *) builtin source "$@" ;;
                esac
              }"#,
        )
        .source("zfsbootmenu/lib/zfsbootmenu-core.sh")
        .code(&zreport_function_text())
        .env("PATH", &format!("{}:{search}", dir.join("bin").display()))
        .env("ROOT", &root.to_string_lossy())
        .env("BASE", &root.join("zfsbootmenu").to_string_lossy())
        .env("ZFS_VERSION", ZFS_VERSION)
        .env("ZPOOL_LIST", ZPOOL_LIST)
        .env("ZFS_LIST", ZFS_LIST)
        .env("ZFS_LIST_STATUS", &status.to_string())
        .call("zreport", &[], b"");
    let text = decolorize(&output.stdout_text()).replace(&*root.to_string_lossy(), "");
    (output.status.code().unwrap_or(-1), text)
}

/// ADR 0018, points 17 and 19: the report prints what the legacy function
/// printed, colours aside, on the same machine, and exits as it did. The
/// built-in steps are set aside: the legacy function never listed them.
#[test]
fn zreport_matches_the_legacy_function() {
    let mut parity = zbm_testkit::parity!("zreport_matches_the_legacy_function");
    for (entries, listing) in MACHINES {
        let input = format!("{entries:?}, zfs list exits {listing}");
        let expected: (i32, String) =
            parity.legacy(input, || report_of_the_legacy_function(entries, listing));

        let (status, text) = printed(&machine_listing(entries, listing));

        let text = decolorize(&text);
        let start = text.find("\n>> Built-in steps\n").unwrap();
        let end = text.find("\n>> ZFS/SPL module information\n").unwrap();
        let without_steps = format!("{}{}", &text[..start], &text[end..]);
        assert_eq!((status, without_steps), expected, "{entries:?}");
    }
}
