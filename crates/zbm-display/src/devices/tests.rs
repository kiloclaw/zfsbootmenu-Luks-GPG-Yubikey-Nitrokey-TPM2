use pretty_assertions::assert_eq;

use super::*;

#[test]
fn a_name_is_of_a_kind_when_it_is_its_prefix_and_a_number() {
    let names = [
        (Kind::Card, "dri/card0", true),
        (Kind::Card, "dri/card12", true),
        (Kind::Card, "dri/renderD128", false),
        (Kind::Card, "card0", false),
        (Kind::Terminal, "tty1", true),
        (Kind::Terminal, "tty0", false),
        (Kind::Terminal, "tty01", false),
        (Kind::Terminal, "ttyS0", false),
        (Kind::Terminal, "tty", false),
        (Kind::Text, "vcsa3", true),
        (Kind::Text, "vcsa0", false),
        (Kind::Text, "vcs3", false),
        (Kind::Input, "input/event7", true),
        (Kind::Input, "input/mice", false),
        (Kind::Input, "input/by-id/event1", false),
        (Kind::Input, "input/../sda", false),
        (Kind::Serial, "ttyS0", true),
        (Kind::Serial, "ttyUSB2", true),
        (Kind::Serial, "hvc0", true),
        (Kind::Serial, "tty1", false),
        (Kind::Serial, "ttyS1/x", false),
    ];
    for (kind, name, is) in names {
        assert_eq!(kind.is_name(name), is, "{kind} {name}");
    }
}

#[test]
fn a_device_is_of_a_kind_by_its_numbers() {
    let devices = [
        (Kind::Card, 226, 0, true),
        (Kind::Card, 226, 128, false),
        (Kind::Card, 8, 0, false),
        (Kind::Terminal, 4, 1, true),
        (Kind::Terminal, 4, 0, false),
        (Kind::Terminal, 4, 64, false),
        (Kind::Text, 7, 129, true),
        (Kind::Text, 7, 128, false),
        (Kind::Text, 7, 1, false),
        (Kind::Input, 13, 64, true),
        (Kind::Input, 13, 300, true),
        (Kind::Input, 13, 63, false),
        (Kind::Input, 13, 100, false),
        (Kind::Serial, 4, 64, true),
        (Kind::Serial, 4, 63, false),
        (Kind::Serial, 188, 0, true),
        (Kind::Serial, 1, 3, false),
    ];
    for (kind, major, minor, is) in devices {
        assert_eq!(kind.is_device(major, minor), is, "{kind} {major}:{minor}");
    }
    // No device is of two kinds.
    for major in [4, 7, 13, 166, 188, 204, 226, 229] {
        for minor in 0..1024 {
            let kinds = KINDS
                .iter()
                .filter(|kind| kind.is_device(major, minor))
                .count();
            assert!(kinds <= 1, "{major}:{minor}");
        }
    }
}

#[test]
fn each_kind_opens_as_it_is_used_and_says_the_path_when_it_cannot() {
    let null = Path::new("/dev/null");
    for kind in KINDS {
        assert!(Direct.open(kind, null).is_ok(), "{kind}");
    }
    let error = direct()
        .open(Kind::Input, Path::new("/nonexistent/event0"))
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    assert!(error.to_string().contains("/nonexistent/event0"), "{error}");
    assert_eq!(Kind::Text.to_string(), "text");
    let kinds: Vec<String> = KINDS.iter().map(ToString::to_string).collect();
    assert_eq!(kinds, ["card", "terminal", "text", "input", "serial"]);
}
