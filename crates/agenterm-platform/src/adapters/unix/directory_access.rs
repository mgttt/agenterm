use std::path::Path;

use crate::directory_access::{
    DirectoryAccessError, DirectoryAccessErrorKind, DirectoryAccessReport, DirectoryPrincipal,
    DirectoryTreeAccess,
};

pub(crate) fn grant_directory_tree_access(
    root: &Path,
    _principal: DirectoryPrincipal<'_>,
    _access: DirectoryTreeAccess,
) -> Result<DirectoryAccessReport, DirectoryAccessError> {
    Err(DirectoryAccessError::new(
        DirectoryAccessErrorKind::Unsupported,
        "grant-directory-tree-access",
        root,
        None,
        "Windows SID directory authorization is unsupported on this host",
    ))
}
