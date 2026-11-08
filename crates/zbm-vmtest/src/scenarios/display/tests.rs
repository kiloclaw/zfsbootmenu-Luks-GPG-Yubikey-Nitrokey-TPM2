use pretty_assertions::assert_eq;

use super::*;
use crate::testing::{cleanup, settings};

#[test]
fn screens_are_read_from_the_mirror() {
    let settings = settings("runtime-console-mirror");
    let test = Test::new(&settings, "runtime-rust-console").unwrap();
    let path = mirror_path(&test, "virtio");
    fs::write(&path, "\x0cone\r\n\x0ctwo 42\r\n").unwrap();

    assert_eq!(
        wait_screen(&path, r"two \d+", 0).unwrap().as_deref(),
        Some("two 42\n")
    );
    assert_eq!(wait_screen(&path, "three", 0).unwrap(), None);
    assert_eq!(
        wait_screen(&test.dir().join("none"), "one", 0).unwrap(),
        None
    );
    assert!(wait_screen(&path, "(", 0).is_err());
    assert_eq!(last_drawn(&path), "two 42\n");
    assert_eq!(last_screen(&path), "two 42");
    fs::write(&path, "\x0ca\r\n\r\nb\r\n\x0c\n").unwrap();
    assert_eq!(last_screen(&path), "a | b");
    assert_eq!(last_drawn(&path), "a\n\nb\n");
    assert_eq!(last_drawn(&test.dir().join("none")), "");
    assert_eq!(last_screen(&test.dir().join("none")), "(nothing)");
    cleanup(&settings);
}

#[test]
fn the_machine_has_the_display_and_the_mirror_s_serial_port() {
    let virtio = machine_extra("virtio", Path::new("/w/m.txt"));
    let simpledrm = machine_extra("simpledrm", Path::new("/w/m.txt"));

    assert_eq!(
        virtio,
        [
            "-vga",
            "none",
            "-device",
            "virtio-vga",
            "-chardev",
            "file,id=mirror,path=/w/m.txt",
            "-serial",
            "chardev:mirror"
        ]
    );
    assert_eq!(simpledrm, virtio[4..]);
    let spec = spec();
    assert_eq!(spec.runtime, Some(ZbmRuntime::Rust));
    assert!(spec.conf.contains("virtio_gpu evdev"), "{}", spec.conf);
    // The mirror the test reads is in test builds only.
    assert_eq!(spec.features.as_deref(), Some("zbm/console-mirror"));
}

#[test]
fn the_names_and_signs_of_the_menu_s_fields_are_typed_too() {
    assert_eq!(
        key_names("a_b@c=d").unwrap(),
        ["a", "shift-minus", "b", "shift-2", "c", "equal", "d"]
    );
    assert_eq!(key_names("'"), None);
}

#[test]
fn the_display_keys_go_through_the_monitor_and_sendkey_through_the_machine() {
    let settings = settings("display-keys");
    let test = Test::new(&settings, "display-keys").unwrap();
    let (vm, guest) = crate::testing::scripted(&test, Vec::new());
    let monitor = Monitor {
        dump: |_, _| Ok(()),
        key: |_, name| {
            if name == "ret" {
                Ok(())
            } else {
                Err(Error::Setup(name.to_owned()))
            }
        },
    };

    assert!(type_text(&vm, &monitor, "\n", Duration::ZERO).is_ok());
    assert!(type_text(&vm, &monitor, "x", Duration::ZERO).is_err());
    assert!(type_text(&vm, &monitor, "~", Duration::ZERO).is_err());
    assert!(sendkey(&vm, "ret").is_err());
    drop(vm);
    assert_eq!(guest.finish(), Vec::<String>::new());
    cleanup(&settings);
}
