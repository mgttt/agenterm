//! OS-neutral local civil time contract.

/// Civil local components used by product chrome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalCivilTime {
    pub year: i32,
    pub month: u8,
    pub day: u8,
    /// 0 = Sunday … 6 = Saturday (matches Win32 `SYSTEMTIME.wDayOfWeek`).
    pub weekday: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

/// Split a Unix timestamp into civil UTC fields.
///
/// Shared by every adapter that has no native call yet, and the reference the
/// native paths are checked against. Uses Howard Hinnant's civil-from-days
/// algorithm, with Euclidean division so pre-1970 timestamps stay in range
/// instead of producing negative components.
pub fn civil_from_unix_seconds(seconds: i64) -> LocalCivilTime {
    let day_seconds = seconds.rem_euclid(86_400);
    let days = seconds.div_euclid(86_400);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    // 1970-01-01 was a Thursday, which is weekday 4 when 0 = Sunday.
    let weekday = ((days + 4).rem_euclid(7)) as u8;
    LocalCivilTime {
        year: y as i32,
        month: m as u8,
        day: d as u8,
        weekday,
        hour: (day_seconds / 3_600) as u8,
        minute: ((day_seconds % 3_600) / 60) as u8,
        second: (day_seconds % 60) as u8,
    }
}

#[cfg(test)]
mod tests {
    use super::civil_from_unix_seconds;

    #[test]
    fn epoch_is_a_thursday_at_midnight() {
        let civil = civil_from_unix_seconds(0);
        assert_eq!((civil.year, civil.month, civil.day), (1970, 1, 1));
        assert_eq!(civil.weekday, 4);
        assert_eq!((civil.hour, civil.minute, civil.second), (0, 0, 0));
    }

    #[test]
    fn leap_day_resolves_and_does_not_slide_into_march() {
        // 2024-02-29T12:34:56Z
        let civil = civil_from_unix_seconds(1_709_210_096);
        assert_eq!((civil.year, civil.month, civil.day), (2024, 2, 29));
        assert_eq!((civil.hour, civil.minute, civil.second), (12, 34, 56));
    }

    #[test]
    fn pre_epoch_timestamps_stay_inside_the_day() {
        // Truncating division would report an hour of 0 and a second of -1.
        let civil = civil_from_unix_seconds(-1);
        assert_eq!((civil.year, civil.month, civil.day), (1969, 12, 31));
        assert_eq!((civil.hour, civil.minute, civil.second), (23, 59, 59));
    }
}
