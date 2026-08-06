//! linux local civil time.

use crate::contract::local_clock::{LocalCivilTime, civil_from_unix_seconds};
use std::time::{SystemTime, UNIX_EPOCH};

/// Reads the host's local civil time.
///
/// TODO(linux): apply the host's UTC offset. This currently reports UTC, so a
/// user outside UTC sees a wrong wall clock. Windows already reads the real
/// local time via `GetLocalTime`, so the three hosts disagree today.
/// The Linux source is `localtime_r(3)`, which honours `TZ` and the system
/// zoneinfo database including DST.
pub(crate) fn local_civil_now() -> LocalCivilTime {
    let unix_seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64);
    civil_from_unix_seconds(unix_seconds)
}
