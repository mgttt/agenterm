use std::path::PathBuf;
#[cfg(feature = "filesystem")]
use std::{
    fs::OpenOptions,
    os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _},
};

use crate::filesystem::{FilesystemError, HostDirectories};

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
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Err(FilesystemError::Unsupported {
            reason: "home-directory-unavailable",
        });
    };
    let application_support = home.join("Library").join("Application Support");
    Ok(HostDirectories {
        config: application_support.clone(),
        local_data: application_support,
    })
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
