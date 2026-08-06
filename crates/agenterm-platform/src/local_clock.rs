//! Local wall-clock time for status chrome.
//!
//! Frontends render a clock in their own chrome, but the OS boundary for
//! "what is the local time" belongs here rather than in an adapter: Windows
//! reaches for `GetLocalTime`, and a `#[link(name = "kernel32")]` written
//! inside `src/platform/adapters/windows/**` would be compiled on macOS and
//! Linux too (that module is declared unconditionally) and fail the link.
//!
//! The conversion below is deliberately host-neutral. It reads the system
//! clock through `std` and applies the host's UTC offset, so all three hosts
//! share one implementation and one set of tests.

use std::time::{SystemTime, UNIX_EPOCH};

/// Seconds to add to UTC to reach local time on this host.
///
/// TODO(windows): read the real offset via `GetTimeZoneInformation`.
/// TODO(macos): read the real offset via `CFTimeZoneGetSecondsFromGMT`.
/// TODO(linux): read the real offset from `localtime(3)` / `TZ`.
/// Until then every host renders UTC, which is wrong for a user-visible clock
/// but at least identical across platforms rather than silently divergent.
fn utc_offset_seconds() -> i64 {
    0
}

/// Local time formatted as `HH:MM:SS`.
pub fn local_clock_hms() -> String {
    let unix_seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64);
    let (hour, minute, second) = hms_from_unix_seconds(unix_seconds + utc_offset_seconds());
    format!("{hour:02}:{minute:02}:{second:02}")
}

/// Split a Unix timestamp into `(hour, minute, second)` within its day.
///
/// Uses Euclidean remainder so timestamps before 1970 still land inside the
/// day rather than producing negative components.
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
    use super::{hms_from_unix_seconds, local_clock_hms};

    #[test]
    fn splits_the_day_at_known_boundaries() {
        assert_eq!(hms_from_unix_seconds(0), (0, 0, 0));
        assert_eq!(hms_from_unix_seconds(86_399), (23, 59, 59));
        // A new day wraps back to midnight rather than reaching hour 24.
        assert_eq!(hms_from_unix_seconds(86_400), (0, 0, 0));
        assert_eq!(hms_from_unix_seconds(3_661), (1, 1, 1));
    }

    #[test]
    fn pre_epoch_timestamps_stay_inside_the_day() {
        // Truncating division would give (0, 0, -1) here; Euclidean does not.
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
}
