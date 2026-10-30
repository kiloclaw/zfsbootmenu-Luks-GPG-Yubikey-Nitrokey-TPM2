//! The console without a display relays on the kernel console, and the
//! hand-over is asked for and waited on.

use std::io::{Read as _, Write as _};
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::time::Duration;

use pretty_assertions::assert_eq;
use zbm_testkit::TempDir;

use super::*;

/// The places of a machine without DRM cards or keyboards, in `dir`.
fn places(dir: &TempDir) -> Places {
    Places {
        sys: dir.join("sys"),
        dev: dir.join("dev"),
        base: dir.join("zfsbootmenu"),
        kmsg: dir.write("kmsg", ""),
        socket: dir.join("run/console.sock"),
    }
}

fn file(stream: UnixStream) -> File {
    File::from(OwnedFd::from(stream))
}

/// Runs the console on `settings` without a display, while the program
/// writes `hello` and goes; returns what the kernel console showed.
fn without_display(dir: &TempDir, settings: Settings) -> String {
    let places = places(dir);
    std::fs::create_dir_all(&places.base).unwrap();
    std::fs::write(places.base.join("console"), "77\n").unwrap();
    let (master, mut program) = UnixStream::pair().unwrap();
    let (terminal, mut user) = UnixStream::pair().unwrap();
    let vt = dir.join("tty1");
    let console = std::thread::spawn(move || {
        let (mut master, mut terminal) = (file(master), file(terminal));
        let mut asked = false;
        let terminals = Terminals {
            master: &mut master,
            terminal: &mut terminal,
            vt: &vt,
            cells: None,
        };
        let mut grid = |_, _| {
            asked = true;
            Err(io::Error::other("no font"))
        };
        let layouts = Layouts {
            first: None,
            names: Vec::new(),
            load: &mut |name: &str| Err(format!("{name}: not in this image")),
        };
        let (control, mut wake) = Control::new().unwrap();
        let served = serve_on(
            &settings,
            &places,
            (&mut grid, layouts),
            terminals,
            (&control, &mut wake),
        );
        served.map(|()| asked)
    });

    program.write_all(b"hello").unwrap();
    let mut shown = [0_u8; 12];
    user.read_exact(&mut shown).unwrap();
    drop(program);
    let asked = console.join().unwrap().unwrap();

    assert!(!asked, "no display, so no font");
    assert!(!dir.join("zfsbootmenu/console").exists());
    String::from_utf8_lossy(&shown).into_owned()
}

#[test]
fn without_a_card_the_console_relays_on_the_kernel_console() {
    let dir = TempDir::new("zbm-console-serve");

    let shown = without_display(&dir, Settings::default());

    assert_eq!(shown, "\x1b[H\x1b[J\nhello");
    let log = std::fs::read_to_string(dir.join("kmsg")).unwrap();
    assert_eq!(
        log,
        "<4>ZFSBootMenu: console: no DRM card; going on on the kernel console\n"
    );
}

#[test]
fn a_card_that_cannot_be_opened_is_said_so() {
    let dir = TempDir::new("zbm-console-serve");
    let settings = Settings {
        card: Some(dir.join("dri/card9")),
        mirror: None,
        ..Settings::default()
    };

    without_display(&dir, settings);

    let log = std::fs::read_to_string(dir.join("kmsg")).unwrap();
    assert!(log.contains("going on on the kernel console"), "{log}");
}

#[test]
fn the_mirror_gets_lines_of_their_own() {
    let dir = TempDir::new("zbm-console-serve");
    let mirror = dir.write("mirror", "");
    let settings = Settings {
        card: None,
        mirror: Some(mirror.clone()),
        ..Settings::default()
    };

    settings.mirror_line(RELEASED);
    Settings::default().mirror_line(RELEASED);

    assert_eq!(
        std::fs::read_to_string(&mirror).unwrap(),
        format!("\x0c{RELEASED}\n")
    );
    assert_eq!(Places::system().kmsg, PathBuf::from("/dev/kmsg"));
    assert!(open_mirror(&mirror).is_some());
    assert!(open_mirror(&dir.join("none/mirror")).is_none());
}

#[test]
fn without_a_console_there_is_nothing_to_hand_back_and_a_silent_one_is_late() {
    let dir = TempDir::new("zbm-console-release");
    let socket = places(&dir).socket;
    let timeout = Duration::from_secs(5);

    // No console: nothing to do, nothing to take again, nobody to ask.
    assert_eq!(release(&socket, timeout).unwrap(), Released::Done);
    assert!(reattach(&socket, timeout).unwrap());
    let switched = request_layout(&socket, "de", timeout).unwrap();
    assert_eq!(switched, Switched::NoConsole);
    assert_eq!(status(&socket, timeout), None);

    // A console that never answers. (One that does is in the tests of
    // `crate::control`.)
    let _silent = Listener::bind(&socket).unwrap();
    let short = Duration::from_millis(30);
    assert_eq!(release(&socket, short).unwrap(), Released::Late);
    assert!(!reattach(&socket, short).unwrap());
    assert_eq!(request_font(&socket, "+", short).unwrap(), Switched::Late);
}

/// The German layout, from the XKB data of the machine (`xkb-data`).
fn german(name: &str) -> Result<Layout, String> {
    let xkb = Path::new(zbm_display::layout::compile::XKB);
    let keymap = zbm_display::layout::compile::keymap(xkb, name).map_err(|e| e.to_string())?;
    Layout::from_keymap(name, keymap.as_bytes()).map_err(|e| e.to_string())
}

