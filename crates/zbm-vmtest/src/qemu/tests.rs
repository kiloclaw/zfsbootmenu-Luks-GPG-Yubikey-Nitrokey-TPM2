use pretty_assertions::assert_eq;

use super::*;

fn sockets(tpm: bool) -> Sockets {
    Sockets {
        serial: "/tmp/s.sock".into(),
        monitor: "/tmp/m.sock".into(),
        tpm: tpm.then(|| "/tmp/t.sock".into()),
    }
}

#[test]
fn a_direct_boot_has_the_kernel_the_disks_and_the_serial_console_on_a_socket() {
    let image = Image {
        kernel: "/z/vmlinuz".into(),
        initrd: "/z/initrd".into(),
        efi: "/z/efi".into(),
        debug: "/z/debug".into(),
    };
    let machine = Machine::boot(&image, "console=ttyS0 zbm.timeout=3").with_disks([
        Disk::new("/d/disk0.img", "d0"),
        Disk::new("/d/payload.img", PAYLOAD),
    ]);

    let args = args(&machine, "kvm", &sockets(false)).join(" ");

    assert_eq!(
        args,
        "-accel kvm -cpu max -m 4096 -smp 2 -display none -no-reboot \
         -chardev socket,id=serial0,path=/tmp/s.sock,server=on,wait=on -serial chardev:serial0 \
         -monitor unix:/tmp/m.sock,server,nowait \
         -kernel /z/vmlinuz -initrd /z/initrd -append console=ttyS0 zbm.timeout=3 \
         -drive file=/d/disk0.img,if=none,id=drv-d0,format=raw,discard=unmap \
         -device virtio-blk-pci,drive=drv-d0,serial=d0,id=dev-d0 \
         -drive file=/d/payload.img,if=none,id=drv-payload,format=raw,readonly=on,file.locking=off \
         -device virtio-blk-pci,drive=drv-payload,serial=payload,id=dev-payload \
         -device qemu-xhci,id=xhci"
    );
}

#[test]
fn a_uefi_boot_has_the_firmware_an_nvme_disk_usb_sticks_and_a_tpm() {
    let machine = Machine {
        nvme: Some(("/d/nvme.img".into(), 4096)),
        usb: vec![("/d/usb.img".into(), "usb0".into())],
        ovmf_vars: Some("/d/OVMF_VARS.fd".into()),
        net: Some("hostfwd=tcp::2222-:22".into()),
        extra: vec!["-snapshot".into()],
        ..Machine::default()
    };

    let args = args(&machine, "tcg", &sockets(true));

    let after = |flag: &str, value: &str| {
        args.windows(2)
            .any(|pair| pair[0] == flag && pair[1] == value)
    };
    assert!(after("-machine", "q35"));
    assert!(after(
        "-drive",
        "if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE_4M.fd"
    ));
    assert!(after("-drive", "if=pflash,format=raw,file=/d/OVMF_VARS.fd"));
    assert!(after(
        "-device",
        "nvme-ns,drive=nv0,bus=nvme0,nsid=1,logical_block_size=4096,physical_block_size=4096"
    ));
    assert!(after(
        "-device",
        "usb-storage,bus=xhci.0,drive=usb0,id=dev-usb0"
    ));
    assert!(after("-netdev", "user,id=n0,hostfwd=tcp::2222-:22"));
    assert!(after("-tpmdev", "emulator,id=tpm0,chardev=chrtpm"));
    assert!(!args.contains(&"-kernel".to_owned()));
    let extra = args.iter().position(|arg| arg == "-snapshot").unwrap();
    let tpm = args.iter().position(|arg| arg == "-tpmdev").unwrap();
    assert!(extra < tpm);
}

#[test]
fn the_monitor_reply_leaves_out_prompts_and_echo() {
    let answer = "QEMU 10.0 monitor\r\n(qemu) drive_add 0 file=x\r\nOK\r\n(qemu) ";

    assert_eq!(monitor_reply(answer), "QEMU 10.0 monitor\nOK");
}

#[test]
fn the_echo_of_a_command_is_left_out_of_its_reply() {
    // As QEMU 10 redraws its input line for each character of `sendkey esc 50`.
    let command = "sendkey esc 50";
    let echo: String = (1..=command.len()).map(|n| &command[..n]).collect();
    assert_eq!(without_echo(&echo, command), "");
    assert_eq!(without_echo(command, command), "");
    let refused = format!("{echo}\ninvalid parameter: escape");
    assert_eq!(without_echo(&refused, command), "invalid parameter: escape");
    assert_eq!(
        without_echo("QEMU 10.0 monitor\nOK", command),
        "QEMU 10.0 monitor\nOK"
    );
}

#[test]
fn the_monitor_is_asked_over_its_socket() {
    use std::os::unix::net::UnixListener;
    let path = std::env::temp_dir().join(format!("zbm-vmtest-mon-{}.sock", std::process::id()));
    std::fs::remove_file(&path).ok();
    let listener = UnixListener::bind(&path).unwrap();
    #[allow(
        clippy::disallowed_methods,
        reason = "a developer tool, not the runtime of ADR 0001"
    )]
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream.write_all(b"QEMU monitor\r\n(qemu) ").unwrap();
        let mut command = [0_u8; 64];
        let read = stream.read(&mut command).unwrap();
        let echo = format!(
            "{}\r\nOK\r\n(qemu) ",
            String::from_utf8_lossy(&command[..read]).trim()
        );
        stream.write_all(echo.as_bytes()).unwrap();
        thread::sleep(Duration::from_millis(900));
    });

    let reply = monitor(&path, "device_add usb-storage").unwrap();

    assert_eq!(reply, "OK");
    server.join().unwrap();
    std::fs::remove_file(&path).ok();
    assert!(monitor(&path, "info").is_err());
}
