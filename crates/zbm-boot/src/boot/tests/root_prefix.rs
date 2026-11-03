//! The root prefix of a boot, from a property a foreign pool may set.

use super::*;
use zbm_tools::console::ConsoleInteraction;

#[test]
fn a_root_prefix_cannot_add_kernel_arguments() {
    // Bug 27: the legacy function put the property before the name as it
    // was, so this prefix added `init=/bin/sh`.
    let mut machine = boot_machine();
    machine.answers.insert(
        0,
        (
            0,
            "zfs get -H -o value org.zfsbootmenu:rootprefix *",
            "root=zfs: init=/bin/sh \\n",
        ),
    );
    let selected = Kernel::new(FS, "/boot/vmlinuz-6.1.10", "/boot/initramfs-6.1.10.img");
    let outcome = machine.rust(|system, state, shells, hooks| {
        let settings = BootSettings {
            hooks: hooks.to_path_buf(),
            ..BootSettings::default()
        };
        let interaction = ConsoleInteraction::new(*system);
        let context = Context::new(*system);
        kexec_kernel(&context, &interaction, state, &settings, &selected, shells);
        String::new()
    });
    let load = outcome
        .calls
        .lines()
        .find(|call| call.starts_with("kexec -a -l"))
        .unwrap_or_default();
    assert!(
        load.contains(&format!("--command-line=root=zfs:{FS} ")),
        "{load}"
    );
    assert!(!load.contains("init=/bin/sh"), "{load}");
    assert!(outcome.log.contains("ignoring org.zfsbootmenu:rootprefix"));
}
