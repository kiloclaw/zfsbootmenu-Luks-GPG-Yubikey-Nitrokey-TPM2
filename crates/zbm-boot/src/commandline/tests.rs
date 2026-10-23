//! Tests of the command line of a boot environment, its root prefix, device
//! tree and the kexec commands, several against the legacy runtime.

use super::*;
use std::time::{Duration, SystemTime};
use zbm_platform::exec::SystemRunner;
use zbm_platform::files::RealFiles;
use zbm_testkit::fakes::FakeFiles;
use zbm_testkit::legacy::{Bash, LOG_STUBS, log_lines};
use zbm_testkit::zfs_script::ZfsScript;
use zbm_testkit::{Rng, TempDir, parity};
use zbm_zfs::ZfsCommand;

#[test]
fn host_ids_come_from_spl_then_etc_then_the_match() {
    let files = FakeFiles::new();
    files.dir("/zfsbootmenu");
    let state = State::new(&files, "/zfsbootmenu");
    let mut log = Vec::new();
    assert_eq!(spl_hostid(&files, &state, &mut log), None);
    state.set_spl_hostid("deadbeef").unwrap();
    assert_eq!(
        spl_hostid(&files, &state, &mut log).as_deref(),
        Some("0xdeadbeef")
    );
    files.file(ETC_HOSTID, [0x0c_u8, 0xb1, 0xba, 0x00]);
    assert_eq!(
        spl_hostid(&files, &state, &mut log).as_deref(),
        Some("0x00bab10c")
    );
    files.file(ETC_HOSTID, [0x01_u8]);
    assert_eq!(
        spl_hostid(&files, &state, &mut log).as_deref(),
        Some("0x00000001")
    );
    files.file(SPL_HOSTID, "0\n");
    assert_eq!(
        spl_hostid(&files, &state, &mut log).as_deref(),
        Some("0x00000001")
    );
    files.file(SPL_HOSTID, "3735928559\n");
    assert_eq!(
        spl_hostid(&files, &state, &mut log).as_deref(),
        Some("0xdeadbeef")
    );
    files.file(SPL_HOSTID, "x\n");
    assert_eq!(
        spl_hostid(&files, &state, &mut log).as_deref(),
        Some("0x00000001")
    );
}

struct Properties(Vec<(&'static str, &'static str)>);

impl CommandLineSource for Properties {
    fn commandline(&self, filesystem: &str) -> Option<String> {
        let value = self.0.iter().find(|(fs, _)| *fs == filesystem);
        Some(value.map_or("-", |(_, value)| *value).to_owned())
    }

    fn inherited_source(&self, _: &str) -> Option<String> {
        Some(String::new())
    }
}

fn setup() -> (FakeFiles, Properties) {
    let files = FakeFiles::new();
    files.dir("/zfsbootmenu/environments/tank/ROOT/void/kcl");
    let properties = Properties(vec![("tank/ROOT/void", "ro quiet root=bad resume=/dev/x")]);
    (files, properties)
}

#[test]
fn command_lines_come_from_the_property_and_are_cached() {
    let (files, properties) = setup();
    let state = State::new(&files, "/zfsbootmenu");
    let mut log = Vec::new();
    let cmdline = |log: &mut Vec<Message>| {
        be_cmdline(
            &files,
            &properties,
            &state,
            "tank/ROOT/void",
            "/boot/vmlinuz",
            false,
            log,
        )
    };
    assert_eq!(cmdline(&mut log), "ro quiet resume=/dev/x");
    let cache = "/zfsbootmenu/environments/tank/ROOT/void/kcl/default.kcl";
    assert_eq!(
        files.text(cache).as_deref(),
        Some("ro\nquiet\nresume=/dev/x\n")
    );

    // The cache wins until the user declines to resume.
    files.file(cache, "cached\n");
    assert_eq!(cmdline(&mut log), "cached");
    state.set_noresume().unwrap();
    files.set_modified(cache, SystemTime::UNIX_EPOCH);
    assert_eq!(cmdline(&mut log), "ro quiet noresume");

    // The user's override wins over everything.
    state.set_cmdline_override("edited\nby user\n").unwrap();
    assert_eq!(cmdline(&mut log), "edited by user");
    assert!(
        log.iter()
            .any(|message| message.text.contains("/zfsbootmenu/cmdline"))
    );
    assert_eq!(
        be_cmdline(&files, &properties, &state, "", "", false, &mut log),
        ""
    );
}

