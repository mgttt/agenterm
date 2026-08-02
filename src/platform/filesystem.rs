use std::path::Path;

pub use agenterm_platform::filesystem::{
    executable_name, metadata_is_link_like, replace_file, sync_parent,
};

pub fn is_direct_file(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && !metadata_is_link_like(&metadata))
}

pub fn is_direct_directory(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.is_dir() && !metadata_is_link_like(&metadata))
}
