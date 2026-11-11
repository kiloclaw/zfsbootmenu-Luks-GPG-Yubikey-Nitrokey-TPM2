//! ZFSBootMenu's command line on a fake machine, and against the legacy
//! function on the same machine.

use pretty_assertions::assert_eq;
use zbm_config::conf::Conf;
use zbm_testkit::fakes::FakeSystem;

use super::*;
use crate::zfunc::{Outcome, run};

/// What `zbmcmdline` prints on `fake`, whose state directory is
/// `/zfsbootmenu`, and its status, run as `zbm zfunc` runs it.
fn printed(fake: &FakeSystem) -> (i32, String) {
    fake.files.dir("/zfsbootmenu");
    let conf = Conf::parse("export BASE=\"/zfsbootmenu\"\n");
    let mut out = Vec::new();
    let Outcome::Status(status) = run(&fake.system(), &conf, "zbmcmdline", &[], &mut out) else {
        panic!("zbmcmdline is legacy");
    };
    (status, String::from_utf8(out).unwrap())
}

/// The legacy function joins the saved lines with one space, an empty
/// line too, and ends with a newline; the native escapes control
/// characters (P13).
#[test]
fn zbmcmdline_joins_the_cache_with_spaces() {
    let fake = FakeSystem::new();
    fake.files.file(
        "/zfsbootmenu/zbm.cmdline",
        "quiet\nzbm.timeout=5\n\nroot=zfs:AUTO\nzbm.x=\x1b[2J",
    );

    assert_eq!(
        printed(&fake),
        (
            0,
            "quiet zbm.timeout=5  root=zfs:AUTO zbm.x=\\x1b[2J\n".to_owned()
        )
    );
    assert!(fake.runner.lines().is_empty());
}

/// A saved command line that is not UTF-8 is still read, as the legacy
/// function's `cat` read it, and left as it is.
#[test]
fn zbmcmdline_reads_a_cache_that_is_not_utf_8() {
    let fake = FakeSystem::new();
    fake.files.file("/proc/cmdline", "built\n");
    fake.files
        .file("/zfsbootmenu/zbm.cmdline", b"quiet\nzbm.x=\xff\n");

    assert_eq!(printed(&fake), (0, "quiet zbm.x=\u{fffd}\n".to_owned()));
    assert_eq!(
        fake.files.contents("/zfsbootmenu/zbm.cmdline").as_deref(),
        Some(&b"quiet\nzbm.x=\xff\n"[..])
    );
}

/// The command lines a machine starts with: the image's, its drop-ins and
/// the kernel's.
const SOURCES: [(&str, &str); 5] = [
    ("/etc/cmdline", "zbm.timeout=3\n"),
    ("/etc/cmdline.d/b.conf", "quiet\n"),
    (
        "/etc/cmdline.d/a.conf",
        "loglevel=7 dyndbg=\"file a.c +p\"\n",
    ),
    ("/etc/cmdline.d/.hidden.conf", "hidden\n"),
    ("/proc/cmdline", "BOOT_IMAGE=/vmlinuz root=zfs:AUTO ro\n"),
];

/// A machine with [`SOURCES`] and no saved command line.
fn unsaved() -> FakeSystem {
    let fake = FakeSystem::new();
    for (path, text) in SOURCES {
        fake.files.file(path, text);
    }
    fake.files.file("/etc/cmdline.d/c.txt", "other\n");
    fake
}