#[test]
fn kernel_command_lines_and_defaults() {
    let (files, properties) = setup();
    let state = State::new(&files, "/zfsbootmenu");
    let mut log = Vec::new();
    files.file(
        "/zfsbootmenu/environments/tank/ROOT/void/kcl/boot:vmlinuz-6.1.kcl",
        "%{parent} single\n",
    );
    assert_eq!(
        be_cmdline(
            &files,
            &properties,
            &state,
            "tank/ROOT/void",
            "/boot/vmlinuz-6.1",
            false,
            &mut log
        ),
        "ro quiet resume=/dev/x single"
    );
    // No cache is written for a kernel with its own command line.
    assert!(!files.exists(Path::new(
        "/zfsbootmenu/environments/tank/ROOT/void/kcl/default.kcl"
    )));
    let empty = Properties(vec![]);
    assert_eq!(
        be_cmdline(
            &files,
            &empty,
            &state,
            "tank/ROOT/other",
            "",
            false,
            &mut log
        ),
        "quiet loglevel=4"
    );
}

#[test]
fn host_ids_replace_those_of_the_command_line() {
    let (files, properties) = setup();
    let state = State::new(&files, "/zfsbootmenu");
    let mut log = Vec::new();
    files.file(SPL_HOSTID, "3735928559\n");
    let line = be_cmdline(
        &files,
        &properties,
        &state,
        "tank/ROOT/void",
        "",
        true,
        &mut log,
    );
    assert_eq!(line, "ro quiet resume=/dev/x spl.spl_hostid=0xdeadbeef");
    files.remove_file(Path::new(SPL_HOSTID)).unwrap();
    files.file(ETC_HOSTID, [0_u8; 4]);
    files
        .remove_file(Path::new(
            "/zfsbootmenu/environments/tank/ROOT/void/kcl/default.kcl",
        ))
        .unwrap();
    let line = be_cmdline(
        &files,
        &properties,
        &state,
        "tank/ROOT/void",
        "",
        true,
        &mut log,
    );
    assert_eq!(line, "ro quiet resume=/dev/x spl_hostid=00000000");
}

#[test]
fn root_prefixes() {
    let files = FakeFiles::new();
    let mnt = Path::new("/mnt");
    assert_eq!(
        root_prefix(&files, Some("root=custom:"), mnt),
        "root=custom:"
    );
    assert_eq!(root_prefix(&files, Some("-"), mnt), "root=zfs:");
    assert_eq!(root_prefix(&files, None, mnt), "root=zfs:");
    files.file("/mnt/usr/lib/os-release", "NAME=\"Arch Linux\"\nID=arch\n");
    assert_eq!(root_prefix(&files, None, mnt), "zfs=");
    files.file("/mnt/etc/os-release", "ID='gentoo'\n");
    assert_eq!(root_prefix(&files, None, mnt), "root=ZFS=");
    files.file(
        "/mnt/etc/os-release",
        "ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n",
    );
    assert_eq!(root_prefix(&files, None, mnt), "root=zfs:");
    files.file("/mnt/etc/os-release", "ID=other\n");
    files.file("/mnt/usr/lib/os-release", "ID_LIKE=alpine\n");
    assert_eq!(root_prefix(&files, None, mnt), "root=ZFS=");
    assert_eq!(
        os_release("A=\"x \\\"y\\\" \\n\"\n# c\nB='z'\nC=a\\ b\nbad line\nD=\"open\n"),
        [
            ("A".to_owned(), "x \"y\" \\n".to_owned()),
            ("B".to_owned(), "z".to_owned()),
            ("C".to_owned(), "a b".to_owned()),
        ]
    );
}

#[test]
fn device_trees() {
    let files = FakeFiles::new();
    let mnt = Path::new("/mnt");
    files.file("/mnt/boot/dtbs/dtbs-6.1.0/qcom/x13s.dtb", "");
    files.file("/mnt/usr/lib/linux-image-6.1.0/x13s.dtb", "");
    let dtb = |property: &str, kernel: &str| {
        devicetree(&files, Some(property), kernel, mnt).map(|path| path.display().to_string())
    };
    assert_eq!(
        dtb("qcom/x13s.dtb", "/boot/vmlinuz-6.1.0"),
        Some("/mnt/boot/dtbs/dtbs-6.1.0/qcom/x13s.dtb".to_owned())
    );
    assert_eq!(
        dtb(
            "/usr/lib/linux-image-%{kernel}/x13s.dtb",
            "/boot/vmlinuz-6.1.0"
        ),
        Some("/mnt/usr/lib/linux-image-6.1.0/x13s.dtb".to_owned())
    );
    assert_eq!(dtb("qcom/missing.dtb", "/boot/vmlinuz-6.1.0"), None);
    assert_eq!(
        devicetree(&files, Some("-"), "/boot/vmlinuz-6.1.0", mnt),
        None
    );
    assert_eq!(devicetree(&files, None, "/boot/vmlinuz-6.1.0", mnt), None);
    // Without a hyphen the "version" is the whole path, as in the legacy code.
    assert_eq!(dtb("x.dtb", "/boot/vmlinuz"), None);
}

