//! Linux console interrupt adapter.

#[path = "../unix/console_interrupt.rs"]
mod unix;

pub(crate) use unix::{IgnoreGuard, Observer};
