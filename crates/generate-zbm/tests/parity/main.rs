//! Parity tests: `legacy/bin/generate-zbm` and the Rust `generate-zbm` run with
//! the same arguments and configuration in identical sandboxes, with fake
//! `dracut`, `mkinitcpio`, mount tools and hooks and the real `objcopy`,
//! `objdump` and `strings`. They must exit the same way, print the same
//! messages and debug log, run the same commands and leave the same files, byte
//! for byte.
//!
//! The tests are in this file, so that their golden files are named after it;
//! [`sandbox`] runs the programs and compares them, and [`fixtures`] holds the
//! configurations and fake programs that several tests share.

#![allow(
    clippy::disallowed_methods,
    reason = "the parity tests run the legacy programs"
)]

mod fixtures;
mod rust_only;
mod sandbox;

use fixtures::{
    BASIC, COMPONENTS, OBJDUMP_NO_SECTIONS, OBJDUMP_NOTHING, SIGNAL_HOOK, fake_objdump,
    setup_backups,
};
use sandbox::{Case, STUB, Setup, compare, efi_tools, kernel_image, tools};
use zbm_testkit::{can_run, parity};

#[test]
fn builds_images() {
    let mut parity = parity!("builds_images");
    let tools = (!parity.mode().needs_legacy() || tools()) && efi_tools();
    if !can_run(tools, "perl, binutils or the systemd EFI stub") {
        return;
    }
    compare(
        &mut parity,
        &Case::new(BASIC, &["-c", "{root}/config.yaml", "-b", "{root}/boot"]),
    );
    compare(
        &mut parity,
        &Case::new(
            BASIC,
            &["-d", "-c", "{root}/config.yaml", "-b", "{root}/boot"],
        ),
    );
    compare(
        &mut parity,
        &Case::new(
            BASIC,
            &[
                "-c",
                "{root}/config.yaml",
                "-b",
                "{root}/boot",
                "--kver",
                "6.1.0-test",
                "-l",
                "rw",
            ],
        ),
    );
    // The initramfs of the prefix `zbm` is named after it (G3).
    compare(
        &mut parity,
        &Case {
            differs: &[("initramfs-9.9_1.img", "initramfs-zbm-9.9_1.img")],
            ..Case::new(
                BASIC,
                &[
                    "-c",
                    "{root}/config.yaml",
                    "-k",
                    "{root}/boot/linux-lts",
                    "-p",
                    "zbm",
                    "-v",
                    "9.9",
                ],
            )
        },
    );
    // The boot partition is not mounted: it is mounted, and unmounted at the end.
    compare(
        &mut parity,
        &Case {
            env: &[("FAKE_MOUNTPOINT_STATUS", "1")],
            ..Case::new(BASIC, &["-c", "{root}/config.yaml", "-b", "{root}/boot"])
        },
    );
}

#[test]
fn keeps_versions() {
    let mut parity = parity!("keeps_versions");
    let tools = (!parity.mode().needs_legacy() || tools()) && efi_tools();
    if !can_run(tools, "perl, binutils or the systemd EFI stub") {
        return;
    }
    let versioned = "Global:
  ManageImages: true
  DracutConfDir: {root}/dracut.conf.d
  Version: 3.1.0
Components:
  ImageDir: {root}/esp/EFI/zbm
  Versions: 2
  Enabled: true
EFI:
  ImageDir: {root}/esp/EFI/zbm
  Versions: 2
  Enabled: true
  Stub: /usr/lib/systemd/boot/efi/linuxx64.efi.stub
";
    let setup: Setup = |sandbox| {
        std::fs::create_dir_all(sandbox.path().join("esp/EFI/zbm")).unwrap();
        for name in [
            "vmlinuz-3.1.0_1",
            "vmlinuz-3.1.0_2",
            "vmlinuz-3.1.0_3",
            "vmlinuz-3.0.0_1",
            "vmlinuz-2.0_1",
            "vmlinuz-2.0_2",
            "vmlinuz-1.0_1",
            "initramfs-3.1.0_1.img",
            "initramfs-3.1.0_2.img",
            "initramfs-3.1.0_3.img",
            "initramfs-3.0.0_1.img",
            "initramfs-2.0_2.img",
            "initramfs-1.0_1.img",
            "vmlinuz-3.1.0_1.EFI",
            "vmlinuz-3.1.0_7.EFI",
            "vmlinuz-3.0.0_1.EFI",
            "vmlinuz-2.0_4.EFI",
            "VMLINUZ-2.0_5.EFI",
            "vmlinuz-backup.EFI",
        ] {
            sandbox.dir.write(format!("esp/EFI/zbm/{name}"), name);
        }
    };
    compare(
        &mut parity,
        &Case {
            setup,
            ..Case::new(
                versioned,
                &["-d", "-c", "{root}/config.yaml", "-b", "{root}/boot"],
            )
        },
    );
    // Without versions, existing images become backups.
    let plain = "Global:
  ManageImages: true
Components:
  ImageDir: {root}/esp/EFI/zbm
  Enabled: true
EFI:
  ImageDir: {root}/esp/EFI/zbm
  Enabled: true
  Stub: /usr/lib/systemd/boot/efi/linuxx64.efi.stub
  SplashImage: {root}/splash.bmp
  DeviceTree: {root}/dtb-%{kernel}.dtb
";
    compare(
        &mut parity,
        &Case {
            setup: |sandbox| {
                setup_backups(sandbox);
                sandbox.dir.write("splash.bmp", "BM splash");
                sandbox.dir.write("dtb-6.2.5-test.dtb", "device tree");
            },
            ..Case::new(
                plain,
                &["-d", "-c", "{root}/config.yaml", "-b", "{root}/boot"],
            )
        },
    );
}

