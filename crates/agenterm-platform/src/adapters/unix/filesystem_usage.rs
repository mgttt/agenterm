//! Unix logical tree accounting without following symbolic links.

use std::{fs, io, path::Path};

pub(crate) fn logical_tree_size(path: &Path) -> io::Result<u64> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Ok(metadata.len());
    }

    let mut total = 0_u64;
    for entry in fs::read_dir(path)? {
        total = total
            .checked_add(logical_tree_size(&entry?.path())?)
            .ok_or_else(|| io::Error::other("logical directory size exceeds u64"))?;
    }
    Ok(total)
}
