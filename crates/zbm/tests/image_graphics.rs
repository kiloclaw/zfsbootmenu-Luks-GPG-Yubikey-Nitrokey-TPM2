//! Tests of `install_zbm_graphics` in `legacy/zfsbootmenu/install-helpers.sh`
//! (ROADMAP.md #17): the graphics drivers are installed past dracut's
//! `omit_drivers`, and their firmware directories keep only the files
//! `zfsbootmenu_graphics_firmware` names. The helper is shell code, so its
//! runs are recorded as golden files (`zbm_testkit::parity`), as in
//! `image.rs`.

#![allow(
    clippy::disallowed_methods,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    reason = "integration tests build images from real files and fail loudly when they cannot"
)]

use zbm_testkit::legacy::Bash;
use zbm_testkit::parity::Parity;
use zbm_testkit::{TempDir, parity};

/// Stubs for the dracut and mkinitcpio functions the helper uses; each
/// records its call, and the installers the omitted drivers they see.
const STUBS: &str = r#"
instmods() { echo "instmods $* (omit: $omit_drivers)"; [ -z "$FAIL_INSTALL" ]; }
add_module() { echo "add_module $*"; [ -z "$FAIL_INSTALL" ]; }
dinfo() { echo "info: $*" >&2; }
msg2() { echo "info: $*" >&2; }
dwarning() { echo "warning: $*" >&2; }
warning() { echo "warning: $*" >&2; }
derror() { echo "error: $*" >&2; }
error() { echo "error: $*" >&2; }
"#;

/// What the drivers brought into the image, below `usr/lib/firmware`, which
/// `lib` links to: compressed files, and a link to a file of another chip.
const FIRMWARE: &[&str] = &[
    "amdgpu/gc_12_0_0_me.bin.zst",
    "amdgpu/gc_12_0_1_me.bin.zst",
    "amdgpu/navi10_me.bin.zst",
    "amdgpu/psp_14_0_2_sos.bin",
    "i915/kbl_dmc_ver1_04.bin.xz",
    "i915/tgl_dmc.bin.zst",
    "xe/lnl_guc_70.bin.zst",
    "rtl_nic/rtl8168h-2.fw",
];

/// The result of a run: exit status, standard output, standard error, the
/// firmware files left in the image, and the image's list of drivers and
/// its modprobe.d file for them.
type Run = (i32, String, String, Vec<String>, [String; 2]);

/// Runs `install_zbm_graphics` with the build style, runtime and settings,
/// on an image with [`FIRMWARE`].
fn run(parity: &mut Parity, style: &str, runtime: &str, settings: &[(&str, &str)]) -> Run {
    parity.legacy(format!("{style} {runtime} {settings:?}"), || {
        let image = TempDir::new("zbm-graphics");
        for file in FIRMWARE {
            image.write(format!("usr/lib/firmware/{file}"), *file);
        }
        std::os::unix::fs::symlink(
            "gc_12_0_1_me.bin.zst",
            image.join("usr/lib/firmware/amdgpu/link.bin"),
        )
        .unwrap();
        std::os::unix::fs::symlink("usr/lib", image.join("lib")).unwrap();
        let mut bash = Bash::new()
            .code(STUBS)
            .source("zfsbootmenu/install-helpers.sh")
            .code(&format!("ZBM_BUILDSTYLE={style}; ZBM_RUNTIME={runtime}"))
            .env("BUILDROOT", &image.path().to_string_lossy())
            .env(
                "omit_drivers",
                "^amdgpu$|^radeon$|^nvidia$|^nouveau$|^i915$|^drm$|^intel_ish_ipc$",
            );
        for (key, value) in settings {
            bash = bash.env(key, value);
        }
        let output = bash.call("install_zbm_graphics", &[], b"");
        let root = image.join("usr/lib/firmware");
        let mut files = Vec::new();
        for dir in ["amdgpu", "i915", "xe", "rtl_nic"] {
            for entry in std::fs::read_dir(root.join(dir)).unwrap().flatten() {
                files.push(format!("{dir}/{}", entry.file_name().to_string_lossy()));
            }
        }
        files.sort();
        let read = |path: &str| std::fs::read_to_string(image.join(path)).unwrap_or_default();
        (
            output.status.shell_code(),
            output.stdout_text(),
            output
                .stderr_text()
                .replace(&image.path().display().to_string(), "<image>"),
            files,
            [
                read("etc/zfsbootmenu/graphics-drivers"),
                read("etc/modprobe.d/zfsbootmenu-graphics.conf"),
            ],
        )
    })
}