/// Without a saved command line, the legacy function built it from the
/// image's and the kernel's, and saved it one argument per line, as
/// start-up saves it.
#[test]
fn zbmcmdline_writes_the_cache_when_it_builds_it() {
    let fake = unsaved();

    let (status, line) = printed(&fake);

    let built = "zbm.timeout=3 loglevel=7 dyndbg=\"file a.c +p\" quiet \
                 BOOT_IMAGE=/vmlinuz root=zfs:AUTO ro\n";
    assert_eq!((status, line.as_str()), (0, built));
    let saved = "zbm.timeout=3\nloglevel=7\ndyndbg=\"file a.c +p\"\nquiet\n\
                 BOOT_IMAGE=/vmlinuz\nroot=zfs:AUTO\nro\n";
    assert_eq!(
        fake.files.text("/zfsbootmenu/zbm.cmdline").as_deref(),
        Some(saved)
    );
    // Saved, it is read back.
    fake.files.file("/proc/cmdline", "changed\n");
    assert_eq!(printed(&fake), (0, built.to_owned()));

    // A state directory that cannot be written still gets the line.
    let read_only = unsaved();
    read_only.files.fail(
        "/zfsbootmenu/zbm.cmdline",
        std::io::ErrorKind::ReadOnlyFilesystem,
    );
    assert_eq!(printed(&read_only), (0, built.to_owned()));
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

/// A line that cannot be printed is status 1, which is logged; a reader
/// that went away, as in `zbmcmdline | true`, is 0.
#[test]
fn a_line_that_cannot_be_printed_is_status_1() {
    let fake = unsaved();
    let state = State::new(&fake.files, "/zfsbootmenu");
    let run = |kind| zbmcmdline(&fake.system(), &state, &[], &mut Failing(kind));

    assert_eq!(run(std::io::ErrorKind::StorageFull), 1);
    assert_eq!(run(std::io::ErrorKind::BrokenPipe), 0);
}

/// The legacy `_build_zbm_kcl` with its paths below `$ROOT`.
fn build_function_text() -> String {
    use zbm_testkit::legacy::path;

    let library = std::fs::read_to_string(path("zfsbootmenu/lib/zfsbootmenu-kcl.sh")).unwrap();
    let start = library.find("\n_build_zbm_kcl() {\n").unwrap() + 1;
    let end = start + library[start..].find("\n}\n").unwrap() + 3;
    let mut function = library[start..end].to_owned();
    for (from, to) in [
        ("/etc/cmdline", "\"${ROOT}\"/etc/cmdline"),
        ("/proc/cmdline", "\"${ROOT}\"/proc/cmdline"),
    ] {
        assert!(function.contains(from), "{from}");
        function = function.replace(from, to);
    }
    function
}

/// The cases of the parity test: the saved command line, if any, and
/// whether the machine has [`SOURCES`].
const CASES: [(Option<&str>, bool); 5] = [
    (Some("quiet\nzbm.timeout=5\nroot=zfs:AUTO\n"), true),
    (Some("a\n\nb c"), false),
    (Some(""), false),
    (None, true),
    (None, false),
];

/// Runs the legacy `zbmcmdline` on a machine of `saved` and, with
/// `sources`, [`SOURCES`], below a temporary root: what it printed and
/// what it left saved.
fn line_of_the_legacy_function(saved: Option<&str>, sources: bool) -> (String, Option<String>) {
    use zbm_testkit::TempDir;
    use zbm_testkit::legacy::{Bash, LOG_STUBS};

    let dir = TempDir::new("zbm-zbmcmdline");
    std::fs::create_dir_all(dir.join("zfsbootmenu")).unwrap();
    if let Some(saved) = saved {
        dir.write("zfsbootmenu/zbm.cmdline", saved);
    }
    if sources {
        for (path, text) in SOURCES {
            dir.write(path.trim_start_matches('/'), text);
        }
        dir.write("etc/cmdline.d/c.txt", "other\n");
    }
    let output = Bash::new()
        .code(LOG_STUBS)
        .source("zfsbootmenu/lib/zfsbootmenu-kcl.sh")
        .code(&build_function_text())
        .env("ROOT", &dir.path().to_string_lossy())
        .env("BASE", &dir.join("zfsbootmenu").to_string_lossy())
        .call("zbmcmdline", &[], b"");
    let left = std::fs::read_to_string(dir.join("zfsbootmenu/zbm.cmdline")).ok();
    (output.stdout_text(), left)
}

/// ADR 0018, point 19: `zbmcmdline` prints what the legacy function
/// printed and leaves the same saved command line, read or built.
#[test]
fn zbmcmdline_matches_the_legacy_function() {
    let mut parity = zbm_testkit::parity!("zbmcmdline_matches_the_legacy_function");
    for (saved, sources) in CASES {
        let input = format!("saved {saved:?}, sources {sources}");
        let expected: (String, Option<String>) =
            parity.legacy(input, || line_of_the_legacy_function(saved, sources));

        let fake = if sources {
            unsaved()
        } else {
            FakeSystem::new()
        };
        if let Some(saved) = saved {
            fake.files.file("/zfsbootmenu/zbm.cmdline", saved);
        }
        let (status, line) = printed(&fake);

        assert_eq!(status, 0);
        let left = fake.files.text("/zfsbootmenu/zbm.cmdline");
        assert_eq!((line, left), expected, "{saved:?}");
    }
}
