//! Platform-neutral Unicode clipboard facade.
//!
//! This source is intentionally independent of terminal, editor, or product
//! paste policy. Every read receives its maximum retained UTF-8 byte count
//! from the caller.

use std::time::Duration;

use crate::{contract::clipboard::ClipboardResult, selected};

const DEFAULT_OPEN_TIMEOUT: Duration = Duration::from_millis(500);

/// Publish Unicode text through the selected native clipboard adapter.
pub fn set_text(text: &str) -> ClipboardResult<()> {
    set_text_with_timeout(text, DEFAULT_OPEN_TIMEOUT)
}

pub fn set_text_with_timeout(text: &str, open_timeout: Duration) -> ClipboardResult<()> {
    selected::clipboard::set_text(text, open_timeout).map_err(selected::clipboard::map_error)
}

/// Read Unicode text while retaining no more than `max_read_bytes` UTF-8
/// bytes. Exceeding the caller's bound is a typed `Failed` result.
pub fn get_text(max_read_bytes: usize) -> ClipboardResult<String> {
    get_text_with_timeout(max_read_bytes, DEFAULT_OPEN_TIMEOUT)
}

pub fn get_text_with_timeout(
    max_read_bytes: usize,
    open_timeout: Duration,
) -> ClipboardResult<String> {
    selected::clipboard::get_text(max_read_bytes, open_timeout)
        .map_err(selected::clipboard::map_error)
}

/// Probe whether Unicode text is presently available without requiring a full
/// payload read where the selected adapter can provide a cheaper probe.
pub fn has_unicode_text() -> bool {
    selected::clipboard::has_unicode_text()
}
