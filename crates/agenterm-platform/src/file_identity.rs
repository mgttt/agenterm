//! Stable host filesystem object identity without directory policy or mutation APIs.

use std::{io, path::Path};

/// Stable identity of an opened host filesystem object.
///
/// `filesystem_id` plus `object_id` remains stable across rename and hard-link
/// aliases while the object exists. `hard_link_count` is an observation and
/// may change between calls, so use [`Self::same_object`] for identity checks.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FileIdentity {
    pub filesystem_id: u64,
    pub object_id: u64,
    pub hard_link_count: u64,
}

impl FileIdentity {
    #[must_use]
    pub const fn same_object(self, other: Self) -> bool {
        self.filesystem_id == other.filesystem_id && self.object_id == other.object_id
    }
}

/// Query the identity of an already-open host file or directory.
///
/// Prefer this handle-based form when identity guards a subsequent operation:
/// it cannot be redirected by a path replacement between calls.
pub fn file_identity(file: &std::fs::File) -> io::Result<FileIdentity> {
    crate::selected::file_identity::file_identity(file)
}

/// Open a host path and query its object identity.
///
/// The final symbolic link is followed. Callers that need race-free behavior
/// must open the object themselves and use [`file_identity`].
pub fn path_identity(path: &Path) -> io::Result<FileIdentity> {
    crate::selected::file_identity::path_identity(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_survives_hard_link_and_rename_aliases() {
        let root = std::env::temp_dir().join(format!(
            "agenterm-platform-file-identity-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir(&root).expect("create identity fixture");
        path_identity(&root).expect("query directory identity");
        let original = root.join("original");
        let alias = root.join("alias");
        let renamed = root.join("renamed");
        std::fs::write(&original, b"identity").expect("write identity fixture");

        let opened = std::fs::File::open(&original).expect("open identity fixture");
        let from_handle = file_identity(&opened).expect("query opened identity");
        let from_path = path_identity(&original).expect("query path identity");
        assert_eq!(from_handle, from_path);

        std::fs::hard_link(&original, &alias).expect("create hard-link alias");
        let original_after_link = path_identity(&original).expect("query linked original");
        let alias_identity = path_identity(&alias).expect("query hard-link alias");
        assert!(original_after_link.same_object(alias_identity));
        assert!(original_after_link.hard_link_count >= 2);
        assert!(alias_identity.hard_link_count >= 2);

        std::fs::rename(&original, &renamed).expect("rename identity fixture");
        let renamed_identity = path_identity(&renamed).expect("query renamed identity");
        assert!(from_handle.same_object(renamed_identity));
        assert!(
            file_identity(&opened)
                .expect("re-query opened identity")
                .same_object(renamed_identity)
        );

        std::fs::remove_dir_all(root).expect("remove identity fixture");
    }
}
