use pretty_assertions::assert_eq;

use super::*;

fn event(kind: u16, code: u16, value: i32) -> Vec<u8> {
    let mut bytes = vec![0_u8; 16];
    bytes.extend_from_slice(&kind.to_ne_bytes());
    bytes.extend_from_slice(&code.to_ne_bytes());
    bytes.extend_from_slice(&value.to_ne_bytes());
    bytes
}

#[test]
fn events_are_read_from_whole_records() {
    let mut bytes = event(EV_KEY, 30, 1);
    bytes.extend(event(0, 0, 0));
    bytes.extend([1, 2, 3]);

    let events = parse_events(&bytes);

    assert_eq!(
        events,
        [
            InputEvent {
                kind: EV_KEY,
                code: 30,
                value: 1
            },
            InputEvent {
                kind: 0,
                code: 0,
                value: 0
            },
        ]
    );
}

/// The `capabilities/key` of an AT keyboard (QEMU's PS/2 keyboard) and
/// of a power button.
const AT_KEYBOARD: &str = "402000000 3803078f800d001 feffffdfffefffff fffffffffffffffe";
const POWER_BUTTON: &str = "10000000000000 0";

#[test]
fn keyboards_are_told_from_other_devices_by_their_keys() {
    assert!(has_bit(AT_KEYBOARD, 30));
    assert!(has_bit(AT_KEYBOARD, 64 + 1));
    assert!(!has_bit(AT_KEYBOARD, 0));
    assert!(!has_bit(AT_KEYBOARD, 64 * 9));
    assert!(!has_bit("zz", 1));
    assert!(is_keyboard(AT_KEYBOARD));
    assert!(!is_keyboard(POWER_BUTTON));
    assert!(!is_keyboard(""));
}

