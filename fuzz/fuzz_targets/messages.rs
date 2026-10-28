//! What ZFSBootMenu's own processes leave for each other (input I11 of
//! docs/dev/threat-model.md): crash reports, the kernel log they read back,
//! the state files of `/zfsbootmenu` (finished stages, the active menu, the
//! default boot environment, degraded pools, the command line override, the
//! console's process and its layout and font requests), and the saved
//! command line. Only ZFSBootMenu writes them, but hooks (I6) can, so
//! reading them may not panic or hang.

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
    "console-layout-request",
    "console-font-request",
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
    if let Some(name) = state.console_layout_request() {
        let _ = zbm_display::layout::compile::split(&name);
    }
    if let Some(zoom) = state.console_font_request() {
        let _ = zbm_console::screen::zoom::Zoom::parse(&zoom);
    }
});
