//! The kernel command line of ZFSBootMenu and its configuration file (inputs
//! I1 and I2 of docs/dev/threat-model.md): someone who can change the image
//! (A1) chooses them. Parsing must never panic or hang.

#![no_main]

use libfuzzer_sys::fuzz_target;
use zbm_cmdline::args::{ZbmArgs, cmdline_files};
use zbm_cmdline::conf::Conf;
use zbm_cmdline::kcl::tokenize;
use zbm_cmdline::options::ZbmOptions;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let tokens = tokenize(&text);
    let _ = cmdline_files(&tokens);
    let args = ZbmArgs::from_cmdline(&text);
    let _ = ZbmOptions::parse(&args);
    let _ = ZbmArgs::from_kcl(&tokens);
    let _ = Conf::parse(&text);
});