#[test]
fn chooses_kernels() {
    let mut parity = parity!("chooses_kernels");
    if !can_run(!parity.mode().needs_legacy() || tools(), "perl or binutils") {
        return;
    }
    let args: &[&[&str]] = &[
        &["-c", "{root}/config.yaml", "-b", "{root}/boot", "-d"],
        &[
            "-c",
            "{root}/config.yaml",
            "-b",
            "{root}/boot",
            "-K",
            "6.1*",
        ],
        &["-c", "{root}/config.yaml", "-b", "{root}/boot", "-K", "7.0"],
        &[
            "-c",
            "{root}/config.yaml",
            "-b",
            "{root}/boot",
            "-K",
            "5.10.0",
        ],
        &["-c", "{root}/config.yaml", "-k", "{root}/boot/none"],
        &[
            "-c",
            "{root}/config.yaml",
            "-k",
            "{root}/boot/vmlinuz-5.10.0",
        ],
        &["-c", "{root}/config.yaml", "-b", "{root}/empty"],
        &[
            "-c",
            "{root}/config.yaml",
            "-b",
            "{root}/boot",
            "-K",
            "%current",
        ],
    ];
    for list in args {
        compare(&mut parity, &Case::new(COMPONENTS, list));
    }
    // `uname -r` printing nothing is an error; the Perl program used to exit with
    // status 0.
    let outcome = compare(
        &mut parity,
        &Case {
            env: &[("FAKE_UNAME", "none")],
            ..Case::new(
                COMPONENTS,
                &[
                    "-c",
                    "{root}/config.yaml",
                    "-b",
                    "{root}/boot",
                    "-K",
                    "%{CURRENT}",
                ],
            )
        },
    );
    assert_eq!(outcome.code, 1);
    assert!(
        outcome
            .stdout
            .ends_with("Cannot determine current kernel version\n")
    );
    let configured = "Global:
  ManageImages: true
Components:
  ImageDir: {root}/esp/EFI/zbm
  Enabled: true
Kernel:
  Path: {root}/boot/vmlinuz-6.1.0-test
  Version: \"%current\"
  Prefix: kernel
";
    compare(
        &mut parity,
        &Case {
            differs: &[("initramfs-bootmenu.img", "initramfs-kernel-bootmenu.img")],
            ..Case::new(configured, &["-c", "{root}/config.yaml", "-d"])
        },
    );
}