#[test]
fn kexec_commands() {
    let dtb = PathBuf::from("/mnt/boot/dtbs/x.dtb");
    let load = KexecLoad {
        mnt: "/zfsbootmenu/environments/tank/ROOT/void/mnt",
        dataset: "tank/ROOT/void",
        kernel: "/boot/vmlinuz-6.12.9_1",
        initramfs: "/boot/initramfs-6.12.9_1.img",
        dtb: Some(&dtb),
        root_prefix: "root=zfs:",
        args: "ro quiet",
    };
    let runner = zbm_testkit::FakeRunner::new();
    load.run(&runner).unwrap();
    let command = &runner.calls()[0];
    assert_eq!(command.program(), "kexec");
    let args: Vec<String> = command
        .get_args()
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        args,
        [
            "-a",
            "-l",
            "/zfsbootmenu/environments/tank/ROOT/void/mnt/boot/vmlinuz-6.12.9_1",
            "--initrd=/zfsbootmenu/environments/tank/ROOT/void/mnt/boot/initramfs-6.12.9_1.img",
            "--dtb=/mnt/boot/dtbs/x.dtb",
            "--command-line=root=zfs:tank/ROOT/void           ro quiet",
        ]
    );
    assert!(command.merges_stderr());
    assert_eq!(load.line(), command.display());
}

#[test]
fn the_separator_is_the_legacy_one() {
    // The legacy argument continues after "\" and a newline on a line
    // indented by ten spaces.
    let mut parity = parity!("the_separator_is_the_legacy_one");
    let output: String = parity.legacy("root=zfs:tank ro", || {
        Bash::new()
            .code(&format!(
                "{}\nshow() {{ root_prefix=root=zfs: fs=tank cli_args=ro; printf '%s' {}; }}",
                "set -u", "\"${root_prefix}${fs} \\\n          ${cli_args}\""
            ))
            .call("show", &[], b"")
            .stdout_text()
    });
    assert_eq!(output, format!("root=zfs:tank{SEPARATOR}ro"));
}

/// The legacy library, with the shim that finds `/lib/zfsbootmenu-kcl.sh`.
fn legacy_core() -> Bash {
    Bash::new()
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
}

/// A starting point of `load_be_cmdline`: properties, files under the
/// base directory, and the host ID setting.
#[derive(Debug)]
struct Scenario {
    /// `org.zfsbootmenu:commandline` of `tank/ROOT`.
    parent: Option<String>,
    /// `org.zfsbootmenu:commandline` of `tank/ROOT/be`.
    own: Option<String>,
    /// The command line cached for the kernel.
    kernel_kcl: Option<String>,
    /// Whether the user edited the command line.
    edited: bool,
    /// Whether `noresume` exists.
    noresume: bool,
    /// Whether a stale `default.kcl` exists.
    stale_default: bool,
    /// `zbm_set_hostid`.
    set_hostid: bool,
}

/// The kernel of every scenario.
const KERNEL: &str = "/boot/vmlinuz-6.1";

/// The cache of the default command line.
const DEFAULT_KCL: &str = "environments/tank/ROOT/be/kcl/default.kcl";

impl Scenario {
    fn generate(rng: &mut Rng) -> Scenario {
        let words = [
            "ro",
            "quiet",
            "root=x",
            "resume=/dev/y",
            "a=\"b c\"",
            "%{parent}",
            "spl_hostid=1",
        ];
        Scenario {
            parent: (rng.below(3) > 0).then(|| rng.join(&words[..5], 3)),
            own: (rng.below(3) > 0).then(|| rng.join(&words, 4)),
            kernel_kcl: (rng.below(3) == 0).then(|| format!("{}\n", rng.join(&words, 3))),
            edited: rng.below(4) == 0,
            noresume: rng.below(3) == 0,
            stale_default: rng.below(3) == 0,
            set_hostid: rng.below(2) == 0,
        }
    }

    /// The pool, with the properties.
    fn zfs(&self) -> ZfsScript {
        let zfs = ZfsScript::new(&["tank", "tank/ROOT", "tank/ROOT/be"]);
        let property = "org.zfsbootmenu:commandline";
        if let Some(value) = &self.parent {
            zfs.set("tank/ROOT", property, value);
        }
        if let Some(value) = &self.own {
            zfs.set("tank/ROOT/be", property, value);
        }
        zfs
    }

    /// A base directory with the files.
    fn base(&self) -> TempDir {
        let dir = TempDir::new("zbm-config");
        std::fs::create_dir_all(dir.join("environments/tank/ROOT/be/kcl")).unwrap();
        if let Some(text) = &self.kernel_kcl {
            dir.write("environments/tank/ROOT/be/kcl/boot:vmlinuz-6.1.kcl", text);
        }
        if self.edited {
            dir.write("cmdline", "edited\nline\n");
        }
        if self.noresume {
            dir.write("noresume", "");
        }
        if self.stale_default {
            dir.write(DEFAULT_KCL, "stale\n");
            let old = SystemTime::now() - Duration::from_secs(3600);
            let file = std::fs::File::options()
                .write(true)
                .open(dir.join(DEFAULT_KCL))
                .unwrap();
            file.set_modified(old).unwrap();
        }
        dir
    }
}

