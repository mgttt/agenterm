//! Product policy tables kept inside the platform glue boundary.
//!
//! These are stable product decisions (shortcuts, layout, runtime defaults)
//! with no native API implementation. Host adapters consume them through the
//! `crate::platform` re-exports.

pub(crate) mod control_center;
pub(crate) mod input;
pub(crate) mod paths;
pub(crate) mod runtime;
pub(crate) mod test_fixtures;
