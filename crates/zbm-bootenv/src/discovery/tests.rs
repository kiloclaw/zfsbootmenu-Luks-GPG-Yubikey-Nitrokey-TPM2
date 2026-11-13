//! The kernel search of a dataset named like an entry of its parent's
//! state, `tank/a/mnt`, while `tank/a` is mounted (SB19): the links of
//! `tank/a`'s root are modelled below its mount point.

use std::path::Path;

use zbm_config::parent::CommandLineSource;
use zbm_platform::Context;
use zbm_state::State;
use zbm_testkit::fakes::{FakeSystem, FileOperation};

use super::*;
use crate::commandline::be_cmdline;
use crate::testing::name;

/// The directory of every filesystem's state.
const E: &str = "/zfsbootmenu/environments";
/// Where `tank/a/mnt` is mounted, in a directory of its own.
const CHILD: &str = "/zfsbootmenu/environments/tank/a/+mnt/mnt";

/// A machine where `tank/a/mnt` has the kernel `/boot/vmlinuz-1` with
/// its initramfs and a command line file, and no `org.zfsbootmenu:kernel`.
fn machine() -> FakeSystem {
    let fake = FakeSystem::new();
    fake.files
        .dir("/zfsbootmenu")
        .dir("/etc")
        .file(format!("{CHILD}/boot/vmlinuz-1"), "")
        .file(format!("{CHILD}/boot/initramfs-1.img"), "")
        .file(format!("{CHILD}/boot/vmlinuz-1.kcl"), "quiet\n");
    fake.runner.respond(
        "zfs get -H -o value org.zfsbootmenu:kernel tank/a/mnt",
        0,
        "-\n",
    );
    fake
}

/// Searches the kernels of `fs` on `fake`.
fn search(fake: &FakeSystem, fs: &str) -> Result<(), BootEnvError> {
    let state = State::new(&fake.files, "/zfsbootmenu");
    find_kernels(&Context::new(fake.system()), &state, &name(fs))
}

/// The paths below `prefix` that `fake`'s code wrote, created or removed.
fn changed_below(fake: &FakeSystem, prefix: &str) -> Vec<FileOperation> {
    fake.files
        .operations()
        .into_iter()
        .filter(|operation| {
            let path = match operation {
                FileOperation::Write(path, _)
                | FileOperation::Append(path, _)
                | FileOperation::CreateDir(path)
                | FileOperation::Remove(path)
                | FileOperation::SetMode(path, _)
                | FileOperation::Rename(path, _) => path,
            };
            path.starts_with(prefix)
        })
        .collect()
}

#[test]
fn a_child_named_mnt_is_not_mounted_through_its_parent_s_link() {
    let fake = machine();
    fake.files.symlink(format!("{E}/tank/a/mnt/mnt"), "/etc");

    search(&fake, "tank/a/mnt").unwrap();

    let mount = format!("mount -o zfsutil,ro -t zfs tank/a/mnt {CHILD}");
    let lines = fake.runner.lines();
    assert!(lines.contains(&mount), "{lines:#?}");
    assert_eq!(changed_below(&fake, "/etc"), []);
}

#[test]
fn a_child_s_kernel_list_is_not_written_through_its_parent_s_link() {
    let fake = machine();
    fake.files
        .file("/etc/passwd", "root:x:0:0::/root:/bin/sh\n")
        .symlink(format!("{E}/tank/a/mnt/kernels"), "/etc/passwd");

    search(&fake, "tank/a/mnt").unwrap();

    assert_eq!(
        fake.files.text("/etc/passwd").as_deref(),
        Some("root:x:0:0::/root:/bin/sh\n")
    );
    assert_eq!(changed_below(&fake, "/etc"), []);
    let list = fake.files.text(format!("{E}/tank/a/+mnt/kernels"));
    assert_eq!(
        list.as_deref(),
        Some("tank/a/mnt\t/boot/vmlinuz-1\t/boot/initramfs-1.img\n")
    );
}

#[test]
fn a_child_s_command_line_cache_is_not_cleared_through_its_parent_s_link() {
    let fake = machine();
    fake.files
        .file("/etc/x.kcl", "kept\n")
        .symlink(format!("{E}/tank/a/mnt/kcl"), "/etc");

    search(&fake, "tank/a/mnt").unwrap();

    assert_eq!(fake.files.text("/etc/x.kcl").as_deref(), Some("kept\n"));
    assert_eq!(changed_below(&fake, "/etc"), []);
    let cached = fake
        .files
        .text(format!("{E}/tank/a/+mnt/kcl/boot:vmlinuz-1.kcl"));
    assert_eq!(cached.as_deref(), Some("quiet\n"));
}

/// No property sets a command line.
struct NoProperties;

impl CommandLineSource for NoProperties {
    fn commandline(&self, _: &str) -> Option<String> {
        Some("-".to_owned())
    }

    fn inherited_source(&self, _: &str) -> Option<String> {
        Some(String::new())
    }
}

#[test]
fn a_child_s_kernel_and_command_line_are_its_own_not_its_parent_s() {
    // Files in `tank/a`'s root, where the child's state was at HEAD.
    let fake = machine();
    fake.files
        .file(
            format!("{E}/tank/a/mnt/kernels"),
            "q\t/boot/vmlinuz-9\t/boot/initramfs-9.img\n",
        )
        .file(
            format!("{E}/tank/a/mnt/kcl/boot:vmlinuz-1.kcl"),
            "init=/bin/sh\n",
        );
    let state = State::new(&fake.files, "/zfsbootmenu");
    let context = Context::new(fake.system());

    assert_eq!(select_kernel(&context, &state, &name("tank/a/mnt")), None);
    let line = be_cmdline(
        &fake.files,
        &NoProperties,
        &state,
        "tank/a/mnt",
        "/boot/vmlinuz-1",
        false,
        &mut Vec::new(),
    );
    assert!(!line.to_string().contains("init=/bin/sh"), "{line}");

    // Once searched, the child's own.
    search(&fake, "tank/a/mnt").unwrap();
    let kernel = select_kernel(&context, &state, &name("tank/a/mnt")).unwrap();
    assert_eq!(kernel.kernel, "/boot/vmlinuz-1");
}

#[test]
fn an_ordinary_environment_s_kernels_are_found_where_they_were() {
    let fake = FakeSystem::new();
    let mnt = format!("{E}/tank/ROOT/be/mnt");
    fake.files
        .dir("/zfsbootmenu")
        .file(format!("{mnt}/boot/vmlinuz-1"), "")
        .file(format!("{mnt}/boot/initramfs-1.img"), "")
        .file(format!("{mnt}/boot/vmlinuz-1.kcl"), "quiet\n");
    fake.runner.respond(
        "zfs get -H -o value org.zfsbootmenu:kernel tank/ROOT/be",
        0,
        "-\n",
    );

    search(&fake, "tank/ROOT/be").unwrap();

    let mount = format!("mount -o zfsutil,ro -t zfs tank/ROOT/be {mnt}");
    assert!(fake.runner.lines().contains(&mount));
    assert_eq!(
        fake.files
            .text(format!("{E}/tank/ROOT/be/kernels"))
            .as_deref(),
        Some("tank/ROOT/be\t/boot/vmlinuz-1\t/boot/initramfs-1.img\n")
    );
    assert_eq!(
        fake.files
            .text(format!("{E}/tank/ROOT/be/kcl/boot:vmlinuz-1.kcl"))
            .as_deref(),
        Some("quiet\n")
    );
    assert!(Path::new(&mnt).starts_with(E));
}
