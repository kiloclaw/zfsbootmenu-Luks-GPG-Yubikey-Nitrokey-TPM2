//! The sections in the order of each builder: what the dracut module
//! (`module-setup.sh`) and the mkinitcpio hook (`install/zfsbootmenu`)
//! installed, in their order.

use std::path::{Path, PathBuf};

use zbm_tools::host::{busybox_programs, hostid, zfs_version};

use super::lists::{
    ESSENTIAL_BINARIES, ESSENTIAL_MODULES, LEGACY_BINARIES, LUKS_MODULES, LUKS_TOKEN_MODULES,
    LUKS_UDEV_RULES, OPTIONAL_BINARIES, OPTIONAL_MODULES, UDEV_RULES,
};
use super::{Entry, Fatal, Image, Need, core, fonts, graphics, libs, luks, shell};

/// Where udev rules may be on the host.
const RULE_DIRS: [&str; 3] = [
    "/etc/udev/rules.d",
    "/usr/lib/udev/rules.d",
    "/lib/udev/rules.d",
];

/// The compatibility feature sets of `zpool create -o compatibility`.
const COMPAT_DIRS: [&str; 2] = ["/etc/zfs/compatibility.d", "/usr/share/zfs/compatibility.d"];

/// Where dracut keeps ZFSBootMenu's module, when `moddir` is not given.
const MODDIR: &str = "/usr/lib/dracut/modules.d/90zfsbootmenu";

/// Whether the host has the udev rule `name`.
fn has_rule(image: &Image, name: &str) -> bool {
    RULE_DIRS
        .iter()
        .any(|dir| image.host.exists(&Path::new(dir).join(name)))
}

/// The udev rules: ZFS's, and dm's for LUKS where the host has them.
fn rules(image: &mut Image, names: &[&str], need: Need) {
    for name in names {
        if need == Need::Required || has_rule(image, name) {
            image.plan.add(Entry::Rule {
                name: (*name).to_owned(),
                need,
            });
        }
    }
}

/// The modules `names`, the same need each.
fn modules(image: &mut Image, names: &[&str], need: Need) {
    for name in names {
        image.plan.module(name, need, false);
    }
}

/// The essential program that legacy images list [`LEGACY_BINARIES`]
/// after. The legacy runtime installed its programs in one list, in this
/// order; legacy images keep it, so their plans do not change. A test
/// fails when this program leaves [`ESSENTIAL_BINARIES`].
const LEGACY_AFTER: &str = "dmesg";

/// The programs every image of the runtime needs: legacy images also get
/// those of the legacy runtime, after [`LEGACY_AFTER`].
fn essential(image: &Image) -> Vec<&'static str> {
    let mut names = ESSENTIAL_BINARIES.to_vec();
    if !image.is_rust() {
        let at = names
            .iter()
            .position(|name| *name == LEGACY_AFTER)
            .map_or(names.len(), |after| after + 1);
        names.splice(at..at, LEGACY_BINARIES.iter().copied());
    }
    names
}

/// The programs of the lists: the essential ones fail the build when the
/// host has none, the others are left out with a warning.
///
/// # Errors
///
/// For an essential program the host does not have.
fn programs(
    image: &mut Image,
    essential: &[&str],
    optional: &[&str],
    what: &str,
) -> Result<(), Fatal> {
    for name in essential {
        if image.program(name).is_none() {
            return Err(format!("failed to install essential executable '{name}'"));
        }
    }
    for name in optional {
        if image.program(name).is_none() {
            image
                .plan
                .warn(format!("{what} '{name}' not found, omitting from image"));
        }
    }
    Ok(())
}

/// The LUKS programs and rules, and the configuration.
fn luks_programs(image: &mut Image) -> Result<(), Fatal> {
    let names = luks::binaries(image);
    programs(image, &[], &names, "LUKS component")?;
    rules(image, LUKS_UDEV_RULES, Need::Optional);
    Ok(())
}

/// `libgcc_s`, where glibc needs it.
fn libgcc_s(image: &mut Image) -> Result<(), Fatal> {
    for src in libs::libgcc_s(image.host, image.config.get("zfsbootmenu_skip_gcc_s"))? {
        image.plan.add(Entry::Library { src });
    }
    Ok(())
}

/// The compatibility feature sets of ZFS.
fn compat(image: &mut Image) {
    for dir in COMPAT_DIRS.map(Path::new) {
        if image.host.is_dir(dir) {
            image.plan.add(Entry::Dir {
                dest: dir.to_owned(),
                mode: 0o755,
            });
            image.tree(dir);
        }
    }
}

/// The kernel modules of a dracut image (its `installkernel`).
fn dracut_modules(image: &mut Image) -> Result<(), Fatal> {
    modules(image, ESSENTIAL_MODULES, Need::Required);
    modules(image, OPTIONAL_MODULES, Need::Optional);
    if luks::is_enabled(image) {
        modules(image, LUKS_MODULES, Need::Optional);
        modules(image, LUKS_TOKEN_MODULES, Need::Optional);
    }
    graphics::graphics(image)
}

