//! The requests of `zbm console release` and `zbm console layout`, with
//! this process as the console: the signals reach it and wake it. They are
//! blocked and read from a `signalfd`, which needs every thread to block
//! them, so this test has no harness (`harness = false` in `Cargo.toml`):
//! the main thread blocks them first, and the console's thread, started
//! after, inherits the mask.

#![allow(clippy::unwrap_used, reason = "a failing check is meant to panic")]
#![allow(
    clippy::disallowed_methods,
    reason = "the checks use real files, threads and time"
)]

use std::os::fd::AsRawFd as _;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use zbm_console::release::{Requests, Wake as _};
use zbm_console::serve::{Places, Released, Switched, release, request_layout, switch_layout};
use zbm_display::input::Keyboards;
use zbm_display::layout::Layout;
use zbm_platform::RealFiles;
use zbm_state::State;
use zbm_testkit::TempDir;

fn main() {
    let requests = Requests::listen().unwrap();
    a_request_s_signal_wakes_the_console(&requests);
    the_hand_over_is_asked_for_and_waited_on(&requests);
    a_layout_request_waits_for_the_console(&requests);
}

/// The places of a machine without DRM cards or keyboards, in `dir`.
fn places(dir: &TempDir) -> Places {
    Places {
        sys: dir.join("sys"),
        dev: dir.join("dev"),
        base: dir.join("zfsbootmenu"),
        kmsg: dir.write("kmsg", ""),
    }
}

fn a_request_s_signal_wakes_the_console(requests: &Requests) {
    let mut wake = requests.wake().unwrap();
    let readable =
        || zbm_sys::console::first_readable(&[wake.fd().as_raw_fd()], Some(Duration::ZERO));
    assert_eq!(readable().unwrap(), None, "no request yet");
    zbm_sys::send_signal(std::process::id(), zbm_sys::Signal::User2).unwrap();
    let woken =
        zbm_sys::console::first_readable(&[wake.fd().as_raw_fd()], Some(Duration::from_secs(5)));
    assert_eq!(woken.unwrap(), Some(0));
    wake.take();
    assert!(requests.layout.swap(false, Ordering::SeqCst));
    assert!(!requests.release.load(Ordering::SeqCst));
    let readable = zbm_sys::console::first_readable(&[wake.fd().as_raw_fd()], Some(Duration::ZERO));
    assert_eq!(readable.unwrap(), None, "the wake-up was taken");
}

fn the_hand_over_is_asked_for_and_waited_on(requests: &Requests) {
    let dir = TempDir::new("zbm-console-release");
    let base = dir.join("zfsbootmenu");
    std::fs::create_dir_all(&base).unwrap();
    // This process plays the console, which removes its record once asked.
    std::fs::write(base.join("console"), format!("{}\n", std::process::id())).unwrap();
    let record = base.join("console");
    let console = std::thread::spawn({
        let requests = requests.clone();
        move || {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !requests.was_release_requested() {
                assert!(Instant::now() < deadline, "no request came");
                std::thread::sleep(Duration::from_millis(5));
            }
            std::fs::remove_file(record).unwrap();
        }
    });
    assert_eq!(
        release(&base, Duration::from_secs(5)).unwrap(),
        Released::Done
    );
    console.join().unwrap();
}

/// The German layout, from the XKB data of the machine (`xkb-data`).
fn german(name: &str) -> Result<Layout, String> {
    let xkb = Path::new(zbm_display::layout::compile::XKB);
    let keymap = zbm_display::layout::compile::keymap(xkb, name).map_err(|e| e.to_string())?;
    Layout::from_keymap(name, keymap.as_bytes()).map_err(|e| e.to_string())
}

fn a_layout_request_waits_for_the_console(requests: &Requests) {
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
    let console_places = places.clone();
    let console = std::thread::spawn({
        let requests = requests.clone();
        move || {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                requests.take();
                if requests.layout.swap(false, Ordering::SeqCst) {
                    break;
                }
                assert!(Instant::now() < deadline, "no request came");
                std::thread::sleep(Duration::from_millis(5));
            }
            let mut keyboards = Keyboards::from_files(Vec::new());
            switch_layout(&console_places, &mut german, &mut keyboards);
        }
    });

    let switched = request_layout(&places.base, "de", timeout).unwrap();
    console.join().unwrap();

    assert_eq!(switched, Switched::Done("de".to_owned()));
    let late = request_layout(&places.base, "fr", Duration::from_millis(30)).unwrap();
    assert_eq!(late, Switched::Late);
    assert!(!state.has_console_layout_request());
}
