//! Helpers for the tests of this crate.

pub use zbm_testkit::legacy::{LINE_LOG_STUBS, stub_lines};
pub use zbm_testkit::redirect::Redirect;

/// `text` parsed as a name, which a test knows is valid.
pub fn name<T>(text: &str) -> T
where
    T: std::str::FromStr<Err = zbm_types::ParseError>,
{
    text.parse()
        .unwrap_or_else(|error| panic!("{text} is not a valid name: {error}"))
}
