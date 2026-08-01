//! Capacity and allocation geometry for a path's backing volume.

pub use crate::contract::storage::{StorageError, StorageErrorKind, VolumeSpace};

pub fn volume_space(path: &std::path::Path) -> Result<VolumeSpace, StorageError> {
    let canonical = std::fs::canonicalize(path).map_err(|source| {
        StorageError::new(
            StorageErrorKind::Path,
            format!("{}: {source}", path.display()),
        )
    })?;
    let metadata = std::fs::metadata(&canonical).map_err(|source| {
        StorageError::new(
            StorageErrorKind::Path,
            format!("{}: {source}", canonical.display()),
        )
    })?;
    if !metadata.is_dir() {
        return Err(StorageError::new(
            StorageErrorKind::Path,
            format!("{} is not a directory", canonical.display()),
        ));
    }
    crate::selected::storage::volume_space(&canonical)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_space_for_the_process_temp_volume() {
        let space = volume_space(&std::env::temp_dir()).expect("query temp volume");
        assert!(space.available_bytes <= space.total_bytes.get());
        assert!(space.allocation_unit.get() <= space.total_bytes.get());
    }

    #[test]
    fn rejects_a_file_as_a_volume_directory() {
        let file = std::env::current_exe().expect("current executable");
        let error = volume_space(&file).expect_err("file is not a directory");
        assert_eq!(error.kind(), StorageErrorKind::Path);
    }
}