#[test]
fn only_the_listed_firmware_stays() {
    let mut parity = parity!("only_the_listed_firmware_stays");
    let settings = [
        ("zfsbootmenu_graphics_drivers", "amdgpu i915"),
        (
            "zfsbootmenu_graphics_firmware",
            "amdgpu/gc_12_0_0_me.bin amdgpu/link.bin amdgpu/psp_14_0_2_sos.bin \
             i915/kbl_dmc_ver1_04.bin amdgpu/gc_12_0_0_pfp.bin",
        ),
    ];

    let (code, stdout, stderr, files, [drivers, modprobe]) =
        run(&mut parity, "dracut", "rust", &settings);

    assert_eq!(code, 0, "{stderr}");
    // The runtime loads the drivers; udev must not.
    assert_eq!(drivers, "amdgpu i915\n");
    assert_eq!(modprobe, "blacklist amdgpu\nblacklist i915\n");
    assert_eq!(
        stdout,
        "instmods -c amdgpu (omit: ^radeon$|^nvidia$|^nouveau$|^intel_ish_ipc$)\n\
         instmods -c i915 (omit: ^radeon$|^nvidia$|^nouveau$|^intel_ish_ipc$)\n"
    );
    // The link keeps the file of the other chip it points to; xe/ is not a
    // directory of these drivers, nor rtl_nic/.
    assert_eq!(
        files,
        [
            "amdgpu/gc_12_0_0_me.bin.zst",
            "amdgpu/gc_12_0_1_me.bin.zst",
            "amdgpu/link.bin",
            "amdgpu/psp_14_0_2_sos.bin",
            "i915/kbl_dmc_ver1_04.bin.xz",
            "rtl_nic/rtl8168h-2.fw",
            "xe/lnl_guc_70.bin.zst",
        ]
    );
    assert_eq!(
        stderr,
        "warning: graphics firmware not found, left out: amdgpu/gc_12_0_0_pfp.bin \n\
         info: graphics drivers amdgpu i915: kept 5 firmware files, removed 2\n"
    );
}

#[test]
fn mkinitcpio_adds_the_drivers_and_xe_keeps_its_display_firmware() {
    let mut parity = parity!("mkinitcpio_adds_the_drivers_and_xe_keeps_its_display_firmware");
    let settings = [
        ("zfsbootmenu_graphics_drivers", "xe"),
        (
            "zfsbootmenu_graphics_firmware",
            "xe/lnl_guc_70.bin i915/tgl_dmc.bin",
        ),
    ];

    let (code, stdout, _, files, _) = run(&mut parity, "mkinitcpio", "rust", &settings);

    assert_eq!(code, 0);
    assert_eq!(stdout, "add_module xe\n");
    assert_eq!(
        files,
        [
            "amdgpu/gc_12_0_0_me.bin.zst",
            "amdgpu/gc_12_0_1_me.bin.zst",
            "amdgpu/link.bin",
            "amdgpu/navi10_me.bin.zst",
            "amdgpu/psp_14_0_2_sos.bin",
            "i915/tgl_dmc.bin.zst",
            "rtl_nic/rtl8168h-2.fw",
            "xe/lnl_guc_70.bin.zst",
        ]
    );
}

#[test]
fn builds_without_drivers_legacy_images_and_missing_firmware() {
    let mut parity = parity!("builds_without_drivers_legacy_images_and_missing_firmware");
    let drivers = ("zfsbootmenu_graphics_drivers", "amdgpu");

    let (code, stdout, stderr, files, written) = run(&mut parity, "dracut", "rust", &[]);
    assert_eq!((code, stdout.as_str(), stderr.as_str()), (0, "", ""));
    assert_eq!(written, [String::new(), String::new()]);
    assert_eq!(files.len(), FIRMWARE.len() + 1);

    let (code, stdout, stderr, _, _) = run(&mut parity, "dracut", "legacy", &[drivers]);
    assert_eq!((code, stdout.as_str()), (1, ""));
    assert_eq!(
        stderr,
        "error: graphics drivers (zfsbootmenu_graphics_drivers) need zfsbootmenu_runtime=rust\n"
    );

    let firmware = ("zfsbootmenu_graphics_firmware", "amdgpu/gc_11_0_0_me.bin");
    let (code, _, stderr, files, _) = run(&mut parity, "dracut", "rust", &[drivers, firmware]);
    assert_eq!(code, 1);
    assert_eq!(
        stderr,
        "error: none of the graphics firmware is in the image; is linux-firmware installed?\n"
    );
    assert!(
        files.iter().all(|file| !file.starts_with("amdgpu/")),
        "{files:?}"
    );

    let (code, _, stderr, _, _) = run(
        &mut parity,
        "dracut",
        "rust",
        &[("zfsbootmenu_graphics_drivers", "nouveau")],
    );
    assert_eq!(
        (code, stderr.as_str()),
        (1, "error: unknown graphics driver 'nouveau'\n")
    );
}
