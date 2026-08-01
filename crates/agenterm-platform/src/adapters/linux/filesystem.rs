use std::path::PathBuf;
#[cfg(feature = "filesystem")]
use std::{
    fs::OpenOptions,
    os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _},
};

use crate::filesystem::{FilesystemError, HostDirectories};

#[cfg(feature = "filesystem")]
#[path = "../unix/file_identity.rs"]
mod file_identity;
#[cfg(feature = "filesystem")]
pub use file_identity::{file_identity, path_identity};

pub fn user_home_directory() -> Result<PathBuf, FilesystemError> {
    crate::filesystem::home_directory_from_env(std::env::var_os("HOME"))
}

#[cfg(feature = "filesystem")]
pub fn replace_file(
    source: &std::path::Path,
    destination: &std::path::Path,
) -> std::io::Result<()> {
    std::fs::rename(source, destination)
}

#[cfg(feature = "filesystem")]
pub fn sync_parent(parent: &std::path::Path) -> std::io::Result<()> {
    std::fs::File::open(parent)?.sync_all()
}

#[cfg(feature = "filesystem")]
pub fn metadata_is_link_like(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

pub fn host_directories() -> Result<HostDirectories, FilesystemError> {
    let home = user_home_directory().ok();
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| home.as_ref().map(|path| path.join(".config")));
    let local_data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| home.as_ref().map(|path| path.join(".local").join("share")));
    match (config, local_data) {
        (Some(config), Some(local_data)) => Ok(HostDirectories { config, local_data }),
        _ => Err(FilesystemError::Unsupported {
            reason: "home-directory-unavailable",
        }),
    }
}

pub fn executable_name(base: &str) -> String {
    base.to_owned()
}

#[cfg(feature = "filesystem")]
pub fn protect_private_directory(path: &std::path::Path) -> std::io::Result<()> {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
}

#[cfg(feature = "filesystem")]
pub fn private_create_new_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true).mode(0o600);
    options
}
