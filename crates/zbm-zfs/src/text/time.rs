//! A time as `ctime(3)` prints it, which `zpool status` gives for when a
//! checkpoint was taken: `Thu Nov  6 12:00:00 2026`, read as UTC, the time
//! zone of ZFSBootMenu's image, which has no `/etc/localtime`.

/// The months as `ctime(3)` names them.
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The days of the week as `ctime(3)` names them.
const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/// `text`, a time as `ctime(3)` prints it, in seconds since 1970, read as
/// UTC; `None` when it is not one, or is before 1970. The day of the week
/// must be one, but is not checked against the date.
pub(crate) fn ctime(text: &str) -> Option<u64> {
    let fields: Vec<&str> = text.split_whitespace().collect();
    let [weekday, month, day, time, year] = fields.as_slice() else {
        return None;
    };
    if !WEEKDAYS.contains(weekday) {
        return None;
    }
    let month = MONTHS.iter().position(|name| name == month)?;
    let month = u64::try_from(month).ok()?.checked_add(1)?;
    let day: u64 = number(day)?;
    let year: u64 = number(year)?;
    if !(1..=days_in(year, month)).contains(&day) || year < 1970 {
        return None;
    }
    let (hours, rest) = time.split_once(':')?;
    let (minutes, seconds) = rest.split_once(':')?;
    let (hours, minutes, seconds) = (number(hours)?, number(minutes)?, number(seconds)?);
    if hours > 23 || minutes > 59 || seconds > 60 {
        return None;
    }
    days_since_1970(year, month, day)?
        .checked_mul(86_400)?
        .checked_add(hours * 3600 + minutes * 60 + seconds)
}

/// `text` as a number of ASCII digits, at most four of them.
fn number(text: &str) -> Option<u64> {
    if text.is_empty() || text.len() > 4 || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// The number of days of `month` in `year`.
fn days_in(year: u64, month: u64) -> u64 {
    match month {
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// The days from 1970-01-01 to the date, by Howard Hinnant's algorithm
/// for the proleptic Gregorian calendar; `None` before 1970.
fn days_since_1970(year: u64, month: u64, day: u64) -> Option<u64> {
    let year = if month <= 2 {
        year.checked_sub(1)?
    } else {
        year
    };
    let era = year / 400;
    let of_era = year % 400;
    let shifted_month = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = of_era * 365 + of_era / 4 - of_era / 100 + day_of_year;
    (era * 146_097 + day_of_era).checked_sub(719_468)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ctime_is_read_as_seconds_in_utc() {
        assert_eq!(ctime("Thu Jan  1 00:00:00 1970"), Some(0));
        assert_eq!(ctime("Tue Feb 29 00:00:00 2000"), Some(951_782_400));
        assert_eq!(ctime("Mon Oct 26 10:15:15 2026"), Some(1_793_009_715));
        assert_eq!(ctime("Sun Feb 28 23:59:59 2100"), Some(4_107_542_399));
        // As `zpool status` prints it, without the newline.
        assert_eq!(ctime("  Fri Nov  6 12:00:00 2026 "), Some(1_793_966_400));
    }

    #[test]
    fn what_is_not_a_ctime_is_no_time() {
        for text in [
            "",
            "Thu Jan  1 00:00:00",
            "Thu Jan  1 00:00:00 1970 UTC",
            "Xyz Jan  1 00:00:00 1970",
            "Thu Foo  1 00:00:00 1970",
            "Thu Jan 32 00:00:00 1970",
            "Thu Feb 29 00:00:00 2100",
            "Thu Jan  0 00:00:00 1970",
            "Thu Jan  1 24:00:00 1970",
            "Thu Jan  1 00:60:00 1970",
            "Thu Jan  1 00:00:61 1970",
            "Thu Jan  1 00:00 1970",
            "Thu Jan  1 +1:00:00 1970",
            "Wed Dec 31 23:59:59 1969",
            "Thu Jan  1 00:00:00 99999",
            "Thu Jan  1 ０:00:00 1970",
        ] {
            assert_eq!(ctime(text), None, "{text:?}");
        }
    }
}
