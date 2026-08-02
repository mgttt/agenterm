use std::path::PathBuf;
#[cfg(feature = "filesystem")]
use std::{fs::OpenOptions, os::unix::fs::OpenOptionsExt as _};

use crate::filesystem::{FilesystemError, HostDirectories};

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

pub fn host_directories() -> Result<HostDirectories, FilesystemError> {
    let home = user_home_directory()?;
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
    if error.raw_os_error() == Some(libc::ELOOP) {
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
