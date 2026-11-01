//! Golden plans (ADR 0006, answer 4): the plans of a set of
//! configurations, against `src/snapshots/`, so that a change to what goes
//! into an image is a diff to review. `cargo insta review` takes a changed
//! plan. The build host is fake, with the real module root of
//! `legacy/zfsbootmenu`.

#![allow(
    clippy::disallowed_methods,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests copy the checkout's module root and fail loudly when they cannot"
)]

use std::path::Path;

use zbm_platform::Files as _;
use zbm_testkit::FakeRunner;
use zbm_testkit::fakes::FakeFiles;

use super::lists::{
    ESSENTIAL_BINARIES, LUKS_BINARIES, LUKS_FIDO2_BINARIES, LUKS_LEGACY_BINARIES,
    LUKS_TPM2_BINARIES, LUKS_YUBIKEY_BINARIES, OPTIONAL_BINARIES,
};
use super::*;

/// Where the module root is on the fake host.
const MODULE_ROOT: &str = "/usr/share/zfsbootmenu";

/// The checkout's module root.
fn checkout_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../legacy/zfsbootmenu")
}

/// Copies the tree `from` of the checkout to `to` of `files`.
fn copy_tree(files: &FakeFiles, from: &Path, to: &Path) {
    let mut entries: Vec<_> = std::fs::read_dir(from)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        let name = path.file_name().unwrap();
        let dest = to.join(name);
        let metadata = std::fs::symlink_metadata(&path).unwrap();
        if metadata.file_type().is_symlink() {
            files.symlink(&dest, std::fs::read_link(&path).unwrap());
        } else if metadata.is_dir() {
            files.dir(&dest);
            copy_tree(files, &path, &dest);
        } else {
            use std::os::unix::fs::PermissionsExt;
            let mode = metadata.permissions().mode() & 0o7777;
            files.file_with_mode(&dest, std::fs::read(&path).unwrap(), mode);
        }
    }
}

/// A build host with the module root, `zbm` and brush, the programs of the
/// lists but `mbuffer`, ZFS's and dm's udev rules, `libgcc_s` and a few of
/// the host's own files.
fn host_files() -> FakeFiles {
    let files = FakeFiles::new();
    copy_tree(&files, &checkout_root(), Path::new(MODULE_ROOT));
    let programs = ESSENTIAL_BINARIES
        .iter()
        .chain(OPTIONAL_BINARIES)
        .chain(LUKS_BINARIES)
        .chain(LUKS_LEGACY_BINARIES)
        .chain(LUKS_TPM2_BINARIES)
        .chain(LUKS_FIDO2_BINARIES)
        .chain(LUKS_YUBIKEY_BINARIES)
        .chain(&[
            "gpg",
            "gpgconf",
            "gpg-connect-agent",
            "setfont",
            "brltty",
            "openssl",
        ])
        .filter(|name| **name != "mbuffer" && **name != "fido2-assert");
    for name in programs {
        files.file_with_mode(Path::new("/usr/bin").join(name), "", 0o755);
    }
    files
        .file_with_mode("/usr/libexec/zfsbootmenu/zbm", "", 0o755)
        .file_with_mode("/usr/libexec/zfsbootmenu/brush", "", 0o755)
        .file_with_mode("/usr/libexec/gpg-agent", "", 0o755)
        .file_with_mode("/usr/libexec/scdaemon", "", 0o755)
        .file("/usr/lib/udev/rules.d/90-zfs.rules", "")
        .file("/usr/lib/udev/rules.d/10-dm.rules", "")
        .file("/usr/lib/udev/rules.d/95-dm-notify.rules", "")
        .file("/usr/lib/x86_64-linux-gnu/libgcc_s.so.1", "")
        .file("/usr/lib/x86_64-linux-gnu/libfido2.so.1", "")
        .file(
            "/usr/lib/x86_64-linux-gnu/cryptsetup/libcryptsetup-token-systemd-tpm2.so",
            "",
        )
        .file("/etc/hostid", [0x3d, 0x2c, 0x1b, 0x0a])
        .file("/etc/zfs/vdev_id.conf", "alias a /dev/sda\n")
        .file("/etc/zfs/compatibility.d/grub2", "async_destroy\n")
        .file("/usr/lib/locale/C.utf8/LC_CTYPE", "")
        .file("/usr/share/terminfo/x/xterm-256color", "")
        .file("/usr/share/terminfo/l/linux", "")
        .file("/usr/share/terminfo/v/vt100", "")
        .file("/usr/share/brltty/text.tbl", "")
        .file("/usr/lib/brltty/libbrlttybba.so", "")
        .file("/etc/vconsole.conf", "XKBLAYOUT=\"de,us\"\n")
        .file("/root/hooks/setup.d/50-mine.sh", "")
        .file_with_mode("/root/hooks/teardown.d/10-bye.sh", "", 0o755)
        .file_with_mode("/etc/zfsbootmenu/hooks/boot-sel.d/10-pick.sh", "", 0o755)
        .file("/etc/zfsbootmenu/crypttab", CRYPTTAB)
        .file("/etc/zfsbootmenu/keys/root.key.gpg", "")
        .file("/etc/zfsbootmenu/keys/data.key.gpg", "")
        .file_with_mode("/lib/cryptsetup/scripts/passdev", "", 0o755);
    files
}

