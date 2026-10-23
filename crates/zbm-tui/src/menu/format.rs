//! Numbers as people read them.

/// `bytes` as `1.2 GiB`, `340 MiB`, `12 KiB`, `512 B`.
pub fn size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes;
    let mut unit = 0;
    let mut tenths = 0;
    while value >= 1024 && unit < UNITS.len() - 1 {
        tenths = (value % 1024) * 10 / 1024;
        value /= 1024;
        unit += 1;
    }
    let name = UNITS.get(unit).copied().unwrap_or_default();
    if unit > 0 && value < 10 {
        format!("{value}.{tenths} {name}")
    } else {
        format!("{value} {name}")
    }
}

/// How long ago `then` was at `now` (seconds since the epoch): `just now`,
/// `5 min ago`, `3 h ago`, `12 d ago`.
pub fn age(now: u64, then: u64) -> String {
    let seconds = now.saturating_sub(then);
    match seconds {
        0..60 => "just now".to_owned(),
        60..3600 => format!("{} min ago", seconds / 60),
        3600..86_400 => format!("{} h ago", seconds / 3600),
        _ => format!("{} d ago", seconds / 86_400),
    }
}

/// The date of `seconds` since the epoch, `2026-10-22`.
pub fn date(seconds: u64) -> String {
    let days = i64::try_from(seconds / 86_400).unwrap_or_default();
    let (year, month, day) = civil(days);
    format!("{year:04}-{month:02}-{day:02}")
}

/// The calendar date of a day number (days since 1970-01-01), by Howard
/// Hinnant's algorithm.
fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// `text` cut to `width` characters, with `…` (or `~` in ASCII) at the end
/// when cut.
pub fn fit(text: &str, width: usize, ascii: bool) -> String {
    if text.chars().count() <= width {
        return text.to_owned();
    }
    let mark = if ascii { '~' } else { '…' };
    let mut cut: String = text.chars().take(width.saturating_sub(1)).collect();
    if width > 0 {
        cut.push(mark);
    }
    cut
}

/// Short words that title case leaves in lower case, unless first or last.
const MINOR: [&str; 15] = [
    "a", "an", "and", "as", "at", "by", "for", "from", "in", "into", "of", "on", "or", "the", "to",
];

/// `text` in title case, as Apple writes titles, buttons and menu items:
/// every word capitalised but short ones inside the text (`Clone into a New
/// Boot Environment`); parts after `/` too (`Read/Write`).
pub fn title_case(text: &str) -> String {
    let words: Vec<&str> = text.split(' ').collect();
    let last = words.len().saturating_sub(1);
    let words: Vec<String> = words
        .iter()
        .enumerate()
        .map(|(index, word)| {
            if index != 0 && index != last && MINOR.contains(word) {
                return (*word).to_owned();
            }
            word.split('/')
                .map(capitalised)
                .collect::<Vec<_>>()
                .join("/")
        })
        .collect();
    words.join(" ")
}

/// `word` with its first letter in upper case.
pub fn capitalised(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn sizes_are_written_in_binary_units() {
        assert_eq!(size(512), "512 B");
        assert_eq!(size(12 * 1024), "12 KiB");
        assert_eq!(size(1536 * 1024 * 1024), "1.5 GiB");
        assert_eq!(size(340 * 1024 * 1024), "340 MiB");
    }

    #[test]
    fn ages_are_rounded_down() {
        assert_eq!(age(100, 90), "just now");
        assert_eq!(age(10_000, 10_000 - 300), "5 min ago");
        assert_eq!(age(100_000, 100_000 - 7300), "2 h ago");
        assert_eq!(age(10_000_000, 10_000_000 - 3 * 86_400), "3 d ago");
        assert_eq!(age(0, 100), "just now");
    }

    #[test]
    fn dates_are_calendar_dates() {
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(1_792_627_200), "2026-10-22");
        assert_eq!(date(951_782_400), "2000-02-29");
    }

    #[test]
    fn long_text_is_cut_with_a_mark() {
        assert_eq!(fit("zroot/ROOT/void", 20, false), "zroot/ROOT/void");
        assert_eq!(fit("zroot/ROOT/void", 8, false), "zroot/R…");
        assert_eq!(fit("zroot/ROOT/void", 8, true), "zroot/R~");
        assert_eq!(fit("abc", 0, false), "");
    }

    #[test]
    fn titles_are_capitalised_as_apple_does() {
        assert_eq!(title_case("roll back"), "Roll Back");
        assert_eq!(
            title_case("clone into a new boot environment"),
            "Clone into a New Boot Environment"
        );
        assert_eq!(title_case("read/write"), "Read/Write");
        assert_eq!(title_case("look into"), "Look Into");
        assert_eq!(title_case(""), "");
    }
}
