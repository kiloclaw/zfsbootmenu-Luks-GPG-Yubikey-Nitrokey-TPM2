//! The layout each key was typed on, which goes to the menu with it: a
//! keyboard's own or the shared one, as it was when the key was read.

use pretty_assertions::assert_eq;

use super::*;

#[test]
fn keys_go_to_the_menu_with_the_layout_of_the_keyboard_that_typed_them() {
    use rustix::fs::{CWD, FileType, Mode, mknodat};
    use zbm_display::layout::{Layout, Source, compile};

    // Two keyboards on pipes, the second a German one of a layout of its own.
    let root = std::env::temp_dir().join(format!("zbm-console-typed-{}", std::process::id()));
    let (sys, dev) = (root.join("sys/class/input"), root.join("dev/input"));
    std::fs::create_dir_all(sys.join("event1/device/id")).unwrap();
    std::fs::write(sys.join("event1/device/id/vendor"), "046d\n").unwrap();
    std::fs::write(sys.join("event1/device/id/product"), "c31c\n").unwrap();
    std::fs::create_dir_all(&dev).unwrap();
    for name in ["event0", "event1"] {
        let keys = sys.join(name).join("device/capabilities");
        std::fs::create_dir_all(&keys).unwrap();
        let at_keyboard = "402000000 3803078f800d001 feffffdfffefffff fffffffffffffffe";
        std::fs::write(keys.join("key"), at_keyboard).unwrap();
        let mode = Mode::RUSR | Mode::WUSR;
        mknodat(CWD, dev.join(name), FileType::Fifo, mode, 0).unwrap();
    }
    let mut keyboards = Keyboards::find(&root.join("sys"), &root.join("dev"));
    let xkb = std::path::Path::new(compile::XKB);
    let keymap = compile::keymap(xkb, "de").expect("the XKB data (xkb-data)");
    let id = zbm_display::input::DeviceId::parse("046d:c31c").unwrap();
    let rules = None;
    let own = Source {
        name: "de".to_owned(),
        keymap: keymap.clone().into_bytes(),
        rules,
    };
    keyboards.set_device_layouts(vec![(id, own)]);
    let (master, _program) = UnixStream::pair().unwrap();
    let mut host = RealHost::new(master, keyboards, control()).with_layouts(Box::new(
        move |keyboards: &mut Keyboards, _: Change<'_>| {
            keyboards.set_layout(Layout::from_keymap("de", keymap.as_bytes()).ok());
        },
    ));
    let wait = |host: &mut RealHost<'_, UnixStream>, keyboard: &str| {
        // Y on a US keyboard, Z on a German one.
        let mut typing = File::options()
            .write(true)
            .open(dev.join(keyboard))
            .unwrap();
        typing.write_all(&event(21, 1)).unwrap();
        let Input::Keys(presses) = host.wait(Duration::from_millis(10)).unwrap() else {
            panic!("no keys from {keyboard}");
        };
        (presses[0].key, host.layout_name())
    };

    assert_eq!(wait(&mut host, "event1"), (Key::Char('z'), "de".to_owned()));
    assert_eq!(wait(&mut host, "event0"), (Key::Char('y'), "us".to_owned()));
    // A Super+Space read with them switches the layout of the keys after
    // them: they were typed on the one before.
    host.next_layout();
    assert_eq!(host.keyboard_layout(), "de");
    assert_eq!(host.layout_name(), "us");
    assert_eq!(wait(&mut host, "event0"), (Key::Char('z'), "de".to_owned()));
    std::fs::remove_dir_all(&root).unwrap();
}
