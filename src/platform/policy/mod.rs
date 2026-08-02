//! Product policy tables kept inside the platform glue boundary.
//!
//! These are stable product decisions (shortcuts, layout, runtime defaults)
//! with no native API implementation. Host adapters consume them through the
//! `crate::platform` re-exports.

pub(crate) mod control_center;
pub(crate) mod input;
