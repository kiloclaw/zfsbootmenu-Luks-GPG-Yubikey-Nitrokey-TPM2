//! Panic messages for humans
//!
//! Reduce the effort for users to report panics back to you
//!
//! You can still get the raw output by either:
//! - Running a debug build
//! - Setting `RUST_BACKTRACE=1`
//!
//! ## Example
//!
//! Add to your `fn main()`:
//! ```rust
//! human_panic::setup_panic!();
//! ```
//!
//! When run locally in a release build:
//! ```txt
//! my-program had a problem and crashed. To help us diagnose the problem you can send us a crash report.
//!
//! We have generated a report file at "/var/folders/zw/bpfvmq390lv2c6gn_6byyv0w0000gn/T/report-8351cad6-d2b5-4fe8-accd-1fcbf4538792.toml". Submit an issue or email with the subject of "human-panic Crash Report" and include the report as an attachment.
//!
//! - Homepage: https://github.com/rust-ci/human-panic
//! - Authors: Yoshua Wuyts <yoshuawuyts@gmail.com>
//!
//! We take privacy seriously, and do not perform any automated error collection. In order to improve the software, we rely on people to submit reports.
//!
//! Thank you kindly!
//! ```
//!
//! Traditional output:
//! ```txt
//! thread 'main' panicked at 'oops', examples/main.rs:2:3
//! note: Run with `RUST_BACKTRACE=1` for a backtrace.
//! ```

#![cfg_attr(docsrs, feature(doc_cfg))]
#![warn(clippy::print_stderr)]
#![warn(clippy::print_stdout)]

#[doc = include_str!("../README.md")]
#[cfg(doctest)]
pub struct ReadmeDoctests;

mod metadata;
mod panic;

pub mod report;
pub use metadata::Metadata;
pub use panic::PanicStyle;
pub use panic::handle_dump;
pub use panic::print_msg;
pub use panic::setup_panic;

/// Collect Cargo [`Metadata`]
///
/// ## Example
///
/// ```rust
/// use human_panic::metadata;
///
/// let metadata = metadata!()
///     .support("- Open a support request by email to support@mycompany.com");
/// ```
#[macro_export]
macro_rules! metadata {
    () => {{
        $crate::Metadata::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
            .authors(env!("CARGO_PKG_AUTHORS").replace(":", ", "))
            .homepage(env!("CARGO_PKG_HOMEPAGE"))
            .repository(env!("CARGO_PKG_REPOSITORY"))
    }};
}

/// Register `human-panic`
///
/// The macro should be called from within a function, for example as the first line of the
/// `main()` function of the program.
///
/// ## Example
///
/// Default [`metadata!`]
/// ```rust
/// use human_panic::setup_panic;
///
/// setup_panic!();
/// ```
///
/// Extend or override default [`metadata!`]
/// ```rust
/// use human_panic::setup_panic;
/// use human_panic::metadata;
///
/// setup_panic!(metadata!()
///     .authors("My Company Support <support@mycompany.com>")
///     .support("- Open a support request by email to support@mycompany.com")
/// );
/// ```
#[macro_export]
macro_rules! setup_panic {
    ($meta:expr) => {{
        $crate::setup_panic(|| $meta);
    }};

    () => {
        $crate::setup_panic!($crate::metadata!());
    };
}