#[test]
fn initramfs_generators() {
    let mut parity = parity!("initramfs_generators");
    if !can_run(!parity.mode().needs_legacy() || tools(), "perl or binutils") {
        return;
    }
    let cpio = "Global:
  ManageImages: true
  InitCPIO: true
  InitCPIOConfig: {root}/mkinitcpio.conf
  InitCPIOHookDirs:
    - {root}/hooks
    - {root}/more hooks
  InitCPIOFlags: -S autodetect
Components:
  ImageDir: {root}/esp/EFI/zbm
  Enabled: true
";
    compare(
        &mut parity,
        &Case::new(cpio, &["-c", "{root}/config.yaml", "-b", "{root}/boot"]),
    );
    compare(
        &mut parity,
        &Case::new(
            cpio,
            &[
                "-c",
                "{root}/config.yaml",
                "-b",
                "{root}/boot",
                "-H",
                "{root}/h",
                "-d",
            ],
        ),
    );
    compare(
        &mut parity,
        &Case::new(
            cpio,
            &[
                "-c",
                "{root}/config.yaml",
                "-b",
                "{root}/boot",
                "--no-initcpio",
            ],
        ),
    );
    let flags = "Global:
  ManageImages: true
  DracutFlags:
    - \"--add-drivers 'a b'\"
    - --no-early-microcode
Components:
  ImageDir: {root}/esp/EFI/zbm
  Enabled: true
";
    compare(
        &mut parity,
        &Case::new(
            flags,
            &[
                "-c",
                "{root}/config.yaml",
                "-b",
                "{root}/boot",
                "-C",
                "{root}/conf d",
            ],
        ),
    );
    compare(
        &mut parity,
        &Case::new(
            flags,
            &["-c", "{root}/config.yaml", "-b", "{root}/boot", "-i"],
        ),
    );
    compare(
        &mut parity,
        &Case {
            env: &[("FAKE_DRACUT_FAIL", "3")],
            ..Case::new(flags, &["-c", "{root}/config.yaml", "-b", "{root}/boot"])
        },
    );
}

#[test]
fn configuration_states() {
    let mut parity = parity!("configuration_states");
    if !can_run(!parity.mode().needs_legacy() || tools(), "perl or binutils") {
        return;
    }
    let disabled =
        "Global:\n  ManageImages: false\n  Other: &x {a: 1}\n  List: [1, 'two', 3.10]\nCopy: *x\n";
    compare(
        &mut parity,
        &Case::new(disabled, &["-c", "{root}/config.yaml"]),
    );
    compare(
        &mut parity,
        &Case::new(disabled, &["-c", "{root}/config.yaml", "--enable"]),
    );
    compare(
        &mut parity,
        &Case::new(BASIC, &["-c", "{root}/config.yaml", "--disable"]),
    );
    compare(
        &mut parity,
        &Case::new(
            BASIC,
            &["-c", "{root}/config.yaml", "--enable", "--disable"],
        ),
    );
    compare(
        &mut parity,
        &Case::new(BASIC, &["-c", "{root}/missing.yaml"]),
    );
    compare(
        &mut parity,
        &Case::new("a: [\n", &["-c", "{root}/config.yaml"]),
    );
    compare(
        &mut parity,
        &Case::new("- a\n", &["-c", "{root}/config.yaml"]),
    );
    compare(
        &mut parity,
        &Case::new(
            "EFI: text\nGlobal:\n  ManageImages: 1\n",
            &["-c", "{root}/config.yaml", "-b", "{root}/boot"],
        ),
    );
    compare(
        &mut parity,
        &Case::new("Global: [1]\n", &["-c", "{root}/config.yaml"]),
    );
    compare(&mut parity, &Case::new(BASIC, &["-V"]));
    compare(&mut parity, &Case::new(BASIC, &["--bogus", "-c"]));
}

