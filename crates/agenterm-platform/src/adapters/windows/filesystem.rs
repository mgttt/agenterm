use std::path::PathBuf;

use crate::filesystem::{FilesystemError, HostDirectories};

pub fn host_directories() -> Result<HostDirectories, FilesystemError> {
    let config = std::env::var_os("APPDATA").map(PathBuf::from);
    let local_data = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    match (config, local_data) {
        (Some(config), Some(local_data)) => Ok(HostDirectories { config, local_data }),
        _ => Err(FilesystemError::Failed {
            code: "host_directory_unavailable",
            message: "APPDATA and LOCALAPPDATA must be available".to_owned(),
        }),
    }
}

pub fn executable_name(base: &str) -> String {
    format!("{base}.exe")
}
