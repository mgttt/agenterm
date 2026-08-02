//! Host directory-tree authorization without product-specific policy.

use std::{
    fmt,
    path::{Path, PathBuf},
};

/// A native principal that may receive access to a host directory tree.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub enum DirectoryPrincipal<'a> {
    /// One exact Windows SID in its binary representation.
    WindowsSid(&'a [u8]),
    /// A principal whose native identity is stable and supplied by the OS.
    WellKnown(WellKnownDirectoryPrincipal),
}

/// Product-neutral well-known principals supported by directory authorization.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum WellKnownDirectoryPrincipal {
    /// Windows `ALL APPLICATION PACKAGES` (`S-1-15-2-1`).
    AllApplicationPackages,
}

/// Deliberately bounded access classes for an existing directory tree.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum DirectoryTreeAccess {
    /// Read files and traverse or execute entries.
    ReadExecute,
    /// Read, create, rewrite, rename, and delete contents without changing ACLs or ownership.
    ModifyContents,
}

/// Observable effects of one completed tree authorization operation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DirectoryAccessReport {
    pub entries_updated: u64,
    pub link_like_entries_skipped: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DirectoryAccessErrorKind {
    InvalidInput,
    NotDirectory,
    LinkLikeRoot,
    Unsupported,
    Io,
    NativeFailure,
}

#[derive(Debug)]
pub struct DirectoryAccessError {
    kind: DirectoryAccessErrorKind,
    operation: &'static str,
    path: PathBuf,
    native_code: Option<u32>,
    detail: String,
}

impl DirectoryAccessError {
    pub const fn kind(&self) -> DirectoryAccessErrorKind {
        self.kind
    }
    pub const fn operation(&self) -> &'static str {
        self.operation
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub const fn native_code(&self) -> Option<u32> {
        self.native_code
    }

    pub(crate) fn new(
        kind: DirectoryAccessErrorKind,
        operation: &'static str,
        path: &Path,
        native_code: Option<u32>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            operation,
            path: path.to_owned(),
            native_code,
            detail: detail.into(),
        }
    }
}

impl fmt::Display for DirectoryAccessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} failed for '{}': {}",
            self.operation,
            self.path.display(),
            self.detail
        )
    }
}

impl std::error::Error for DirectoryAccessError {}

/// Merge an allow rule into every ordinary entry in `root` without following links.
///
/// The root itself must be an ordinary directory. Link-like descendants are counted and
/// skipped. Callers must prevent concurrent mutation of the tree while authorization runs.
pub fn grant_directory_tree_access(
    root: &Path,
    principal: DirectoryPrincipal<'_>,
    access: DirectoryTreeAccess,
) -> Result<DirectoryAccessReport, DirectoryAccessError> {
    crate::selected::directory_access::grant_directory_tree_access(root, principal, access)
}

#[cfg(test)]
mod tests {
    #[test]
    fn capability_status_is_independent_from_full_filesystem() {
        assert_eq!(
            crate::capability_status(crate::Capability::DirectoryAccess),
            crate::CapabilityStatus::Available
        );
        #[cfg(not(feature = "filesystem"))]
        assert_eq!(
            crate::capability_status(crate::Capability::Filesystem),
            crate::CapabilityStatus::Unsupported {
                reason: std::borrow::Cow::Borrowed("feature-disabled")
            }
        );
    }
}