/// `/etc/hostid` and `vdev_id.conf`, which are the host's own and stay out
/// of releases. With ZFS 2.0 and later the host's `/etc/hostid` is taken,
/// else the ID `hostid` prints, unless it is zero; before ZFS 2.0, whatever
/// `hostid` prints, as dracut images of musl hosts had it.
fn dracut_host(image: &mut Image) {
    if !image.config.is_release() {
        let vdev = Path::new("/etc/zfs/vdev_id.conf");
        if image.host.exists(vdev) {
            image.install(vdev, vdev);
            image.plan.add(Entry::HostOnly {
                dest: vdev.to_owned(),
            });
        }
        let is_new = zfs_version(image.host.runner).is_some_and(|version| is_at_least_2(&version));
        let hostid_file = Path::new("/etc/hostid");
        if is_new && image.host.exists(hostid_file) {
            image.install(hostid_file, hostid_file);
        } else if let Some(id) = hostid(image.host.runner)
            && (!is_new || id != "00000000")
        {
            image.plan.write(hostid_file, 0o644, hostid_bytes(&id));
        }
    }
    image.plan.add(Entry::HostOnly {
        dest: "/etc/hostid".into(),
    });
    if image.config.is_release() {
        image
            .plan
            .write("/etc/cmdline.d/hostonly.conf", 0o644, "rd.hostonly=0\n");
    }
}

/// Whether the ZFS version `version` is 2.0 or later (`sort -V`).
fn is_at_least_2(version: &str) -> bool {
    let major: String = version.chars().take_while(char::is_ascii_digit).collect();
    major.parse::<u32>().is_ok_and(|major| major >= 2)
}

/// The bytes of `/etc/hostid` for the host ID `id` of eight hexadecimal
/// digits: little-endian, as `zgenhostid` writes them on the hosts ZFS
/// runs on.
fn hostid_bytes(id: &str) -> Vec<u8> {
    let value = u32::from_str_radix(id, 16).unwrap_or_default();
    value.to_le_bytes().to_vec()
}

/// The hooks that start the runtime, in dracut's `cmdline` and
/// `pre-mount` stages, and those that make dracut's event loop run once.
fn dracut_hooks(image: &mut Image) {
    let moddir = PathBuf::from(image.config.or("moddir", MODDIR));
    let legacy_hooks = image.module_root().join("pre-init");
    let (cmdline, preinit) = if image.is_rust() {
        (
            moddir.join("zfsbootmenu-rust-cmdline.sh"),
            moddir.join("zfsbootmenu-rust-preinit.sh"),
        )
    } else {
        (
            legacy_hooks.join("zfsbootmenu-parse-commandline.sh"),
            legacy_hooks.join("zfsbootmenu-preinit.sh"),
        )
    };
    let profiling = image
        .config
        .get("dracut_trace_enable")
        .map(|_| image.module_root().join("profiling/profiling-lib.sh"));
    let mut hook = |stage: &str, priority: u8, src: PathBuf| {
        image.plan.add(Entry::Hook {
            stage: stage.to_owned(),
            priority,
            src,
        });
    };
    hook("cmdline", 95, cmdline);
    hook("pre-mount", 90, preinit);
    hook(
        "initqueue/settled",
        99,
        moddir.join("zfsbootmenu-ready-set.sh"),
    );
    hook(
        "initqueue/finished",
        99,
        moddir.join("zfsbootmenu-ready-chk.sh"),
    );
    if let Some(profiling) = profiling {
        hook("cmdline", 0, profiling);
    }
}

/// The plan of a dracut image.
///
/// # Errors
///
/// Why the image cannot be built.
pub fn dracut(image: &mut Image) -> Result<(), Fatal> {
    dracut_modules(image)?;
    core::runtime(image)?;
    rules(image, UDEV_RULES, Need::Required);
    let essential = essential(image);
    programs(image, &essential, OPTIONAL_BINARIES, "optional component")?;
    if luks::is_wanted(image) {
        luks_programs(image)?;
        if luks::luks(image).is_err() {
            image
                .plan
                .warn("LUKS configuration was not fully installed");
        }
        // The dracut crypt module, if included anyway, must not ask for the
        // LUKS volumes before ZFSBootMenu can
        image
            .plan
            .write("/etc/cmdline.d/zfsbootmenu-luks.conf", 0o644, "rd.luks=0\n");
    }
    libgcc_s(image)?;
    compat(image);
    core::core(image);
    core::hooks(image);
    dracut_hooks(image);
    core::docs(image);
    core::osver(image);
    dracut_host(image);
    shell::conf(image);
    shell::profiles(image);
    shell::traceconf(image);
    console(image)?;
    Ok(())
}

