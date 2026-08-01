//! Host filesystem conventions without product-specific directory names.

use std::{
    fs::Metadata,
    io,
    path::{Path, PathBuf},
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

pub fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    selected::filesystem::replace_file(source, destination)
}

pub fn sync_parent(parent: &Path) -> io::Result<()> {
    selected::filesystem::sync_parent(parent)
}

pub fn metadata_is_link_like(metadata: &Metadata) -> bool {
    selected::filesystem::metadata_is_link_like(metadata)
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
}
