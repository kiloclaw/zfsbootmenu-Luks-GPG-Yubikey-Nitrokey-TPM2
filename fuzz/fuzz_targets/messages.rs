//! What ZFSBootMenu's own processes leave for each other (input I11 of
//! docs/dev/threat-model.md): crash reports, the kernel log they read back
//! and a device's messages the menu picks from it, by the names of the
//! device a pool's label gives, the state files of
//! `/zfsbootmenu` (finished stages, the active menu, the default boot
//! environment, degraded pools, the command line override, the console's
//! process, the console that handed the display back, and its layout),
//! and the saved command line. Only ZFSBootMenu writes them, but
//! hooks (I6) can, so reading them may not panic or hang. Also the messages
//! on the session's and the console's sockets (I17), a remote menu's among
//! them, which a root login can send, on the client channel between the
//! console and the menu (I18), and between a client and the keyholder that
//! opens its volumes (I19), and between the console and the supervisor
//! that opens its devices (I20).

#![no_main]

use std::path::Path;

use libfuzzer_sys::fuzz_target;
use zbm_testkit::fakes::FakeFiles;

/// The state files the input fills, one per line of it.
const STATE: [&str; 8] = [
    ".stages",
    "active",
    "bootfs",
    "degraded",
    "cmdline",
    "console/pid",
    "console/layout",
    "console/released",
];

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let _ = zbm_trace::crash::Report::parse(&text);
    // The kernel's messages about a device, which the menu draws: the
    // filter on names and numbers takes any text without a panic, and
    // nothing names an empty name.
    let words: Vec<&str> = text.split_whitespace().take(4).collect();
    for record in zbm_platform::dmesg::records(data) {
        assert!(record.level <= 7, "{record:?}");
        let _ = zbm_platform::dmesg::is_about(&record, &words, &["8:16"]);
        assert!(!zbm_platform::dmesg::names(&record.text, ""));
        for word in &words {
            let _ = zbm_platform::dmesg::names(&record.text, word);
        }
    }
    // The same words as a pool's label names its devices, on a machine
    // whose kernel lists none: a lost device's names are entries of
    // `/sys/class/block`, never a path.
    let (runner, files) = (zbm_testkit::FakeRunner::new(), FakeFiles::new());
    let devices = zbm_tools::devices::Devices::new(&runner, &files);
    for word in &words {
        for found in devices.stack(word) {
            assert!(
                !found.name.is_empty() && !found.name.contains('/'),
                "{found:?}"
            );
        }
    }
    let _ = zbm_console::screen::zoom::Zoom::parse(&text);
    let _ = zbm_display::layout::compile::split(&text);
    let _ = zbm_platform::console::parse_size(&text);
    let _ = zbm_config::args::ZbmArgs::from_cmdline(&text);
    if let Ok(request) = zbm_app::session::Request::decode(data) {
        assert_eq!(
            zbm_app::session::Request::decode(&request.encode()),
            Ok(request)
        );
    }
    if let Ok(request) = zbm_app::console::Request::decode(data) {
        assert_eq!(
            zbm_app::console::Request::decode(&request.encode()),
            Ok(request)
        );
    }
    if let Some((_, reply)) = zbm_app::console::Reply::decode(data) {
        assert_eq!(
            zbm_app::console::Reply::decode(&reply.encode()),
            Some((zbm_app::console::VERSION, reply))
        );
    }
    if let Some((_, answer)) = zbm_app::session::Answer::decode(data) {
        let encoded = answer.encode();
        assert_eq!(
            zbm_app::session::Answer::decode(&encoded),
            Some((zbm_app::session::VERSION, answer))
        );
    }
    // The keyholder's socket pair (I19): what is decoded encodes to a
    // message that decodes the same.
    if let Ok(message) = zbm_app::keyholder::ToKeyholder::decode(data) {
        let encoded = message.encode().expect("a decoded request has an encoding");
        assert_eq!(
            zbm_app::keyholder::ToKeyholder::decode(&encoded),
            Ok(message)
        );
    }
    if let Ok(message) = zbm_app::keyholder::FromKeyholder::decode(data) {
        assert_eq!(
            zbm_app::keyholder::FromKeyholder::decode(&message.encode()),
            Ok(message)
        );
    }
    // A remote menu on the session's socket (I17): what is decoded
    // encodes to a message that decodes the same.
    if let Ok(message) = zbm_app::remote::ToSession::decode(data) {
        let encoded = message.encode().expect("a decoded request has an encoding");
        assert_eq!(
            zbm_app::remote::ToSession::decode(&encoded).ok(),
            Some(message)
        );
    }
    if let Ok(message) = zbm_app::remote::ToMenu::decode(data) {
        let encoded = message.encode().expect("a decoded message has an encoding");
        assert_eq!(
            zbm_app::remote::ToMenu::decode(&encoded).ok(),
            Some(message)
        );
    }
    // The device broker's socket pair (I20 of SE11): a request decodes to
    // one that encodes back, and its path is judged by name without a
    // panic; the console reads the supervisor's replies the same way.
    if let Ok(request) = zbm_app::devices::Request::decode(data) {
        assert_eq!(
            zbm_app::devices::Request::decode(&request.encode()),
            Ok(request.clone())
        );
        let _ = request.kind.is_name(&request.path.to_string_lossy());
    }
    if let Some(reply) = zbm_app::devices::Reply::decode(data) {
        assert_eq!(
            zbm_app::devices::Reply::decode(&reply.encode()),
            Some(reply)
        );
    }
    for kind in zbm_display::devices::KINDS {
        let _ = kind.is_name(&text);
    }
    // The client channel between the console and the menu (I18): what is
    // decoded is exactly one message, and encodes to the same bytes.
    if let Some(message) = zbm_display::channel::FromClient::decode(data) {
        assert_eq!(message.encode(), data);
    }
    if let Some(message) = zbm_display::channel::FromConsole::decode(data) {
        assert_eq!(message.encode(), data);
    }

    let files = FakeFiles::new();
    files.dir("/zfsbootmenu");
    let mut lines = text.split('\x1e');
    for name in STATE {
        if let Some(contents) = lines.next() {
            files.file(Path::new("/zfsbootmenu").join(name), contents);
        }
    }
    let state = zbm_state::State::new(&files, "/zfsbootmenu");
    let _ = state.finished_stages();
    let _ = state.active_pid();
    let _ = state.bootfs();
    let _ = state.degraded_pools();
    let _ = state.cmdline_override();
    let _ = state.console_pid();
    let _ = state.released_console_pid();
    if let Some(name) = state.console_layout() {
        let _ = zbm_display::layout::compile::split(&name);
    }
});
