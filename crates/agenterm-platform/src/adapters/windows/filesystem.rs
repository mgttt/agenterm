#[cfg(feature = "filesystem")]
use std::fs::OpenOptions;
use std::path::PathBuf;

use crate::filesystem::{FilesystemError, HostDirectories};

#[cfg(feature = "filesystem")]
pub fn replace_file(
    source: &std::path::Path,
    destination: &std::path::Path,
) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt as _;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    let source = std::fs::canonicalize(source)?;
    let destination = std::fs::canonicalize(
        destination
            .parent()
            .ok_or_else(|| std::io::Error::other("destination parent required"))?,
    )?
    .join(
        destination
            .file_name()
            .ok_or_else(|| std::io::Error::other("destination name required"))?,
    );
    let source = source
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    if unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(feature = "filesystem")]
pub fn sync_parent(_parent: &std::path::Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(feature = "filesystem")]
pub fn metadata_is_link_like(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt as _;
    metadata.file_attributes() & 0x0000_0400 != 0
}

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

#[cfg(feature = "filesystem")]
pub fn protect_private_directory(_path: &std::path::Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(feature = "filesystem")]
pub fn private_create_new_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    options
}
