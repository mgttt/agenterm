//! Local wall-clock time for status chrome.
//!
//! The OS boundary for "what is the local time" belongs in this crate rather
//! than in a product adapter: Windows reaches for `GetLocalTime`, and a
//! `#[link(name = "kernel32")]` written inside `src/platform/adapters/windows/**`
//! is compiled on macOS and Linux too (that module is declared
//! unconditionally) and fails the link there.
//!
//! The native read lives in the per-host adapter selected by `selected.rs`;
//! this file only formats. Keeping the `#[cfg]` and the `#[link]` down in the
//! adapters is what `platform::boundary_tests` enforces.

pub use crate::contract::local_clock::LocalCivilTime;

/// Read the host's local civil time.
pub fn local_civil_now() -> LocalCivilTime {
    crate::selected::local_clock::local_civil_now()
}

/// Local time formatted as `HH:MM:SS`.
pub fn local_clock_hms() -> String {
    let now = local_civil_now();
    format!("{:02}:{:02}:{:02}", now.hour, now.minute, now.second)
}

/// Two-line chrome: `YY-MM-DD Ddd` and `HH:MM:SS` (English weekday).
pub fn local_clock_chrome_lines() -> (String, String) {
    let now = local_civil_now();
    let year = now.year.rem_euclid(100);
    let date = format!(
        "{year:02}-{:02}-{:02} {}",
        now.month,
        now.day,
        weekday_short_en(now.weekday)
    );
    let time = format!("{:02}:{:02}:{:02}", now.hour, now.minute, now.second);
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

#[cfg(test)]
mod tests {
    use super::{local_clock_chrome_lines, local_clock_hms, weekday_short_en};

    #[test]
    fn renders_a_fixed_width_clock() {
        let text = local_clock_hms();
        assert_eq!(text.len(), 8);
        assert_eq!(text.as_bytes()[2], b':');
        assert_eq!(text.as_bytes()[5], b':');
    }

    #[test]
    fn chrome_lines_are_fixed_width_and_carry_a_weekday() {
        let (date, time) = local_clock_chrome_lines();
        assert_eq!(date.len(), 12, "YY-MM-DD Ddd");
        assert_eq!(time.len(), 8);
        let weekday = &date[9..];
        assert!(
            ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"].contains(&weekday),
            "unexpected weekday {weekday}"
        );
    }

    #[test]
    fn weekday_index_is_sunday_first_and_out_of_range_is_visible() {
        assert_eq!(weekday_short_en(0), "Sun");
        assert_eq!(weekday_short_en(6), "Sat");
        // A bad index must not silently read as a real day.
        assert_eq!(weekday_short_en(7), "???");
    }
}
