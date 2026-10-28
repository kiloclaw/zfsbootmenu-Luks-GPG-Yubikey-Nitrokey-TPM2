//! The kernel command line of ZFSBootMenu and its configuration file (inputs
//! I1 and I2 of docs/dev/threat-model.md): someone who can change the image
//! (A1) chooses them. Parsing must never panic or hang.

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
});