/// The crypttab of the fake host.
const CRYPTTAB: &str = "# name device keyfile options\r
root UUID=1 /etc/zfsbootmenu/keys/root.key.gpg luks\r
data UUID=2 /etc/zfsbootmenu/keys/data.key.gpg luks,keyscript=passdev\r
plain UUID=3 /etc/zfsbootmenu/keys/plain.key luks\r
usb UUID=4 /key:LABEL=usb luks,keyscript=/none\r
none UUID=5 none luks
";

/// What the fake host's programs answer.
fn host_runner() -> FakeRunner {
    let runner = FakeRunner::new();
    runner.respond("fzf -f abc --raw --exit-0", 2, "");
    runner.respond("zfs version", 0, "zfs-2.2.6-1\nzfs-kmod-2.2.6-1\n");
    runner.respond("hostid", 0, "0a1b2c3d\n");
    runner.respond(
        "ldconfig -p",
        0,
        "2 libs found in cache\n\
         \tlibc.so.6 (libc6,x86-64) => /lib/x86_64-linux-gnu/libc.so.6\n\
         \tlibfido2.so.1 (libc6,x86-64) => /usr/lib/x86_64-linux-gnu/libfido2.so.1\n",
    );
    runner.respond(
        "ldd /usr/bin/zpool",
        0,
        "\tlibzfs.so.6 => /lib/x86_64-linux-gnu/libzfs.so.6\n",
    );
    runner.respond("gcc-config -c", 1, "");
    runner.respond(
        "gpgconf --list-components",
        0,
        "gpg:OpenPGP:/usr/bin/gpg\ngpg-agent:Private Keys:/usr/libexec/gpg-agent\n\
         scdaemon:Smartcards:/usr/libexec/scdaemon\n",
    );
    runner.respond(
        "gpg --batch --list-only --list-packets /etc/zfsbootmenu/keys/root.key.gpg",
        2,
        ":pubkey enc packet: version 3, algo 1\n",
    );
    runner.respond(
        "gpg --batch --list-only --list-packets /etc/zfsbootmenu/keys/data.key.gpg",
        2,
        ":symkey enc packet: version 4\n",
    );
    runner.respond("gpg --batch --export ABCD1234", 0, "KEY");
    runner.respond("coreutils --list", 1, "");
    runner.respond(
        "/usr/lib/initcpio/busybox --list",
        0,
        "sh\nls\ncat\nhead\nsed\nawk\n",
    );
    runner
}

/// The plan of `builder` with the settings `settings`.
fn plan_of(
    builder: Builder,
    settings: &str,
    files: &FakeFiles,
    runner: &FakeRunner,
) -> Result<Plan, Fatal> {
    let config = Config::parse(settings);
    let host = Host {
        files,
        runner,
        path: vec!["/usr/bin".into(), "/usr/sbin".into()],
        scratch: "/tmp/zbmgpg.1".into(),
        xkb: "/nonexistent/xkb".into(),
        compose: "/usr/share/X11/locale/en_US.UTF-8/Compose".into(),
        now: 1_793_491_200,
    };
    plan(builder, &config, &host)
}

