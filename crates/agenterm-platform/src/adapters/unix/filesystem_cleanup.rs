//! Unix caller-owned tree cleanup without following symbolic links.

use std::{fs, io, os::unix::fs::PermissionsExt as _, path::Path};

pub(crate) fn remove_tree(path: &Path) -> io::Result<()> {
    let Some(metadata) = metadata_if_present(path)? else {
        return Ok(());
    };
    if metadata.file_type().is_symlink() {
        return remove_file_if_present(path);
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
        if !metadata.file_type().is_symlink() {
            prepare_tree(&child, &metadata)?;
        }
    }
    Ok(())
}

fn restore_removal_access(path: &Path, metadata: &fs::Metadata) -> io::Result<()> {
    let required = if metadata.is_dir() { 0o700 } else { 0o600 };
    let mode = metadata.permissions().mode();
    if mode & required != required {
        fs::set_permissions(path, fs::Permissions::from_mode(mode | required))?;
    }
    Ok(())
}

fn metadata_if_present(path: &Path) -> io::Result<Option<fs::Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn remove_file_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn remove_dir_all_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}
