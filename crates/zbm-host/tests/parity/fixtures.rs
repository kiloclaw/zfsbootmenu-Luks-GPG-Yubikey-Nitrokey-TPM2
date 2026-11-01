//! The configurations and fake programs that several parity tests share.

use crate::sandbox::Sandbox;

/// A configuration that builds an EFI bundle and versioned components, with
/// a boot partition and hooks.
pub(crate) const BASIC: &str = "Global:
  ManageImages: true
  BootMountPoint: {root}/esp
  DracutConfDir: {root}/dracut.conf.d
  PreHooksDir: {root}/pre.d
  PostHooksDir: {root}/post.d
Components:
  ImageDir: {root}/esp/EFI/zbm
  Versions: 3
  Enabled: true
EFI:
  ImageDir: {root}/esp/EFI/zbm
  Versions: false
  Enabled: true
  Stub: /usr/lib/systemd/boot/efi/linuxx64.efi.stub
Kernel:
  CommandLine: ro quiet loglevel=0
";

/// A configuration that builds versioned components only.
pub(crate) const COMPONENTS: &str = "Global:
  ManageImages: true
  DracutConfDir: {root}/dracut.conf.d
Components:
  ImageDir: {root}/esp/EFI/zbm
  Versions: 3
  Enabled: true
";

/// Writes the images that a run without versions turns into backups.
pub(crate) fn setup_backups(sandbox: &Sandbox) {
    std::fs::create_dir_all(sandbox.path().join("esp/EFI/zbm")).unwrap();
    for name in ["vmlinuz.EFI", "vmlinuz-bootmenu", "initramfs-bootmenu.img"] {
        sandbox.dir.write(format!("esp/EFI/zbm/{name}"), name);
    }
}

/// A hook that sends SIGTERM to the generator, which may be its parent or the
/// parent of the shell that runs it.
pub(crate) const SIGNAL_HOOK: &str = r#"#!/bin/sh
p=$PPID
[ "$(cat /proc/$p/comm)" = sh ] && p=$(cut -d' ' -f4 /proc/$p/stat)
kill -TERM "$p"
"#;
