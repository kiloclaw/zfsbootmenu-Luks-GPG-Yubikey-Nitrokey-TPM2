//! Boots a kernel in QEMU and drives a shell on its serial console: the
//! harness end to end, without the build container.
//!
//! Ignored by default; run with
//!
//! ```sh
//! cargo test -p zbm-vmtest --test smoke -- --ignored
//! ```
//!
//! It boots `/boot/vmlinuz` and `/boot/initrd.img` (or the files named by
//! `ZBMTEST_SMOKE_KERNEL` and `ZBMTEST_SMOKE_INITRD`) with `rdinit=/bin/sh`,
//! so the initramfs must have a shell.

use std::path::PathBuf;

use zbm_vmtest::qemu::Machine;
use zbm_vmtest::vm::{Sh, Vm};
use zbm_vmtest::{Settings, Test};

fn file(variable: &str, default: &str) -> PathBuf {
    std::env::var_os(variable).map_or_else(|| PathBuf::from(default), PathBuf::from)
}

#[test]
#[ignore = "boots a kernel in QEMU"]
fn a_shell_on_the_serial_console_runs_commands() {
    let mut settings = Settings::from_env();
    settings.work = std::env::temp_dir().join(format!("zbm-vmtest-smoke-{}", std::process::id()));
    let test = Test::new(&settings, "smoke").unwrap();
    let kernel = file("ZBMTEST_SMOKE_KERNEL", "/boot/vmlinuz");
    let initrd = file("ZBMTEST_SMOKE_INITRD", "/boot/initrd.img");
    let mut machine = Machine::kernel(&kernel, &initrd, "console=ttyS0 rdinit=/bin/sh panic=-1");
    machine.mem = 1024;
    let mut vm = Vm::start(&test, "smoke", &machine).unwrap();

    vm.expect1(r"# ", 600).unwrap();
    // The initramfs may lack stty: the terminal then echoes the commands.
    vm.sendline("stty -echo cols 250; export PS1='zbm# ' PS2=''")
        .unwrap();
    vm.sh("true").unwrap();
    let (rc, out) = vm
        .sh_with("echo hello; uname -s", Sh::default().show())
        .unwrap();
    let (failed, _) = vm.sh("false").unwrap();
    let (multi, lines) = vm.sh("for i in 1 2\ndo echo line$i; done").unwrap();
    vm.sendline("mount -t proc proc /proc").unwrap();
    vm.poweroff().unwrap();

    assert_eq!(rc, 0);
    assert!(out.ends_with("hello\nLinux"), "{out}");
    assert_eq!(failed, 1);
    assert_eq!(multi, 0);
    assert!(lines.ends_with("line1\nline2"), "{lines}");
    let log = std::fs::read_to_string(vm.serial_log()).unwrap();
    assert!(
        log.contains("Linux version"),
        "the serial log holds the boot messages"
    );
    let outcome = test.finish();
    assert_eq!(outcome.failures, Vec::<String>::new());
    drop(vm);
    std::fs::remove_dir_all(&settings.work).ok();
}
