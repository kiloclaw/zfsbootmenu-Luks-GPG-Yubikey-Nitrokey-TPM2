//! `org.zfsbootmenu:kernel` (input I4 of docs/dev/threat-model.md), which a
//! foreign pool (A3) sets: the pattern is translated and compiled, then
//! matched against kernel paths. Neither may panic, and a selector prints as
//! the text it was parsed from. The `regex` crate matches in linear time, so
//! no pattern makes a match hang.

#![no_main]

use libfuzzer_sys::fuzz_target;
use zbm_types::KernelSelector;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    // The pattern, then the text after the first newline as a path.
    let (pattern, path) = text
        .split_once('\n')
        .unwrap_or((text, "/boot/vmlinuz-6.12.9_1"));
    if let Ok(selector) = pattern.parse::<KernelSelector>() {
        assert_eq!(selector.to_string(), pattern);
        let _ = selector.matches(path);
        let _ = selector.matches("");
    }
});