/// Checks the plan of `builder` with `settings` against its golden file.
fn golden(name: &str, builder: Builder, settings: &str) {
    let files = host_files();
    let runner = host_runner();
    let plan = plan_of(builder, settings, &files, &runner).unwrap();
    insta::with_settings!({ prepend_module_to_snapshot => false, omit_expression => true }, {
        insta::assert_snapshot!(name, plan.golden());
    });
}

/// The settings of a Rust image with everything it can have.
const RUST_ALL: &str = "zfsbootmenu_runtime\trust
zfsbootmenu_module_root\t/usr/share/zfsbootmenu
zfsbootmenu_hook_root\t/etc/zfsbootmenu/hooks
zfsbootmenu_luks_gpg_keys\tABCD1234
zfsbootmenu_graphics_drivers\ti915 virtio_gpu
zfsbootmenu_graphics_firmware\ti915/adlp_dmc.bin i915/tgl_guc_70.bin
zfsbootmenu_lock_hash\t$argon2id$v=19$m=65536,t=3,p=4$c2FsdA$aGFzaA
zfsbootmenu_brltty\tyes
zfsbootmenu_speakup\tsoft
zfsbootmenu_console_font\tspleen
zfsbootmenu_console_fallback_fonts\t/usr/share/fonts/none.ttf
zfsbootmenu_teardown[]\t/root/hooks/teardown.d/10-bye.sh
zfsbootmenu_setup\t/root/hooks/setup.d/50-mine.sh
embedded_kcl\tzbm.timeout=5 quiet='yes'
moddir\t/usr/lib/dracut/modules.d/90zfsbootmenu
SOURCE_DATE_EPOCH\t1700000000
";

#[test]
fn dracut_legacy() {
    golden(
        "dracut-legacy",
        Builder::Dracut,
        "zfsbootmenu_trace_enable\tyes\n",
    );
}

#[test]
fn dracut_rust() {
    golden("dracut-rust", Builder::Dracut, RUST_ALL);
}

#[test]
fn dracut_rust_release() {
    golden(
        "dracut-rust-release",
        Builder::Dracut,
        "zfsbootmenu_runtime\trust\nzfsbootmenu_release_build\tyes\nzfsbootmenu_luks\tno\n\
         zfsbootmenu_console_faces\tregular\nzfsbootmenu_keyboard_layouts\tus,fr\n",
    );
}

#[test]
fn mkinitcpio_legacy_miser() {
    golden(
        "mkinitcpio-legacy-miser",
        Builder::Mkinitcpio,
        "zfsbootmenu_miser\tyes\n",
    );
}

#[test]
fn mkinitcpio_rust() {
    golden("mkinitcpio-rust", Builder::Mkinitcpio, RUST_ALL);
}

/// Makes `runner` answer `line` so, before what it answered before.
fn answer(runner: &FakeRunner, line: &str, status: u8, stdout: &str) {
    let line = line.to_owned();
    let output = zbm_platform::Output::new(zbm_platform::Status::exited(status), stdout.as_bytes());
    runner.first(move |command| (command.display() == line).then(|| Ok(output.clone())));
}

/// The plan of `builder` with `settings` on the fake host as `change`
/// leaves it, as the golden files keep it, or why it cannot be built.
fn plan_with(
    builder: Builder,
    settings: &str,
    change: impl Fn(&FakeFiles, &FakeRunner),
) -> Result<String, Fatal> {
    let files = host_files();
    let runner = host_runner();
    change(&files, &runner);
    plan_of(builder, settings, &files, &runner).map(|plan| plan.golden())
}

