use std::path::PathBuf;
#[cfg(feature = "filesystem")]
use std::{fs::OpenOptions, os::unix::fs::OpenOptionsExt as _};

use crate::filesystem::{FilesystemError, HostDirectories};

pub fn user_home_directory() -> Result<PathBuf, FilesystemError> {
    crate::filesystem::home_directory_from_env(std::env::var_os("HOME"))
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
    let metadata = std::fs::symlink_metadata(path)?;
    if !crate::filesystem_entry::metadata_is_real_directory(&metadata) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "private directory must be an existing real directory",
        ));
    }
    // Keep every ancestor open while traversing.  O_NOFOLLOW on only the
    // final path component does not protect against a replaced symlink in an
    // intermediate directory.
    let directory = crate::filesystem_open::open_existing_path(
        path,
        crate::filesystem_open::ExistingEntryType::Directory,
    )
    .map_err(map_link_like_error)?;
    let facts = crate::filesystem_entry::opened_file_entry_facts(&directory)?;
    if !facts.is_real_directory() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "private directory must be an existing real directory",
        ));
    }
    use std::os::fd::AsRawFd as _;
    if unsafe { libc::fchmod(directory.as_raw_fd(), 0o700) } != 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(feature = "filesystem")]
fn map_link_like_error(error: std::io::Error) -> std::io::Error {
    // O_NOFOLLOW reports a refused symlink as ELOOP for the final component
    // and as ENOTDIR when an intermediate component is the link, so both
    // describe the same rejected ancestry.
    if matches!(
        error.raw_os_error(),
        Some(libc::ELOOP) | Some(libc::ENOTDIR)
    ) {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "private directory path contains a symbolic link",
        )
    } else {
        error
    }
}

#[cfg(feature = "filesystem")]
pub fn private_create_new_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true).mode(0o600);
    options
}
