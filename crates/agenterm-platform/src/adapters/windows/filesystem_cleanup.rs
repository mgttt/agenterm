//! Windows caller-owned tree cleanup without traversing reparse points.

use std::{fs, io, os::windows::fs::MetadataExt as _, path::Path};

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;

pub(crate) fn remove_tree(path: &Path) -> io::Result<()> {
    let Some(metadata) = metadata_if_present(path)? else {
        return Ok(());
    };
    if is_reparse_point(&metadata) {
        return remove_reparse_point(path, &metadata);
    }
    if !metadata.is_dir() {
        restore_removal_access(path, &metadata)?;
        return remove_file_if_present(path);
    }
    prepare_tree(path, &metadata)?;
    remove_dir_all_if_present(path)
}

fn prepare_tree(path: &Path, metadata: &fs::Metadata) -> io::Result<()> {
    restore_removal_access(path, metadata)?;
    if !metadata.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(path)? {
        let child = entry?.path();
        let Some(metadata) = metadata_if_present(&child)? else {
            continue;
        };
        if !is_reparse_point(&metadata) {
            prepare_tree(&child, &metadata)?;
        }
    }
    Ok(())
}

#[allow(clippy::permissions_set_readonly_false)]
fn restore_removal_access(path: &Path, metadata: &fs::Metadata) -> io::Result<()> {
    let mut permissions = metadata.permissions();
    if permissions.readonly() {
        // This toggles FILE_ATTRIBUTE_READONLY; it does not grant Unix-style
        // world-write permission.
        permissions.set_readonly(false);
        fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

fn remove_reparse_point(path: &Path, metadata: &fs::Metadata) -> io::Result<()> {
    let result = if metadata.is_dir() {
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    };
    not_found_is_success(result)
}

fn metadata_if_present(path: &Path) -> io::Result<Option<fs::Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn remove_file_if_present(path: &Path) -> io::Result<()> {
    not_found_is_success(fs::remove_file(path))
}

fn remove_dir_all_if_present(path: &Path) -> io::Result<()> {
    not_found_is_success(fs::remove_dir_all(path))
}

fn not_found_is_success(result: io::Result<()>) -> io::Result<()> {
    match result {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}
