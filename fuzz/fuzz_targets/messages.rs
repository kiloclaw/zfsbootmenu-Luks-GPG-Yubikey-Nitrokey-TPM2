//! What ZFSBootMenu's own processes leave for each other (input I11 of
//! docs/dev/threat-model.md): crash reports, the kernel log they read back,
//! the state files of `/zfsbootmenu` (finished stages, the active menu, the
//! default boot environment, degraded pools, the command line override, the
//! console's process, the console that handed the display back, and its
//! layout), and the saved command line. Only ZFSBootMenu writes them, but
//! hooks (I6) can, so reading them may not panic or hang. Also the messages
//! on the session's and the console's sockets (I17), which a root login can
//! send, and on the client channel between the console and the menu (I18).

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
    "console",
    "console-layout",
    "console-released",
];

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let _ = zbm_trace::crash::Report::parse(&text);
    for record in zbm_platform::dmesg::records(data) {
        assert!(record.level <= 7, "{record:?}");
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