/// `text` with the base directory written as `$BASE`, so that it does not
/// depend on where the test ran.
fn without_base(text: &str, dir: &TempDir) -> String {
    text.replace(&*dir.path().to_string_lossy(), "$BASE")
}

#[test]
fn command_lines_match_the_legacy_function() {
    let mut parity = parity!("command_lines_match_the_legacy_function");
    let mut rng = Rng::from_env(31);
    for case in 0..30 {
        let scenario = Scenario::generate(&mut rng);
        let zfs = scenario.zfs();
        let (stdout, logged, cache): (String, String, Option<String>) =
            parity.legacy(format!("{scenario:?}"), || {
                let dir = scenario.base();
                let legacy = legacy_core()
                    .env("BASE", &dir.path().to_string_lossy())
                    .env("PATH", &zfs.path_env())
                    .env(
                        "zbm_set_hostid",
                        if scenario.set_hostid { "1" } else { "0" },
                    )
                    .call("load_be_cmdline", &["tank/ROOT/be", KERNEL], b"");
                (
                    without_base(&legacy.stdout_text(), &dir),
                    without_base(&log_lines(&legacy.stderr_text()), &dir),
                    std::fs::read_to_string(dir.join(DEFAULT_KCL)).ok(),
                )
            });

        let dir = scenario.base();
        let base = dir.path().to_string_lossy().into_owned();
        let state = State::new(&RealFiles, base.as_str());
        let runner = SystemRunner;
        let source = ZfsCommand::with_program(&runner, zfs.program());
        let mut log = Vec::new();
        let rust = be_cmdline(
            &RealFiles,
            &source,
            &state,
            "tank/ROOT/be",
            KERNEL,
            scenario.set_hostid,
            &mut log,
        );
        // kexec_kernel reads it with $( ... ), which drops final newlines
        assert_eq!(
            without_base(&rust, &dir),
            stdout.trim_end_matches('\n'),
            "case {case}: {logged}"
        );
        let rust_cache = std::fs::read_to_string(dir.join(DEFAULT_KCL)).ok();
        assert_eq!(rust_cache, cache, "case {case}: cache");
        let messages: String = log
            .iter()
            .map(|message| format!("{}\n", message.stderr_line()))
            .collect();
        assert_eq!(without_base(&messages, &dir), logged, "case {case}: log");
    }
}

/// The os-release files under `dir`, and the property, for the
/// description of a case.
fn os_releases(dir: &TempDir) -> Vec<Option<String>> {
    ["usr/lib/os-release", "etc/os-release"]
        .iter()
        .map(|file| std::fs::read_to_string(dir.join(file)).ok())
        .collect()
}

#[test]
fn root_prefixes_match_the_legacy_function() {
    let mut parity = parity!("root_prefixes_match_the_legacy_function");
    let mut rng = Rng::from_env(32);
    let ids = [
        "void",
        "arch",
        "gentoo",
        "alpine",
        "fedora",
        "\"ubuntu debian\"",
        "'artix'",
        "chimera",
        "",
    ];
    for case in 0..30 {
        let dir = TempDir::new("zbm-prefix");
        for file in ["usr/lib/os-release", "etc/os-release"] {
            if rng.below(3) > 0 {
                let mut text = String::from("NAME=\"Some Linux\"\n");
                if rng.below(2) == 0 {
                    text.push_str(&format!("ID={}\n", rng.choose(&ids)));
                }
                if rng.below(2) == 0 {
                    text.push_str(&format!("ID_LIKE={}\n", rng.choose(&ids)));
                }
                dir.write(file, text);
            }
        }
        let zfs = ZfsScript::new(&["tank", "tank/be"]);
        let property = rng.below(4) == 0;
        if property {
            zfs.set("tank/be", "org.zfsbootmenu:rootprefix", "root=custom:");
        }
        let mnt = dir.path().to_string_lossy().into_owned();
        let input = format!("os-release {:?}, property {property}", os_releases(&dir));
        let legacy: String = parity.legacy(input, || {
            legacy_core()
                .env("PATH", &zfs.path_env())
                .call("find_root_prefix", &["tank/be", &mnt], b"")
                .stdout_text()
        });
        let rust = root_prefix(
            &RealFiles,
            Some(if property { "root=custom:" } else { "-" }),
            dir.path(),
        );
        assert_eq!(format!("{rust}\n"), legacy, "case {case}");
    }
}