#[test]
fn what_an_image_cannot_have_stops_the_build() {
    let none = |_: &FakeFiles, _: &FakeRunner| {};
    let rust = "zfsbootmenu_runtime\trust\n";
    let cases = [
        (
            format!("{rust}zfsbootmenu_zbm\t/none/zbm\n"),
            "zbm not found at /none/zbm",
        ),
        (
            "zfsbootmenu_graphics_drivers\ti915\n".to_owned(),
            "need zfsbootmenu_runtime=rust",
        ),
        (
            format!("{rust}zfsbootmenu_graphics_drivers\tnouveau\n"),
            "unknown graphics driver 'nouveau'",
        ),
        (
            "zfsbootmenu_lock_hash\t$argon2id$x\n".to_owned(),
            "needs zfsbootmenu_runtime=rust",
        ),
        (
            format!("{rust}zfsbootmenu_lock_hash\t$2b$hash\n"),
            "no Argon2id hash",
        ),
    ];
    for (settings, why) in cases {
        let fatal = plan_with(Builder::Dracut, &settings, none).unwrap_err();
        assert!(fatal.contains(why), "{settings}: {fatal}");
    }
    let no_brltty = plan_with(
        Builder::Mkinitcpio,
        "zfsbootmenu_brltty\tyes\n",
        |files, _| {
            files.remove_file(Path::new("/usr/bin/brltty")).unwrap();
        },
    );
    assert!(no_brltty.unwrap_err().contains("no brltty"));
    let no_libgcc = plan_with(Builder::Dracut, "", |files, _| {
        files
            .remove_file(Path::new("/usr/lib/x86_64-linux-gnu/libgcc_s.so.1"))
            .unwrap();
    });
    assert_eq!(no_libgcc.unwrap_err(), "Unable to locate libgcc_s.so");
    let legacy_no_zbm = plan_with(Builder::Dracut, "zfsbootmenu_zbm\t/none/zbm\n", none).unwrap();
    assert!(legacy_no_zbm.contains("info\tzbm not found at /none/zbm; the legacy runtime"));
    let missing = plan_with(Builder::Dracut, "", |files, _| {
        files.remove_file(Path::new("/usr/bin/zfs")).unwrap();
    });
    assert_eq!(
        missing.unwrap_err(),
        "failed to install essential executable 'zfs'"
    );
}

#[test]
fn uutils_gives_its_programs_as_hard_links() {
    for builder in [Builder::Dracut, Builder::Mkinitcpio] {
        let plan = plan_with(builder, "", |files, runner| {
            files.file_with_mode("/usr/bin/coreutils", "", 0o755);
            answer(runner, "coreutils --list", 0, "cat\nhead\nmkdir\nsort\n");
        })
        .unwrap();
        let lines: Vec<&str> = plan.lines().collect();
        let binary = "binary\t/usr/bin/coreutils\t/usr/bin/coreutils";
        let installed = lines.iter().position(|line| *line == binary);
        assert!(installed.is_some(), "{plan}");
        for name in ["cat", "head", "mkdir", "sort"] {
            let link = format!("hardlink\t/usr/bin/coreutils\t/usr/bin/{name}");
            let linked = lines.iter().position(|line| *line == link);
            // A hard link needs its file in the image first.
            assert!(linked > installed, "{name} before the binary:\n{plan}");
            assert!(!plan.contains(&format!("binary\t/usr/bin/{name}\t")));
        }
    }
}

#[test]
fn every_link_follows_what_it_links_to() {
    let plan = plan_with(Builder::Dracut, "", |files, runner| {
        files.file_with_mode("/usr/bin/coreutils", "", 0o755);
        answer(
            runner,
            "coreutils --list",
            0,
            "cat\nhead\nmkdir\ncut\ntr\nwc\n",
        );
    })
    .unwrap();
    let mut placed: Vec<&str> = Vec::new();
    for line in plan.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        match fields.as_slice() {
            ["hardlink", target, dest] => {
                assert!(placed.contains(target), "{dest} before {target}:\n{plan}");
                placed.push(dest);
            }
            ["binary" | "file" | "link", _, dest, ..] => placed.push(dest),
            _ => {}
        }
    }
}

