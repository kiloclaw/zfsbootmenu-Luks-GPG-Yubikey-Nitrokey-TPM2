//! Helpers for the tests of this crate.

use std::path::Path;

use zbm_system::KernelLog;
use zbm_testutil::fakes::FakeSystem;
use zbm_testutil::legacy::{Bash, LOG_STUBS};

/// The legacy LUKS library, loaded as the unit suites load it, with messages
/// on standard error as `LEVEL: text`.
pub fn legacy_luks() -> Bash {
    Bash::new()
        .code(LOG_STUBS)
        .code("_ZFSBOOTMENU_CORE=1")
        .code(r#"LUKS_KEYS_LIB="$LEGACY/zfsbootmenu/lib/zfsbootmenu-luks-keys.sh""#)
        .source("zfsbootmenu/lib/zfsbootmenu-kcl.sh")
        .source("zfsbootmenu/lib/zfsbootmenu-luks.sh")
}

/// A kernel log of the fake machine that writes every level.
pub fn log(fake: &FakeSystem) -> KernelLog<'_> {
    KernelLog::new(&fake.files, "7", Path::new("/zfsbootmenu"), 1)
}

/// The text of a legacy function from a library in `legacy/`, to run it with
/// changes.
pub fn legacy_function(library: &str, name: &str) -> String {
    let text = std::fs::read_to_string(zbm_testutil::legacy::path(library)).unwrap();
    let start = text.find(&format!("\n{name}() {{\n")).unwrap() + 1;
    let end = start + text[start..].find("\n}\n").unwrap() + 3;
    text[start..end].to_owned()
}
