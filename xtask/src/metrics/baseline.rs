//! The committed baselines of the metrics: plain text tables.
//!
//! ```text
//! # comment
//! crate      long_functions  flag_functions
//! zbm-core                2               7
//! ```
//!
//! The first line that is not a comment names the columns; every other line
//! is one crate. A crate that is not in the table has a baseline of zero, so
//! new crates start clean.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::error::{Error, Result};

/// The baseline: per crate, per metric key, the highest allowed value.
pub type Baseline = BTreeMap<String, BTreeMap<String, usize>>;

/// The header comment written above the table of the debt metrics.
pub const DEBT_HEADER: &str = "\
# The debt metrics of each crate at their last improvement, written by
# `cargo xtask metrics --bless`. `cargo xtask metrics --check` fails when a
# crate goes above them; a crate that is not listed must stay at zero.
# Lower a number by improving the code and running the command again;
# never raise one by hand.
";

/// Parses the baseline file `text`.
///
/// # Errors
///
/// [`Error::Format`] when a row does not match the header.
pub fn parse(text: &str) -> Result<Baseline> {
    let mut rows = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'));
    let Some(header) = rows.next() else {
        return Ok(Baseline::new());
    };
    let columns: Vec<&str> = header.split_whitespace().skip(1).collect();
    let mut baseline = Baseline::new();
    for row in rows {
        let mut fields = row.split_whitespace();
        let name = fields.next().unwrap_or_default();
        let values: Vec<&str> = fields.collect();
        if values.len() != columns.len() {
            return Err(Error::Format(format!(
                "metrics baseline: {name} has {} values for {} columns",
                values.len(),
                columns.len()
            )));
        }
        let mut metrics = BTreeMap::new();
        for (column, value) in columns.iter().zip(values) {
            let value = value.parse().map_err(|_| {
                Error::Format(format!("metrics baseline: {name} {column} is not a number"))
            })?;
            metrics.insert((*column).to_owned(), value);
        }
        baseline.insert(name.to_owned(), metrics);
    }
    Ok(baseline)
}

/// Formats `baseline` with the columns `keys`, aligned, below the comment
/// `header`.
pub fn format(baseline: &Baseline, header: &str, keys: &[&str]) -> String {
    let name_width = baseline
        .keys()
        .map(String::len)
        .chain(["crate".len()])
        .max()
        .unwrap_or_default();
    let mut text = String::from(header);
    let _ = write!(text, "{:name_width$}", "crate");
    for key in keys {
        let _ = write!(text, "  {key}");
    }
    text.push('\n');
    for (name, metrics) in baseline {
        let _ = write!(text, "{name:name_width$}");
        for key in keys {
            let value = metrics.get(*key).copied().unwrap_or_default();
            let _ = write!(text, "  {value:>width$}", width = key.len());
        }
        text.push('\n');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_formatted_baseline_parses_back() {
        let mut baseline = Baseline::new();
        baseline.insert(
            "zbm-core".to_owned(),
            [
                ("long_functions".to_owned(), 2),
                ("unsafe_blocks".to_owned(), 0),
            ]
            .into(),
        );

        let keys = ["long_functions", "unsafe_blocks"];
        let text = format(&baseline, DEBT_HEADER, &keys);

        assert!(text.contains("zbm-core               2              0\n"));
        assert_eq!(parse(&text).unwrap(), baseline);
    }

    #[test]
    fn an_empty_file_is_an_empty_baseline() {
        assert_eq!(parse("# nothing yet\n").unwrap(), Baseline::new());
    }

    #[test]
    fn a_short_row_is_an_error() {
        let error = parse("crate a b\nzbm-core 1\n").unwrap_err();

        assert_eq!(
            error.to_string(),
            "metrics baseline: zbm-core has 1 values for 2 columns"
        );
    }
}
