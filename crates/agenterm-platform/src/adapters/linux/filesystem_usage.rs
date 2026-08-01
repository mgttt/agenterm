//! Linux logical tree accounting adapter.

#[path = "../unix/filesystem_usage.rs"]
mod unix;

pub(crate) use unix::logical_tree_size;