#[test]
fn libgcc_s_is_gccs_current_one_or_any_of_gccs() {
    let gcc = "/usr/lib/gcc/x86_64-pc-linux-gnu/14/libgcc_s.so.1";
    let current = plan_with(Builder::Dracut, "", |files, runner| {
        files.file(gcc, "");
        answer(runner, "gcc-config -c", 0, "x86_64-pc-linux-gnu-14\n");
    })
    .unwrap();
    assert!(current.contains(&format!("library\t{gcc}\n")), "{current}");
    let any = plan_with(Builder::Dracut, "", |files, _| {
        files
            .remove_file(Path::new("/usr/lib/x86_64-linux-gnu/libgcc_s.so.1"))
            .unwrap();
        files.file("/usr/lib/gcc/aarch64/13/libgcc_s.so.1", "");
    })
    .unwrap();
    assert!(any.contains("library\t/usr/lib/gcc/aarch64/13/libgcc_s.so.1\n"));
    let linked = plan_with(Builder::Dracut, "", |_, runner| {
        answer(
            runner,
            "ldd /usr/bin/zpool",
            0,
            "libgcc_s.so.1 => /lib64/libgcc_s.so.1\n",
        );
    })
    .unwrap();
    assert!(!linked.contains("libgcc_s"));
    let skipped = plan_with(Builder::Dracut, "zfsbootmenu_skip_gcc_s\tYes\n", |_, _| {}).unwrap();
    assert!(!skipped.contains("libgcc_s"));
}

#[test]
fn clevis_pcscd_and_key_files_are_taken_as_the_host_has_them() {
    let settings = "zfsbootmenu_luks_pcscd\tyes\n";
    let plan = plan_with(Builder::Dracut, settings, |files, runner| {
        files
            .file_with_mode("/usr/bin/clevis", "", 0o755)
            .file_with_mode("/usr/bin/clevis-luks-unlock", "", 0o755)
            .file("/usr/bin/clevis-luks-common-functions", "")
            .file_with_mode("/usr/bin/pcscd", "", 0o755)
            .file("/usr/lib/pcsc/drivers/ifd-ccid.bundle/Info.plist", "")
            .file("/etc/libccid_Info.plist", "")
            .remove_file(Path::new("/etc/zfsbootmenu/keys/data.key.gpg"))
            .unwrap();
        answer(
            runner,
            "gpg --batch --list-only --list-packets /etc/zfsbootmenu/keys/root.key.gpg",
            2,
            "gpg: no valid OpenPGP data found.\n",
        );
    })
    .unwrap();
    for line in [
        "binary\t/usr/bin/clevis\t/usr/bin/clevis\n",
        "binary\t/usr/bin/clevis-luks-unlock\t/usr/bin/clevis-luks-unlock\n",
        "file\t/usr/bin/clevis-luks-common-functions\t/usr/bin/clevis-luks-common-functions\t-\n",
        "binary\t/usr/bin/pcscd\t/usr/bin/pcscd\n",
        "dir\t/usr/lib/pcsc/drivers\t",
        "file\t/usr/lib/pcsc/drivers/ifd-ccid.bundle/Info.plist\t",
        "file\t/etc/libccid_Info.plist\t/etc/libccid_Info.plist\t-\n",
        "warn\t/etc/zfsbootmenu/keys/root.key.gpg is not GnuPG-encrypted; refusing",
        "warn\tGnuPG key file /etc/zfsbootmenu/keys/data.key.gpg not found\n",
    ] {
        assert!(plan.contains(line), "{line}\n{plan}");
    }
    let without = plan_with(Builder::Dracut, "zfsbootmenu_luks_tokens\tno\n", |_, _| {}).unwrap();
    assert!(!without.contains("tpm2_unseal") && !without.contains("libfido2"));
    let no_cryptsetup = plan_with(Builder::Mkinitcpio, "", |files, _| {
        files.remove_file(Path::new("/usr/bin/cryptsetup")).unwrap();
    })
    .unwrap();
    assert!(no_cryptsetup.contains("warn\tcryptsetup not found; ZFS pools on LUKS volumes"));
}
