//! Platform-neutral Unicode clipboard facade.
//!
//! This source is intentionally independent of terminal, editor, or product
//! paste policy. Every read receives its maximum retained UTF-8 byte count
//! from the caller.

use crate::{contract::clipboard::ClipboardResult, selected};

/// Publish Unicode text through the selected native clipboard adapter.
pub fn set_text(text: &str) -> ClipboardResult<()> {
    selected::clipboard::set_text(text).map_err(selected::clipboard::map_error)
}

/// Read Unicode text while retaining no more than `max_read_bytes` UTF-8
/// bytes. Exceeding the caller's bound is a typed `Failed` result.
pub fn get_text(max_read_bytes: usize) -> ClipboardResult<String> {
    selected::clipboard::get_text(max_read_bytes).map_err(selected::clipboard::map_error)
}

/// Probe whether Unicode text is presently available without requiring a full
/// payload read where the selected adapter can provide a cheaper probe.
pub fn has_unicode_text() -> bool {
    selected::clipboard::has_unicode_text()
}