#[test]
fn keyboards_are_found_in_sysfs() {
    let root = std::env::temp_dir().join(format!("zbm-display-input-{}", std::process::id()));
    for (name, keys) in [
        ("event0", POWER_BUTTON),
        ("event3", AT_KEYBOARD),
        ("mouse0", AT_KEYBOARD),
    ] {
        let caps = root
            .join("sys/class/input")
            .join(name)
            .join("device/capabilities");
        std::fs::create_dir_all(&caps).unwrap();
        std::fs::write(caps.join("key"), format!("{keys}\n")).unwrap();
    }

    let found = keyboards(&root.join("sys"), Path::new("/dev"));
    let missing = Keyboards::open(&root.join("nothing"), Path::new("/dev"));

    assert_eq!(found, [PathBuf::from("/dev/input/event3")]);
    assert_eq!(missing.unwrap_err().kind(), io::ErrorKind::NotFound);
    let unopenable = Keyboards::open(&root.join("sys"), &root.join("no-dev"));
    assert!(unopenable.unwrap_err().to_string().contains("event3"));
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn presses_are_read_from_the_devices_that_have_events() {
    let root = std::env::temp_dir().join(format!("zbm-display-keys-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let mut bytes = event(EV_KEY, 42, 1);
    for (kind, code, value) in [(EV_KEY, 30, 1), (0, 0, 0), (EV_KEY, 30, 0), (EV_KEY, 48, 1)] {
        bytes.extend(event(kind, code, value));
    }
    std::fs::write(root.join("events"), bytes).unwrap();
    let mut keyboards = Keyboards::from_files(vec![File::open(root.join("events")).unwrap()]);

    let first = keyboards.next(Duration::ZERO).unwrap().unwrap();
    let second = keyboards.next(Duration::ZERO).unwrap().unwrap();

    assert_eq!(first.key, crate::keys::Key::Char('A'));
    assert_eq!(second.key, crate::keys::Key::Char('B'));
    assert_eq!(keyboards.next(Duration::ZERO).unwrap(), None);
    std::fs::remove_dir_all(&root).unwrap();
}

fn socket_keyboards() -> (Keyboards, std::os::unix::net::UnixStream) {
    let (device, typing) = std::os::unix::net::UnixStream::pair().unwrap();
    let file = File::from(std::os::fd::OwnedFd::from(device));
    (Keyboards::from_files(vec![file]), typing)
}

#[test]
fn a_keyboard_that_goes_away_is_dropped() {
    let (mut keyboards, typing) = socket_keyboards();
    assert_eq!(keyboards.count(), 1);
    drop(typing);

    assert_eq!(keyboards.read_device(0).unwrap(), []);
    assert_eq!(keyboards.count(), 0);
    assert_eq!(keyboards.read_device(3).unwrap(), []);
    assert_eq!(keyboards.next(Duration::ZERO).unwrap(), None);
    assert_eq!(keyboards.rescan(), 0);
}

#[test]
fn caps_lock_lights_the_keyboards() {
    use std::io::Write as _;
    let (mut keyboards, mut typing) = socket_keyboards();

    typing.write_all(&event(EV_KEY, 58, 1)).unwrap();
    assert_eq!(keyboards.read_device(0).unwrap(), []);

    let mut lit = [0_u8; 4 * EVENT_SIZE];
    typing.read_exact(&mut lit).unwrap();
    let mut both = led_records(LED_CAPSL, true);
    both.extend(led_records(LED_NUML, false));
    assert_eq!(lit.to_vec(), both);
    assert_eq!(
        parse_events(&lit[..2 * EVENT_SIZE]),
        [
            InputEvent {
                kind: EV_LED,
                code: LED_CAPSL,
                value: 1
            },
            BLANK_EVENT
        ]
    );
    typing.write_all(&event(EV_KEY, 30, 1)).unwrap();
    let presses = keyboards.read_device(0).unwrap();
    assert_eq!(presses[0].key, crate::keys::Key::Char('A'));
}

#[test]
fn events_lost_are_skipped_and_the_keys_resynchronised() {
    use std::io::Write as _;
    let (mut keyboards, mut typing) = socket_keyboards();
    let mut records = event(EV_KEY, 42, 1);
    // The release of Shift is lost with the events after it, up to the
    // end of the group; the keys down are then read from the device.
    for (kind, code, value) in [
        (EV_SYN, SYN_DROPPED, 0),
        (EV_KEY, 30, 1),
        (EV_SYN, SYN_REPORT, 0),
        (EV_KEY, 48, 1),
    ] {
        records.extend(event(kind, code, value));
    }
    typing.write_all(&records).unwrap();

    let presses = keyboards.read_device(0).unwrap();

    assert_eq!(presses.len(), 1);
    assert_eq!(presses[0].key, crate::keys::Key::Char('b'));
}

#[test]
fn keyboards_that_come_later_are_found() {
    let root = std::env::temp_dir().join(format!("zbm-display-hotplug-{}", std::process::id()));
    let add = |name: &str| {
        let caps = root
            .join("sys/class/input")
            .join(name)
            .join("device/capabilities");
        std::fs::create_dir_all(&caps).unwrap();
        std::fs::write(caps.join("key"), AT_KEYBOARD).unwrap();
        std::fs::create_dir_all(root.join("dev/input")).unwrap();
        std::fs::write(root.join("dev/input").join(name), b"").unwrap();
    };
    // Without a keyboard yet, the keyboards are found when they come.
    let mut keyboards = Keyboards::find(&root.join("sys"), &root.join("dev"));
    assert_eq!(keyboards.count(), 0);
    add("event1");
    assert_eq!(keyboards.rescan(), 1);

    assert_eq!(keyboards.rescan(), 0);
    add("event2");
    assert_eq!(keyboards.rescan(), 1);
    assert_eq!(keyboards.count(), 2);
    assert_eq!(keyboards.fds().len(), 2);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn num_lock_starts_as_the_keyboards_show_it() {
    let root = std::env::temp_dir().join(format!("zbm-display-numlock-{}", std::process::id()));
    let device = root.join("sys/class/input/event4/device");
    std::fs::create_dir_all(device.join("capabilities")).unwrap();
    std::fs::write(device.join("capabilities/key"), AT_KEYBOARD).unwrap();
    std::fs::create_dir_all(device.join("input4::numlock")).unwrap();
    std::fs::create_dir_all(device.join("input4::capslock")).unwrap();
    std::fs::write(device.join("input4::capslock/brightness"), "1\n").unwrap();
    std::fs::create_dir_all(root.join("dev/input")).unwrap();
    std::fs::write(root.join("dev/input/event4"), b"").unwrap();
    let (sys, dev) = (root.join("sys"), root.join("dev"));

    std::fs::write(device.join("input4::numlock/brightness"), "0\n").unwrap();
    assert!(!Keyboards::find(&sys, &dev).decoder.is_num_lock());
    std::fs::write(device.join("input4::numlock/brightness"), "1\n").unwrap();
    let mut keyboards = Keyboards::find(&sys, &dev);
    assert!(keyboards.decoder.is_num_lock());
    // The keypad types digits, and the lights stay as they are.
    let (device, mut typing) = std::os::unix::net::UnixStream::pair().unwrap();
    keyboards.devices[0].file = File::from(std::os::fd::OwnedFd::from(device));
    typing.write_all(&event(EV_KEY, 79, 1)).unwrap();
    let typed = keyboards.read_device(0).unwrap();
    assert_eq!(typed[0].key, crate::keys::Key::Char('1'));
    assert_eq!(keyboards.lights, Some((false, true)));
    std::fs::remove_dir_all(&root).unwrap();
}
