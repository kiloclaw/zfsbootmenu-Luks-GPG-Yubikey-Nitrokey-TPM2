//! The names of snapshots taken at a time: the time stamp the legacy menu
//! names a new snapshot with, which the safety snapshot of a restore
//! ([`RESTORE_SNAPSHOT`]) ends with too.

use zbm_types::{DatasetName, SnapshotName};

use super::RESTORE_SNAPSHOT;

/// The safety snapshot of `environment` that a restore at `seconds` since
/// 1970 takes first: `<environment>@before-restore-<UTC time>`; `None`
/// when the name is too long for ZFS.
pub fn restore_snapshot(environment: &DatasetName, seconds: u64) -> Option<SnapshotName> {
    format!("{environment}@{RESTORE_SNAPSHOT}-{}", timestamp(seconds))
        .parse()
        .ok()
}

/// `seconds` since 1970 as the legacy menu names a snapshot:
/// `%Y-%m-%d-%H%M%S`, in UTC.
pub fn timestamp(seconds: u64) -> String {
    let days = seconds / 86_400;
    let time = seconds % 86_400;
    let (year, month, day) = civil(days);
    format!(
        "{year:04}-{month:02}-{day:02}-{:02}{:02}{:02}",
        time / 3600,
        time % 3600 / 60,
        time % 60
    )
}

/// The date `days` after 1970-01-01, by Howard Hinnant's algorithm for
/// the proleptic Gregorian calendar.
fn civil(days: u64) -> (u64, u64, u64) {
    let shifted = days + 719_468;
    let era = shifted / 146_097;
    let of_era = shifted % 146_097;
    let year_of_era = (of_era - of_era / 1460 + of_era / 36_524 - of_era / 146_096) / 365;
    let day_of_year = of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_stamps_are_dates_and_times_in_utc() {
        assert_eq!(timestamp(0), "1970-01-01-000000");
        assert_eq!(timestamp(951_782_400), "2000-02-29-000000");
        assert_eq!(timestamp(1_793_009_715), "2026-10-26-101515");
        assert_eq!(timestamp(4_107_542_399), "2100-02-28-235959");
    }

    #[test]
    fn a_safety_snapshot_is_named_for_the_time_of_its_restore() {
        let void: DatasetName = "zroot/ROOT/void".parse().unwrap();
        assert_eq!(
            restore_snapshot(&void, 1_793_009_715).map(|name| name.to_string()),
            Some("zroot/ROOT/void@before-restore-2026-10-26-101515".to_owned())
        );
        let long: DatasetName = format!("zroot/{}", "a".repeat(230)).parse().unwrap();
        assert_eq!(restore_snapshot(&long, 0), None);
    }
}
