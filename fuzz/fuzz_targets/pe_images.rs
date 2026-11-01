//! EFI images (input I12 of docs/dev/threat-model.md): the stubs
//! `generate-zbm` builds from and the images `zbm-kcl` reads from any file
//! or standard input (ADR 0014). Reading an image may not panic or hang; an
//! image read can take a `.cmdline` before its kernel, be written, and the
//! image written reads back with that command line.

#![no_main]

use libfuzzer_sys::fuzz_target;
use zbm_host::pe::{Image, WriteOptions};

fuzz_target!(|data: &[u8]| {
    let Ok(mut image) = Image::read(data.to_vec()) else {
        return;
    };
    let line = b"quiet\0";
    let replaced = image.replace(".cmdline", line).unwrap_or(false);
    if !replaced && image.insert_before(".cmdline", line, &[".linux"]).is_err() {
        return;
    }
    let Ok(written) = image.write(&WriteOptions::default()) else {
        return;
    };
    let read = Image::read(written.bytes).expect("an image written reads back");
    let cmdline = read
        .section(".cmdline")
        .expect("the image has its .cmdline");
    assert_eq!(cmdline.data(), line);
});
