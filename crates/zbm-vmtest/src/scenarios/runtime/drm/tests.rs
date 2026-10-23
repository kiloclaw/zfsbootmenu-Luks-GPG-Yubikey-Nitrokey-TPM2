use pretty_assertions::assert_eq;

use super::*;
use crate::error::Error;
use crate::testing::{Step, checks, cleanup, scripted, settings};

/// A PPM screenshot of `count` colours, one row each.
fn ppm(count: u8) -> Vec<u8> {
    let mut bytes = format!("P6\n4 {count}\n255\n").into_bytes();
    for colour in 0..count {
        bytes.extend(std::iter::repeat_n([colour, 0, 0], 4).flatten());
    }
    bytes
}

/// Screenshots of the menu in 7 colours and of the console in 16.
fn dump(_: &Vm<'_>, path: &Path) -> Result<()> {
    let name = path.display().to_string();
    let count = if name.ends_with("menu.ppm") { 7 } else { 16 };
    fs::write(path, ppm(count)).map_err(Error::io("writing the screenshot"))
}

/// A screenshot that never comes.
fn no_dump(_: &Vm<'_>, _: &Path) -> Result<()> {
    Ok(())
}

fn key(_: &Vm<'_>, _: &str) -> Result<()> {
    Ok(())
}

/// What the guest says to each command of a run, in order.
fn script(answers: [&'static str; 7]) -> Vec<Step> {
    let leak = |text: String| -> &'static str { Box::leak(text.into_boxed_str()) };
    [DRIVER, START, MENU, SELECTED, MOVED, ENDED, BAR]
        .into_iter()
        .zip(answers)
        .flat_map(|(command, answer)| {
            [
                Step::Hear(leak(format!("{command}; echo RC=$?"))),
                Step::Say(answer),
            ]
        })
        .collect()
}

#[test]
fn the_menu_is_drawn_moved_booted_and_the_console_comes_back() {
    let settings = settings("runtime-drm-run");
    let test = Test::new(&settings, "runtime-rust-drm").unwrap();
    let answers = [
        "/sys/bus/pci/drivers/virtio-pci\r\nRC=0\r\n",
        "RC=0\r\n",
        "Boot Environments\r\n▶ tank/ROOT/void\r\n  tank/ROOT/arch\r\nRC=0\r\n",
        "▶ tank/ROOT/void\r\nRC=0\r\n",
        "▶ tank/ROOT/arch\r\nRC=0\r\n",
        "menu-demo: 1280x800 display: Booting tank/ROOT/arch\r\nRC=0\r\n",
        "RC=0\r\n",
    ];
    let (mut vm, guest) = scripted(&test, script(answers));
    let monitor = Monitor { dump, key };

    check_menu(&test, &mut vm, "virtio", "virtio", &monitor).unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    let checks = checks(&test);
    assert_eq!(checks.len(), 6, "{checks:?}");
    assert!(
        checks.iter().all(|check| check.starts_with("ok")),
        "{checks:?}"
    );
    assert!(test.dir().join("virtio-console.ppm").exists());
    cleanup(&settings);
}

#[test]
fn a_menu_that_does_not_show_or_move_fails_its_checks() {
    let settings = settings("runtime-drm-fail");
    let test = Test::new(&settings, "runtime-rust-drm").unwrap();
    let answers = [
        "/sys/bus/platform/drivers/vesafb\r\nRC=0\r\n",
        "RC=0\r\n",
        "\r\nRC=0\r\n",
        "\r\nRC=1\r\n",
        "\r\nRC=1\r\n",
        "menu-demo: no keyboard found\r\nRC=0\r\n",
        "RC=0\r\n",
    ];
    let (mut vm, guest) = scripted(&test, script(answers));
    let monitor = Monitor { dump: no_dump, key };

    check_menu(&test, &mut vm, "simpledrm", "simple", &monitor).unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    let checks = checks(&test);
    assert_eq!(checks.len(), 6, "{checks:?}");
    assert!(
        checks.iter().all(|check| check.starts_with("FAIL")),
        "{checks:?}"
    );
    cleanup(&settings);
}

#[test]
fn the_image_has_the_demo_and_the_drivers() {
    let spec = spec();

    assert_eq!(spec.runtime, Some(ZbmRuntime::Rust));
    assert_eq!(spec.features.as_deref(), Some("zbm/menu-demo"));
    assert!(spec.conf.contains("virtio_gpu evdev"), "{}", spec.conf);
}
