//! `zbm explain`: what an error code means and what to do about it.
//!
//! - `zbm explain` lists every code with its summary;
//! - `zbm explain CODE` explains one, given as `ZBM-E0412`, `E0412` or `412`.

use zbm_types::code::{ErrorCode, Explanation};

use crate::{Io, USAGE};

/// The usage text.
const USAGE_TEXT: &str = "\
Usage: zbm explain           list the error codes
       zbm explain CODE      explain one, such as ZBM-E0412
";

/// The tables of every crate whose errors have codes, by crate.
const TABLES: &[&[Explanation]] = &[
    zbm_app::codes::ZFS_CODES,
    zbm_pool::POOL_CODES,
    zbm_luks::LUKS_CODES,
    std::slice::from_ref(&zbm_luks::pilot::UNLOCK_FAILED),
    zbm_app::codes::CRYPT_CODES,
    zbm_boot::BOOT_CODES,
    zbm_boot::saved_logs::SAVE_CODES,
    zbm_startup::STARTUP_CODES,
];

/// Every code, in order.
pub fn explanations() -> Vec<Explanation> {
    let mut all: Vec<Explanation> = TABLES
        .iter()
        .flat_map(|table| table.iter().copied())
        .collect();
    all.sort_by_key(|entry| entry.code);
    all
}

/// The explanation of `code`, if it has one.
pub fn find(code: ErrorCode) -> Option<Explanation> {
    explanations().into_iter().find(|entry| entry.code == code)
}

/// `text` as lines of at most `width` characters, cut between words.
fn wrapped(text: &str, width: usize) -> String {
    let mut lines = String::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.len() + 1 + word.len() > width {
            lines.push_str(&line);
            lines.push('\n');
            line.clear();
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    lines.push_str(&line);
    lines.push('\n');
    lines
}

/// `zbm explain`.
pub fn run(args: &[String], io: &mut Io) -> i32 {
    match args {
        [] => {
            for entry in explanations() {
                let _ = writeln!(io.stdout, "{}  {}", entry.code, entry.summary);
            }
            0
        }
        [code] if !code.starts_with('-') => {
            let Some(entry) = ErrorCode::parse(code).and_then(find) else {
                let _ = writeln!(io.stderr, "zbm explain: no error has the code {code}");
                return 1;
            };
            let _ = writeln!(io.stdout, "{}: {}\n", entry.code, entry.summary);
            let _ = io.stdout.write_all(wrapped(entry.text, 78).as_bytes());
            0
        }
        _ => {
            let _ = io.stderr.write_all(USAGE_TEXT.as_bytes());
            USAGE
        }
    }
}

#[cfg(test)]
mod tests;
