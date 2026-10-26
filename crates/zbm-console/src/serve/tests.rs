//! The console without a display relays on the kernel console, and the
//! hand-over is asked for and waited on.

use std::io::{Read as _, Write as _};
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::sync::atomic::Ordering;
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
        };
        let mut grid = |_| {
            asked = true;
            Err(io::Error::other("no font"))
        };
        let layouts = Layouts {
            first: None,
            load: &mut |name: &str| Err(format!("{name}: not in this image")),
        };
        let served = serve_on(
            &settings,
            &places,
            (&mut grid, layouts),
            terminals,
            &Requests::default(),
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
fn the_hand_over_is_asked_for_and_waited_on() {
    let dir = TempDir::new("zbm-console-release");
    let base = dir.join("zfsbootmenu");
    std::fs::create_dir_all(&base).unwrap();
    let timeout = Duration::from_secs(5);

    // No console: nothing to do.
    assert_eq!(release(&base, timeout).unwrap(), Released::Done);

    // A console that is gone.
    std::fs::write(base.join("console"), "4194000\n").unwrap();
    assert_eq!(release(&base, timeout).unwrap(), Released::Done);
    assert!(!base.join("console").exists());

    // This process plays the console, which removes its record once asked.
    let requested = crate::release::on_request().unwrap();
    std::fs::write(base.join("console"), format!("{}\n", std::process::id())).unwrap();
    let record = base.join("console");
    let console = std::thread::spawn(move || {
        while !requested.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(5));
        }
        std::fs::remove_file(record).unwrap();
    });
    assert_eq!(release(&base, timeout).unwrap(), Released::Done);
    console.join().unwrap();

    // A console that never answers.
    std::fs::write(base.join("console"), format!("{}\n", std::process::id())).unwrap();
    assert_eq!(
        release(&base, Duration::from_millis(30)).unwrap(),
        Released::Late
    );
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

    switch_layout(&places, &mut german, &mut keyboards);
    assert_eq!(state.console_layout().as_deref(), Some("us"));
    state.request_console_layout("de").unwrap();
    switch_layout(&places, &mut german, &mut keyboards);
    assert_eq!(keyboards.layout(), "de");
    state.request_console_layout("zz").unwrap();
    switch_layout(&places, &mut german, &mut keyboards);

    assert_eq!(keyboards.layout(), "de");
    assert_eq!(state.console_layout().as_deref(), Some("de"));
    assert!(!state.has_console_layout_request());
    let log = std::fs::read_to_string(dir.join("kmsg")).unwrap();
    assert_eq!(
        log,
        "<6>ZFSBootMenu: console: keyboard de\n\
         <4>ZFSBootMenu: console: zz: no such keyboard layout; keeping keyboard de\n"
    );
}

#[test]
fn a_layout_request_waits_for_the_console() {
    let dir = TempDir::new("zbm-console-layout");
    let places = places(&dir);
    std::fs::create_dir_all(&places.base).unwrap();
    let state = State::new(&RealFiles, &places.base);
    let timeout = Duration::from_secs(5);

    assert_eq!(
        request_layout(&places.base, "de", timeout).unwrap(),
        Switched::NoConsole
    );
    state.set_console(std::process::id()).unwrap();
    let requested = crate::release::on_layout_request().unwrap();
    let console_places = places.clone();
    let console = std::thread::spawn(move || {
        while !requested.swap(false, Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(5));
        }
        let mut keyboards = Keyboards::from_files(Vec::new());
        switch_layout(&console_places, &mut german, &mut keyboards);
    });

    let switched = request_layout(&places.base, "de", timeout).unwrap();
    console.join().unwrap();

    assert_eq!(switched, Switched::Done("de".to_owned()));
    let late = request_layout(&places.base, "fr", Duration::from_millis(30)).unwrap();
    assert_eq!(late, Switched::Late);
    assert!(!state.has_console_layout_request());
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
        load: &mut |name: &str| Err(name.to_owned()),
    };
    assert_eq!(format!("{layouts:?}"), "Layouts { first: None, .. }");
}

#[test]
fn without_a_virtual_terminal_the_console_cannot_serve() {
    let dir = TempDir::new("zbm-console-novt");
    let places = places(&dir);
    let mut grid = |_| Err(io::Error::other("no font"));
    let layouts = Layouts {
        first: None,
        load: &mut |name: &str| Err(name.to_owned()),
    };

    let error = serve(&Settings::default(), &places, &mut grid, layouts).unwrap_err();

    assert!(error.to_string().contains("dev/tty0"), "{error}");
}

#[test]
fn a_new_input_device_and_a_request_s_signal_wake_the_console() {
    let dir = TempDir::new("zbm-console-watch");
    std::fs::create_dir_all(dir.join("dev/input")).unwrap();
    let mut devices = watch_devices(&dir.join("dev")).unwrap();
    let mut buffer = [0_u8; 256];
    assert!(devices.read(&mut buffer).is_err(), "nothing appeared yet");
    dir.write("dev/input/event7", "");
    assert!(devices.read(&mut buffer).unwrap() > 0);
    assert!(watch_devices(&dir.join("none")).is_none());

    let mut wake = File::from(wake_on_requests().unwrap());
    assert!(wake.read(&mut buffer).is_err(), "no request yet");
    let me = rustix::process::Pid::from_raw(i32::try_from(std::process::id()).unwrap()).unwrap();
    // The flags first, as the console registers them.
    let _flag = crate::release::on_layout_request().unwrap();
    rustix::process::kill_process(me, rustix::process::Signal::USR2).unwrap();
    let start = std::time::Instant::now();
    let woken = loop {
        match wake.read(&mut buffer) {
            Ok(length) => break length,
            Err(_) if start.elapsed() < Duration::from_secs(5) => std::thread::yield_now(),
            Err(error) => panic!("no wake-up: {error}"),
        }
    };
    assert!(woken > 0);
}