#[test]
fn a_layout_asked_for_is_taken_and_recorded() {
    let dir = TempDir::new("zbm-console-layout");
    let places = places(&dir);
    std::fs::create_dir_all(&places.base).unwrap();
    let state = State::new(&RealFiles, &places.base);
    let mut keyboards = Keyboards::from_files(Vec::new());

    switch_layout(&places, &mut german, &mut keyboards, "de");
    assert_eq!(keyboards.layout(), "de");
    switch_layout(&places, &mut german, &mut keyboards, "zz");

    assert_eq!(keyboards.layout(), "de");
    assert_eq!(state.console_layout().as_deref(), Some("de"));
    let log = std::fs::read_to_string(dir.join("kmsg")).unwrap();
    assert_eq!(
        log,
        "<6>ZFSBootMenu: console: keyboard de\n\
         <4>ZFSBootMenu: console: zz: no such keyboard layout; keeping keyboard de\n"
    );
}

#[test]
fn the_start_is_announced_with_the_font_and_the_layout() {
    let dir = TempDir::new("zbm-console-start");
    let places = places(&dir);
    std::fs::create_dir_all(&places.base).unwrap();
    let mirror = dir.write("mirror", "");
    let settings = Settings {
        card: None,
        mirror: Some(mirror.clone()),
        ..Settings::default()
    };
    let started = Started {
        card: PathBuf::from("/dev/dri/card0"),
        display: (1280, 800),
        font: "DejaVuSansMono.ttf".to_owned(),
        cell: (10, 20),
        grid: (128, 40),
        keyboard: "de".to_owned(),
    };

    announce(&settings, &places, &started);

    let text = "on /dev/dri/card0: 1280x800 display, DejaVuSansMono.ttf 10x20 cells, 128x40, \
                keyboard de";
    let log = std::fs::read_to_string(dir.join("kmsg")).unwrap();
    assert_eq!(log, format!("<6>ZFSBootMenu: console: {text}\n"));
    let mirrored = std::fs::read_to_string(&mirror).unwrap();
    assert_eq!(mirrored, format!("\x0czbm console: {text}\n"));
    let state = State::new(&RealFiles, &places.base);
    assert_eq!(state.console_layout().as_deref(), Some("de"));
    let layouts = Layouts {
        first: None,
        names: Vec::new(),
        load: &mut |name: &str| Err(name.to_owned()),
    };
    assert_eq!(format!("{layouts:?}"), "Layouts { first: None, .. }");
}

#[test]
fn without_a_virtual_terminal_the_console_cannot_serve() {
    let dir = TempDir::new("zbm-console-novt");
    let places = places(&dir);
    let mut grid = |_, _| Err(io::Error::other("no font"));
    let layouts = Layouts {
        first: None,
        names: Vec::new(),
        load: &mut |name: &str| Err(name.to_owned()),
    };

    let error = serve(&Settings::default(), &places, &mut grid, layouts).unwrap_err();

    assert!(error.to_string().contains("dev/tty0"), "{error}");
}

#[test]
fn a_new_input_device_wakes_the_console() {
    let dir = TempDir::new("zbm-console-watch");
    std::fs::create_dir_all(dir.join("dev/input")).unwrap();
    let mut devices = watch_devices(&dir.join("dev")).unwrap();
    let mut buffer = [0_u8; 256];
    assert!(devices.read(&mut buffer).is_err(), "nothing appeared yet");
    dir.write("dev/input/event7", "");
    assert!(devices.read(&mut buffer).unwrap() > 0);
    assert!(watch_devices(&dir.join("none")).is_none());
    // A request wakes the console too: the tests of `crate::control`.
}

#[test]
fn super_space_goes_to_the_image_s_next_layout() {
    let dir = TempDir::new("zbm-console-next");
    let places = places(&dir);
    let mut keyboards = Keyboards::from_files(Vec::new());
    let mut asked = Vec::new();
    let names = ["de".to_owned(), "fr".to_owned()];
    // Typing on the US layout, which is not among them: the first.
    next_layout(
        &places,
        &names,
        &mut |name: &str| {
            asked.push(name.to_owned());
            Err(format!("{name}: not in this image"))
        },
        &mut keyboards,
    );
    assert_eq!(asked, ["de"]);
    assert_eq!(keyboards.layout(), "us");
    let log = std::fs::read_to_string(dir.join("kmsg")).unwrap();
    assert!(
        log.contains("<4>ZFSBootMenu: console: de: not in this image"),
        "{log}"
    );
    // Without layouts, nothing to switch to.
    next_layout(
        &places,
        &[],
        &mut |name: &str| Err(name.to_owned()),
        &mut keyboards,
    );
    assert_eq!(keyboards.layout(), "us");
}

#[test]
fn a_card_that_is_not_there_is_not_waited_for() {
    let dir = TempDir::new("zbm-console-seat");
    let places = places(&dir);
    let seat = Seat::new(dir.join("dri/card0"), None);
    let start = std::time::Instant::now();

    let (back, error) = take_seat(&places, seat, std::time::Duration::from_secs(10)).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::NotFound, "{error}");
    assert_eq!(back.card(), dir.join("dri/card0"));
    assert!(start.elapsed() < std::time::Duration::from_secs(5));
}
