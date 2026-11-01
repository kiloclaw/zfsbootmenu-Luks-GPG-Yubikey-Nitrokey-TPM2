//! What the Rust runtime's console and menu need beyond the shell: the
//! graphics drivers and the firmware of the GPUs the image is for, the
//! screen readers, and the lock of the menu.

use std::fmt::Write as _;
use std::path::Path;

use super::{Entry, Fatal, Image, Need};

/// The USB serial drivers of Braille displays, and their HID driver.
const BRAILLE_MODULES: [&str; 6] = ["usbhid", "cdc_acm", "ftdi_sio", "cp210x", "pl2303", "ch341"];

/// Where brltty keeps its configuration, drivers and tables.
const BRLTTY_DIRS: [&str; 5] = [
    "/etc/brltty",
    "/usr/share/brltty",
    "/usr/lib/brltty",
    "/usr/lib64/brltty",
    "/lib/brltty",
];

/// The firmware directories of the graphics driver `driver`, below
/// `lib/firmware`; `None` for a driver that is not one.
fn firmware_dirs(driver: &str) -> Option<&'static [&'static str]> {
    match driver {
        "amdgpu" => Some(&["amdgpu"]),
        "i915" => Some(&["i915"]),
        "xe" => Some(&["xe", "i915"]),
        // The virtual GPUs of QEMU, for virtual machines and the VM tests
        "virtio_gpu" | "bochs" => Some(&[]),
        _ => None,
    }
}

/// The graphics drivers of `zfsbootmenu_graphics_drivers` (generate-zbm
/// sets it from `Kernel.GraphicsDrivers`), with the firmware they list;
/// then, of their firmware directories, only the files
/// `zfsbootmenu_graphics_firmware` names (`firmware`, which
/// `zbm image prune-firmware` keeps). dracut must not leave the drivers or
/// `drm` out (`unomit`). The image lists the drivers in
/// `/etc/zfsbootmenu/graphics-drivers` and keeps udev from loading them,
/// since the runtime loads them, for the GPUs `zbm.gpu` chooses.
///
/// # Errors
///
/// For the legacy runtime, which takes no graphics drivers, or a driver
/// that is not one.
pub fn graphics(image: &mut Image) -> Result<(), Fatal> {
    let Some(listed) = image
        .config
        .get("zfsbootmenu_graphics_drivers")
        .map(str::to_owned)
    else {
        return Ok(());
    };
    if !image.is_rust() {
        return Err(
            "graphics drivers (zfsbootmenu_graphics_drivers) need zfsbootmenu_runtime=rust".into(),
        );
    }
    let drivers = image.config.words("zfsbootmenu_graphics_drivers");
    let mut dirs: Vec<String> = Vec::new();
    for driver in &drivers {
        let of =
            firmware_dirs(driver).ok_or_else(|| format!("unknown graphics driver '{driver}'"))?;
        dirs.extend(of.iter().map(|dir| (*dir).to_owned()));
    }
    for name in drivers.iter().map(String::as_str).chain(["drm"]) {
        image.plan.add(Entry::Unomit {
            name: name.to_owned(),
        });
    }
    for driver in &drivers {
        image.plan.module(driver, Need::Required, true);
    }
    image.plan.write(
        "/etc/zfsbootmenu/graphics-drivers",
        0o644,
        format!("{listed}\n"),
    );
    let blacklist = drivers.iter().fold(String::new(), |mut text, driver| {
        let _ = writeln!(text, "blacklist {driver}");
        text
    });
    image.plan.write(
        "/etc/modprobe.d/zfsbootmenu-graphics.conf",
        0o644,
        blacklist,
    );
    let names = image.config.words("zfsbootmenu_graphics_firmware");
    if !names.is_empty() {
        image.plan.add(Entry::Firmware { dirs, names });
    }
    Ok(())
}

/// The screen readers (ADR 0011). With `zfsbootmenu_brltty=yes`, brltty
/// with its configuration, drivers and tables, and the USB serial drivers
/// of Braille displays; with `zfsbootmenu_speakup=<synth>`, the speakup
/// modules of that synthesiser (soft: with espeakup, when the host has
/// it). An early-setup hook starts them before anything is asked.
///
/// # Errors
///
/// When brltty is asked for and the host has none.
pub fn screen_readers(image: &mut Image) -> Result<(), Fatal> {
    let brltty = image.config.is_on("zfsbootmenu_brltty", false);
    let synth = image.config.get("zfsbootmenu_speakup").map(str::to_owned);
    if !brltty && synth.is_none() {
        return Ok(());
    }
    let mut hook = String::from(
        "#!/bin/sh\n## Start the screen readers of the image, before anything is asked\n\
         ## (zfsbootmenu_brltty, zfsbootmenu_speakup).\n",
    );
    if brltty {
        if image.executable("brltty").is_none() {
            return Err("zfsbootmenu_brltty is set, but this host has no brltty".into());
        }
        if image.host.exists(Path::new("/etc/brltty.conf")) {
            image.install(Path::new("/etc/brltty.conf"), Path::new("/etc/brltty.conf"));
        }
        for dir in BRLTTY_DIRS.map(Path::new) {
            for (path, found) in image.host.walk(dir) {
                if found != super::Found::File {
                    continue;
                }
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                if name.ends_with(".so") || name.contains(".so.") {
                    image.plan.add(Entry::Library { src: path });
                } else {
                    image.install(&path, &path.clone());
                }
            }
        }
        for module in BRAILLE_MODULES {
            image.plan.module(module, Need::Optional, true);
        }
        hook.push_str("brltty -q >/dev/null 2>&1 || true\n");
    }
    if let Some(synth) = synth {
        image.plan.module("speakup", Need::Required, true);
        image
            .plan
            .module(&format!("speakup_{synth}"), Need::Required, true);
        let _ = writeln!(hook, "modprobe speakup_{synth} >/dev/null 2>&1 || true");
        if synth == "soft" && image.executable("espeakup").is_some() {
            hook.push_str("espeakup >/dev/null 2>&1 || true\n");
        }
    }
    hook.push_str("exit 0\n");
    image.plan.write(
        "/libexec/hooks/early-setup.d/05-screen-readers.sh",
        0o755,
        hook,
    );
    Ok(())
}

/// The lock of the menu (ADR 0010): the Argon2id hash of its PIN, which
/// `generate-zbm --lock-hash` writes, in `/etc/zfsbootmenu/lock`, readable
/// by root alone.
///
/// # Errors
///
/// Rather than leave the image open: for the legacy runtime, which has no
/// lock, or a hash that is not Argon2id's in the PHC format.
pub fn lock(image: &mut Image) -> Result<(), Fatal> {
    let Some(hash) = image.config.get("zfsbootmenu_lock_hash").map(str::to_owned) else {
        return Ok(());
    };
    if !image.is_rust() {
        return Err(
            "the locked menu (zfsbootmenu_lock_hash) needs zfsbootmenu_runtime=rust".into(),
        );
    }
    if !hash.starts_with("$argon2id$") {
        return Err(
            "zfsbootmenu_lock_hash is no Argon2id hash; write one with generate-zbm --lock-hash"
                .into(),
        );
    }
    image
        .plan
        .write("/etc/zfsbootmenu/lock", 0o400, format!("{hash}\n"));
    Ok(())
}
