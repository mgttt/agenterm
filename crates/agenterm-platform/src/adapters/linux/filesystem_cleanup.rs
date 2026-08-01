//! Linux restricted tree cleanup adapter.

#[path = "../unix/filesystem_cleanup.rs"]
mod unix;

pub(crate) use unix::remove_tree;
