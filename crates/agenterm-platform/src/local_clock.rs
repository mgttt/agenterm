//! Local wall-clock time for status chrome.
//!
//! Frontends render a clock in their own chrome, but the OS boundary for
//! "what is the local time" belongs here rather than in an adapter: Windows
//! reaches for `GetLocalTime`, and a `#[link(name = "kernel32")]` written
//! inside `src/platform/adapters/windows/**` would be compiled on macOS and
//! Linux too (that module is declared unconditionally) and fail the link.

use std::time::{SystemTime, UNIX_EPOCH};

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

/// Read the host's local civil time.
pub fn local_civil_now() -> LocalCivilTime {
    #[cfg(target_os = "windows")]
    {
        return windows_local_civil();
    }
    #[cfg(not(target_os = "windows"))]
    {
        // Fallback: UTC civil components until host adapters land.
        let unix_seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs() as i64);
        civil_from_unix_days(unix_seconds)
    }
}

/// Local time formatted as `HH:MM:SS`.
pub fn local_clock_hms() -> String {
    let t = local_civil_now();
    format!("{:02}:{:02}:{:02}", t.hour, t.minute, t.second)
}

/// Two-line chrome: `YY-MM-DD Ddd` and `HH:MM:SS` (English weekday).
pub fn local_clock_chrome_lines() -> (String, String) {
    let t = local_civil_now();
    let yy = t.year.rem_euclid(100);
    let date = format!(
        "{yy:02}-{:02}-{:02} {}",
        t.month,
        t.day,
        weekday_short_en(t.weekday)
    );
    let time = format!("{:02}:{:02}:{:02}", t.hour, t.minute, t.second);
    (date, time)
}

fn weekday_short_en(weekday: u8) -> &'static str {
    match weekday {
        0 => "Sun",
        1 => "Mon",
        2 => "Tue",
        3 => "Wed",
        4 => "Thu",
        5 => "Fri",
        6 => "Sat",
        _ => "???",
    }
}

#[cfg(target_os = "windows")]
fn windows_local_civil() -> LocalCivilTime {
    #[repr(C)]
    struct SystemTime {
        year: u16,
        month: u16,
        day_of_week: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        milliseconds: u16,
    }
    // SAFETY: GetLocalTime only writes the provided SYSTEMTIME buffer.
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetLocalTime(time: *mut SystemTime);
    }
    let mut time = SystemTime {
        year: 0,
        month: 0,
        day_of_week: 0,
        day: 0,
        hour: 0,
        minute: 0,
        second: 0,
        milliseconds: 0,
    };
    unsafe {
        GetLocalTime(&raw mut time);
    }
    LocalCivilTime {
        year: i32::from(time.year),
        month: time.month as u8,
        day: time.day as u8,
        weekday: time.day_of_week as u8,
        hour: time.hour as u8,
        minute: time.minute as u8,
        second: time.second as u8,
    }
}

/// Split a Unix timestamp into civil UTC fields (fallback path / tests).
#[cfg_attr(target_os = "windows", allow(dead_code))]
fn civil_from_unix_days(seconds: i64) -> LocalCivilTime {
    let day_seconds = seconds.rem_euclid(86_400);
    let days = seconds.div_euclid(86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant algorithm).
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
    // 1970-01-01 was Thursday (4). weekday 0=Sun.
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

/// Split a Unix timestamp into `(hour, minute, second)` within its day.
#[cfg_attr(not(test), allow(dead_code))]
fn hms_from_unix_seconds(seconds: i64) -> (u8, u8, u8) {
    let day_seconds = seconds.rem_euclid(86_400);
    (
        (day_seconds / 3_600) as u8,
        ((day_seconds % 3_600) / 60) as u8,
        (day_seconds % 60) as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::{civil_from_unix_days, hms_from_unix_seconds, local_clock_chrome_lines, local_clock_hms};

    #[test]
    fn splits_the_day_at_known_boundaries() {
        assert_eq!(hms_from_unix_seconds(0), (0, 0, 0));
        assert_eq!(hms_from_unix_seconds(86_399), (23, 59, 59));
        assert_eq!(hms_from_unix_seconds(86_400), (0, 0, 0));
        assert_eq!(hms_from_unix_seconds(3_661), (1, 1, 1));
    }

    #[test]
    fn pre_epoch_timestamps_stay_inside_the_day() {
        assert_eq!(hms_from_unix_seconds(-1), (23, 59, 59));
    }

    #[test]
    fn renders_a_fixed_width_clock() {
        let text = local_clock_hms();
        assert_eq!(text.len(), 8);
        assert_eq!(text.as_bytes()[2], b':');
        assert_eq!(text.as_bytes()[5], b':');
        assert!(text.chars().filter(char::is_ascii_digit).count() == 6);
    }

    #[test]
    fn chrome_lines_have_date_weekday_and_time() {
        let (date, time) = local_clock_chrome_lines();
        assert!(date.len() >= 12, "{date}");
        assert_eq!(time.len(), 8);
        // YY-MM-DD
        assert_eq!(date.as_bytes()[2], b'-');
        assert_eq!(date.as_bytes()[5], b'-');
        let weekday = date.split_whitespace().nth(1).unwrap_or("");
        assert_eq!(weekday.len(), 3);
    }

    #[test]
    fn epoch_day_is_thursday() {
        let civil = civil_from_unix_days(0);
        assert_eq!(civil.year, 1970);
        assert_eq!(civil.month, 1);
        assert_eq!(civil.day, 1);
        assert_eq!(civil.weekday, 4); // Thursday
    }
}
