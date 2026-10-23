//! The LUKS device suites: the LUKS code against real LUKS2 volumes on loop
//! devices, ported from the Bash suites of `tests/luks/unit` case for case and
//! check for check (see [`harness`]).
//!
//! They need root, `cryptsetup`, `losetup`, `swtpm`, `systemd-cryptenroll`,
//! clevis and GnuPG, so they run only when `ZBM_DEVICE_TESTS=1` is set, and
//! say so otherwise; with it set they fail where they cannot run. As root:
//!
//! ```sh
//! cargo xtask test devices
//! ```
//!
//! The suites run one after another (they share `/dev/hidraw99`), each
//! printing its cases and checks as the Bash suites do.

#![allow(
    clippy::disallowed_macros,
    clippy::format_collect,
    clippy::format_push_string,
    reason = "the device suites print their cases and checks, and build the small texts of \
              their stand-ins, as the Bash suites do"
)]
#![allow(
    clippy::disallowed_methods,
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    clippy::too_many_lines,
    missing_docs,
    reason = "device suites set up real volumes and processes, and fail loudly when they cannot"
)]

mod config;
mod edge;
mod harness;
mod tokens;
mod unlock;
