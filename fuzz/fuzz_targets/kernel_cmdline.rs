//! The kernel command line of ZFSBootMenu and its configuration file (inputs
//! I1 and I2 of docs/dev/threat-model.md): someone who can change the image
//! (A1) chooses them. Parsing must never panic or hang. So do the kernel's
//! index of its modules and `modprobe.d`, which the runtime reads to load
//! modules itself (ROADMAP.md CD4), and the module parameters on the
//! command line.

#![no_main]

use libfuzzer_sys::fuzz_target;
use zbm_config::args::{ZbmArgs, cmdline_files};
use zbm_config::conf::Conf;
use zbm_config::options::ZbmOptions;
use zbm_types::KernelCommandLine;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let (cmdline, _) = KernelCommandLine::lenient(&text);
    let _ = cmdline_files(&cmdline);
    // What start-up writes to zbm.cmdline reads back the same.
    let again = KernelCommandLine::lenient(&cmdline.one_per_line()).0;
    assert_eq!(again, cmdline);
    let args = ZbmArgs::new(&cmdline);
    let _ = ZbmOptions::parse(&args);
    let _ = Conf::parse(&text);
    // The input split into the four files of the index, the configuration
    // and the command line.
    let mut parts = text.split('\x1e');
    let mut next = || parts.next().unwrap_or_default();
    let kernel = zbm_platform::modules::index::KernelFiles {
        dep: next(),
        alias: next(),
        softdep: next(),
        builtin: next(),
    };
    let config = next();
    let index = zbm_platform::modules::index::Index::parse(
        std::path::Path::new("/lib/modules/x"),
        &kernel,
        &[config],
        next(),
    );
    for name in next().split_whitespace().take(8) {
        let _ = index.plan(name, &["a=1"]);
        let _ = index.file(name);
    }
});