/// The console: its fonts (with `setfont` for the legacy runtime, which
/// loads them with it), the keyboard layouts, the line editor, the lock
/// and the screen readers.
fn console(image: &mut Image) -> Result<(), Fatal> {
    if image.is_rust() {
        fonts::fonts(image);
    } else if image.host.which("setfont").is_some() && fonts::has_fonts(image) {
        fonts::fonts(image);
        image.executable("setfont");
    }
    fonts::layouts(image);
    shell::input(image);
    graphics::lock(image)?;
    graphics::screen_readers(image)
}

/// The programs of a mkinitcpio image: with `zfsbootmenu_miser`, only
/// those its busybox lacks.
fn mkinitcpio_programs(image: &mut Image) -> Result<(), Fatal> {
    let miser = image.config.get("zfsbootmenu_miser").is_some_and(|miser| {
        matches!(
            miser.to_ascii_lowercase().as_str(),
            "1" | "y" | "yes" | "on"
        )
    });
    let essential = essential(image);
    if !miser {
        return programs(image, &essential, OPTIONAL_BINARIES, "optional component");
    }
    let busybox = busybox_programs(image.host.runner);
    let lacks = |names: &[&'static str]| -> Vec<&'static str> {
        names
            .iter()
            .copied()
            .filter(|name| !busybox.iter().any(|have| have == name))
            .collect()
    };
    let (essential, optional) = (lacks(&essential), lacks(OPTIONAL_BINARIES));
    for name in essential {
        if image.executable(name).is_none() {
            return Err(format!("failed to install essential executable '{name}'"));
        }
    }
    programs(image, &[], &optional, "optional component")
}

/// What a mkinitcpio image has of the host: the start-up hooks of the
/// legacy runtime in `/lib`, `/etc/filesystems` for `mount`, the
/// compatibility feature sets, the host ID, `vdev_id.conf` and ZFS's
/// module options, and the terminal descriptions.
fn mkinitcpio_files(image: &mut Image) {
    let pre_init = image.module_root().join("pre-init");
    for name in image.host.names(&pre_init) {
        image.install(&pre_init.join(&name), &Path::new("/lib").join(&name));
    }
    // mount(8) finds ZFS by itself
    image.plan.write("/etc/filesystems", 0o644, "zfs\n");
    compat(image);
    for file in [
        "/etc/hostid",
        "/etc/zfs/vdev_id.conf",
        "/etc/modprobe.d/zfs.conf",
    ]
    .map(Path::new)
    {
        if image.host.is_file(file) {
            image.install(file, file);
        }
    }
    shell::terminfo(image);
}

/// The entry point of the legacy runtime, which the run hook starts with
/// Bash; the Rust runtime's run hook starts `zbm` itself.
const ENTRYPOINT: &str = r#"#!/bin/bash
hooks=(
    /lib/zfsbootmenu-parse-commandline.sh
    /lib/zfsbootmenu-preinit.sh
)

for hook in "${hooks[@]}"; do
    [ -r "${hook}" ] && source "${hook}" && continue

    echo "ERROR: failed to load hook "${hook}"; good luck..."
    exec /bin/bash
done
"#;

/// The plan of a mkinitcpio image.
///
/// # Errors
///
/// Why the image cannot be built.
pub fn mkinitcpio(image: &mut Image) -> Result<(), Fatal> {
    core::runtime(image)?;
    graphics::graphics(image)?;
    modules(image, ESSENTIAL_MODULES, Need::Required);
    modules(image, OPTIONAL_MODULES, Need::Optional);
    rules(image, UDEV_RULES, Need::Required);
    mkinitcpio_programs(image)?;
    if luks::is_wanted(image) {
        luks_programs(image)?;
        modules(image, LUKS_MODULES, Need::Optional);
        modules(image, LUKS_TOKEN_MODULES, Need::Optional);
    }
    libgcc_s(image)?;
    core::core(image);
    core::hooks(image);
    core::docs(image);
    core::osver(image);
    mkinitcpio_files(image);
    shell::conf(image);
    shell::profiles(image);
    shell::traceconf(image);
    if !image.is_rust() {
        image
            .plan
            .write("/libexec/zfsbootmenu-initcpio", 0o755, ENTRYPOINT);
    }
    if luks::luks(image).is_err() {
        image
            .plan
            .warn("LUKS configuration was not fully installed");
    }
    console(image)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_ids_are_little_endian_and_versions_compare_by_their_major() {
        assert_eq!(hostid_bytes("0a1b2c3d"), [0x3d, 0x2c, 0x1b, 0x0a]);
        assert!(is_at_least_2("2.2.6-1") && is_at_least_2("10.0"));
        assert!(!is_at_least_2("0.8.6") && !is_at_least_2(""));
    }

    #[test]
    fn the_program_legacy_images_list_fzf_after_is_essential() {
        assert!(ESSENTIAL_BINARIES.contains(&LEGACY_AFTER));
    }
}
