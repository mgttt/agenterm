//! Host filesystem conventions without product-specific directory names.

use std::path::{Path, PathBuf};
#[cfg(feature = "filesystem")]
use std::{
    fs::{Metadata, OpenOptions},
    io,
};

use crate::selected;

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum FilesystemError {
    Unsupported { reason: &'static str },
    Failed { code: &'static str, message: String },
}

impl std::fmt::Display for FilesystemError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported { reason } => write!(formatter, "filesystem unsupported: {reason}"),
            Self::Failed { code, message } => {
                write!(formatter, "filesystem failed ({code}): {message}")
            }
        }
    }
}

impl std::error::Error for FilesystemError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostDirectories {
    pub config: PathBuf,
    pub local_data: PathBuf,
}

pub fn host_directories() -> Result<HostDirectories, FilesystemError> {
    selected::filesystem::host_directories()
}

#[must_use]
pub fn executable_name(base: &str) -> String {
    selected::filesystem::executable_name(base)
}

#[must_use]
pub fn sibling_executable(current_executable: &Path, base: &str) -> PathBuf {
    current_executable.with_file_name(executable_name(base))
}

#[cfg(feature = "filesystem")]
pub fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    selected::filesystem::replace_file(source, destination)
}

#[cfg(feature = "filesystem")]
pub fn sync_parent(parent: &Path) -> io::Result<()> {
    selected::filesystem::sync_parent(parent)
}

#[cfg(feature = "filesystem")]
pub fn metadata_is_link_like(metadata: &Metadata) -> bool {
    selected::filesystem::metadata_is_link_like(metadata)
}

/// Restrict an existing directory to the current user.
///
/// Unix requests owner-only mode `0700`. Windows replaces inherited access
/// with a protected current-user-only ACL that propagates to child objects.
#[cfg(feature = "filesystem")]
pub fn protect_private_directory(path: &Path) -> io::Result<()> {
    selected::filesystem::protect_private_directory(path)
}

/// Build exclusive-create options for a private state file.
///
/// Unix adapters additionally request mode `0600`; Windows relies on the
/// protected current-user-only ACL of the caller-owned private directory.
#[must_use]
#[cfg(feature = "filesystem")]
pub fn private_create_new_options() -> OpenOptions {
    selected::filesystem::private_create_new_options()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sibling_name_replaces_the_current_filename() {
        let actual = sibling_executable(Path::new("bin/current"), "worker");
        assert_eq!(
            actual.file_stem().and_then(|value| value.to_str()),
            Some("worker")
        );
    }

    #[cfg(feature = "filesystem")]
    #[test]
    fn private_create_is_exclusive() {
        let path = std::env::temp_dir().join(format!(
            "agenterm-platform-private-create-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        private_create_new_options()
            .open(&path)
            .expect("first exclusive create");
        assert_eq!(
            private_create_new_options()
                .open(&path)
                .expect_err("second exclusive create")
                .kind(),
            io::ErrorKind::AlreadyExists
        );
        std::fs::remove_file(path).expect("remove private-create fixture");
    }

    #[cfg(all(unix, feature = "filesystem"))]
    #[test]
    fn private_file_and_directory_use_owner_only_modes() {
        use std::os::unix::fs::PermissionsExt as _;

        let root = std::env::temp_dir().join(format!(
            "agenterm-platform-private-mode-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir(&root).expect("create mode fixture");
        protect_private_directory(&root).expect("protect directory");
        assert_eq!(
            std::fs::metadata(&root)
                .expect("directory metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        let file = root.join("state");
        private_create_new_options()
            .open(&file)
            .expect("create private file");
        assert_eq!(
            std::fs::metadata(&file)
                .expect("file metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        std::fs::remove_dir_all(root).expect("remove mode fixture");
    }
}
