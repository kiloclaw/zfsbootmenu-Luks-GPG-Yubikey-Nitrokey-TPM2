//! The kernel's file systems booting needs: `securityfs`, where the kernel
//! says whether it is locked down, and `efivarfs`.

use std::path::Path;

use zbm_bootenv::is_mount_point;
use zbm_platform::Context;
use zbm_tools::mount::Mount;

/// Whether the machine booted with UEFI: `/sys/firmware/efi` exists.
fn is_efi_system(context: &Context<'_>) -> bool {
    context.files().is_dir(Path::new("/sys/firmware/efi"))
}

/// Mounts `securityfs` on `/sys/kernel/security`, where the kernel says
/// whether it is locked down (ROADMAP.md, SE10), unless it is there: the
/// Rust runtime is process 1, and no init mounted it before. Does nothing
/// on a kernel without it.
pub fn mount_securityfs(context: &Context<'_>) {
    let location = "/sys/kernel/security";
    if is_mount_point(context.files(), location) || !context.files().is_dir(Path::new(location)) {
        return;
    }
    zbm_trace::debug!("mounting '{location}'"; %location);
    if let Err(code) = Mount::new(context.runner(), context.files()).securityfs(location) {
        zbm_trace::debug!("securityfs did not mount ({code})"; %code);
    }
}

/// Mounts or remounts `efivarfs` read-only (`ro`) or read-write (`rw`) on
/// `/sys/firmware/efi/efivars`. Does nothing without UEFI.
pub fn mount_efivarfs(context: &Context<'_>, mode: &str) {
    let location = "/sys/firmware/efi/efivars";
    if !is_efi_system(context) {
        zbm_trace::debug!("efivarfs unsupported");
        return;
    }
    let mount = Mount::new(context.runner(), context.files());
    let mounted = is_mount_point(context.files(), location);
    let options = if mounted {
        zbm_trace::debug!("remounting '{location}' '{mode}'"; %location, %mode);
        format!("remount,{mode}")
    } else {
        zbm_trace::debug!("mounting '{location}' '{mode}'"; %location, %mode);
        mode.to_owned()
    };
    let _ = mount.efivarfs(location, &options);
}