#[test]
fn mounts_and_hooks() {
    let mut parity = parity!("mounts_and_hooks");
    if !can_run(!parity.mode().needs_legacy() || tools(), "perl or binutils") {
        return;
    }
    let config = "Global:
  ManageImages: true
  BootMountPoint: {root}/my esp
  PreHooksDir: {root}/pre.d
  PostHooksDir: {root}/post.d
Components:
  ImageDir: {root}/esp/zbm
  Enabled: true
";
    compare(
        &mut parity,
        &Case {
            env: &[("FAKE_MOUNTPOINT_STATUS", "1"), ("FAKE_MOUNT_FAIL", "1")],
            ..Case::new(config, &["-c", "{root}/config.yaml", "-b", "{root}/boot"])
        },
    );
    compare(
        &mut parity,
        &Case {
            env: &[("FAKE_MOUNTPOINT_STATUS", "1")],
            ..Case::new(
                config,
                &["-d", "-c", "{root}/config.yaml", "-b", "{root}/boot"],
            )
        },
    );
    // A hook that sends SIGTERM to the generator: the partition is unmounted once
    // and the generator exits. The Perl program used to unmount twice, once in the
    // handler and again in its END block.
    let outcome = compare(
        &mut parity,
        &Case {
            env: &[("FAKE_MOUNTPOINT_STATUS", "1")],
            setup: |sandbox| {
                sandbox.dir.write_executable("pre.d/05-signal", SIGNAL_HOOK);
            },
            ..Case::new(config, &["-c", "{root}/config.yaml", "-b", "{root}/boot"])
        },
    );
    assert_eq!(outcome.code, 1);
    assert!(
        outcome
            .stdout
            .ends_with("<program> terminating on signal TERM\n")
    );
    assert_eq!(outcome.stdout.matches("Unmounting").count(), 1);
    assert_eq!(outcome.state.matches("umount [").count(), 1);
    // A hook killed by a signal reports 128 plus the signal number.
    let outcome = compare(
        &mut parity,
        &Case {
            setup: |sandbox| {
                sandbox.dir.write_executable(
                    "pre.d/05-killed",
                    "#!/bin/sh
kill -KILL $$
",
                );
            },
            ..Case::new(
                config,
                &["-d", "-c", "{root}/config.yaml", "-b", "{root}/boot"],
            )
        },
    );
    assert!(outcome.log.contains("## 137\n"), "{}", outcome.log);
    // A boot directory and kernel names with spaces and quotes, which the Perl
    // program used to pass to the shell unquoted.
    let outcome = compare(
        &mut parity,
        &Case {
            differs: &[(
                "initramfs-bootmenu.img",
                "initramfs-kernel it's-bootmenu.img",
            )],
            setup: |sandbox| {
                std::fs::create_dir_all(sandbox.path().join("my boot")).unwrap();
                sandbox.dir.write(
                    "my boot/kernel it's",
                    kernel_image(&["Linux version 6.3.0-it's (zbm)"]),
                );
            },
            ..Case::new(
                config,
                &[
                    "-c",
                    "{root}/config.yaml",
                    "-k",
                    "{root}/my boot/kernel it's",
                    "-d",
                ],
            )
        },
    );
    assert_eq!(outcome.code, 0, "{}", outcome.stdout);
    assert!(
        outcome.log.contains("using 6.3.0-it's from kernel strings"),
        "{}",
        outcome.log
    );
    assert!(
        outcome
            .state
            .contains("esp/zbm/kernel it's-bootmenu: 6000 bytes"),
        "{}",
        outcome.state
    );
}

#[test]
fn bundle_failures() {
    let mut parity = parity!("bundle_failures");
    let tools = (!parity.mode().needs_legacy() || tools()) && efi_tools();
    if !can_run(tools, "perl, binutils or the systemd EFI stub") {
        return;
    }
    let config = "Global:
  ManageImages: true
EFI:
  ImageDir: {root}/esp/EFI/zbm
  Enabled: true
  Stub: {root}/stub.efi
";
    compare(
        &mut parity,
        &Case::new(config, &["-c", "{root}/config.yaml", "-b", "{root}/boot"]),
    );
    // An objdump that reports no section alignment or sections: the Perl program
    // used to go on with offsets it could not compute.
    let cases: [(Setup, &str); 2] = [
        (
            |sandbox| fake_objdump(sandbox, OBJDUMP_NOTHING),
            "Unable to determine stub alignment!\n",
        ),
        (
            |sandbox| fake_objdump(sandbox, OBJDUMP_NO_SECTIONS),
            "Unable to determine initial stub offset!\n",
        ),
    ];
    for (setup, expected) in cases {
        let outcome = compare(
            &mut parity,
            &Case {
                setup,
                ..Case::new(config, &["-c", "{root}/config.yaml", "-b", "{root}/boot"])
            },
        );
        assert_eq!(outcome.code, 1);
        assert!(outcome.stdout.ends_with(expected), "{}", outcome.stdout);
    }
    let device_tree = "Global:
  ManageImages: true
EFI:
  ImageDir: {root}/esp/EFI/zbm
  Enabled: true
  Stub: /usr/lib/systemd/boot/efi/linuxx64.efi.stub
  DeviceTree: board.dtb
";
    compare(
        &mut parity,
        &Case::new(
            device_tree,
            &["-c", "{root}/config.yaml", "-b", "{root}/boot"],
        ),
    );
    // An image directory that cannot be created.
    compare(
        &mut parity,
        &Case {
            setup: |sandbox| {
                sandbox.dir.write("esp/EFI", "a file");
            },
            ..Case::new(
                &config.replace("{root}/stub.efi", STUB),
                &["-c", "{root}/config.yaml", "-b", "{root}/boot"],
            )
        },
    );
}
