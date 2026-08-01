use std::path::PathBuf;

use crate::filesystem::{FilesystemError, HostDirectories};

pub fn replace_file(
    source: &std::path::Path,
    destination: &std::path::Path,
) -> std::io::Result<()> {
    std::fs::rename(source, destination)
}

pub fn sync_parent(parent: &std::path::Path) -> std::io::Result<()> {
    std::fs::File::open(parent)?.sync_all()
}

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
