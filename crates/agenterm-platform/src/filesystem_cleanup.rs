//! Removal of caller-owned, quiescent filesystem trees.

use std::{fs, io, path::Path};

/// Remove a caller-owned tree after restoring the access required for deletion.
///
/// Missing paths are accepted. Traversal does not intentionally follow Unix
/// symbolic links or Windows reparse points. The caller must ensure the tree is
/// quiescent: this helper is not a defense against a concurrent path-replacement
/// attacker and does not decide whether a root is safe to delete.
pub fn remove_tree(path: &Path) -> io::Result<()> {
    let Some(metadata) = metadata_if_present(path)? else {
        return Ok(());
    };
    if is_link_like(&metadata) {
        return remove_link(path, &metadata);
    }
    if !metadata.is_dir() {
        restore_removal_access(path, &metadata)?;
        return remove_file_if_present(path);
    }

    prepare_tree(path, &metadata)?;
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn prepare_tree(path: &Path, metadata: &fs::Metadata) -> io::Result<()> {
    restore_removal_access(path, metadata)?;
    if !metadata.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let child = entry.path();
        let Some(metadata) = metadata_if_present(&child)? else {
            continue;
        };
        if !is_link_like(&metadata) {
            prepare_tree(&child, &metadata)?;
        }
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

#[cfg(unix)]
fn restore_removal_access(path: &Path, metadata: &fs::Metadata) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;

    let required = if metadata.is_dir() { 0o700 } else { 0o600 };
    let mode = metadata.permissions().mode();
    if mode & required != required {
        fs::set_permissions(path, fs::Permissions::from_mode(mode | required))?;
    }
    Ok(())
}

#[cfg(windows)]
#[allow(clippy::permissions_set_readonly_false)]
fn restore_removal_access(path: &Path, metadata: &fs::Metadata) -> io::Result<()> {
    let mut permissions = metadata.permissions();
    if permissions.readonly() {
        // On Windows this toggles FILE_ATTRIBUTE_READONLY; it does not grant
        // Unix-style world-write permission.
        permissions.set_readonly(false);
        fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

#[cfg(unix)]
fn is_link_like(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(windows)]
fn is_link_like(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt as _;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(unix)]
fn remove_link(path: &Path, _metadata: &fs::Metadata) -> io::Result<()> {
    remove_file_if_present(path)
}

#[cfg(windows)]
fn remove_link(path: &Path, metadata: &fs::Metadata) -> io::Result<()> {
    let result = if metadata.is_dir() {
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    };
    match result {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "agenterm-platform-cleanup-{label}-{}",
            std::process::id()
        ))
    }

    #[test]
    fn removes_a_normal_tree_and_accepts_a_missing_path() {
        let root = fixture("normal");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("nested")).expect("create cleanup fixture");
        fs::write(root.join("nested/file"), b"value").expect("write cleanup fixture");

        remove_tree(&root).expect("remove normal tree");
        assert!(!root.exists());
        remove_tree(&root).expect("missing cleanup is idempotent");
    }

    #[cfg(unix)]
    #[test]
    fn restores_owner_access_without_following_symlinks() {
        use std::os::unix::fs::{PermissionsExt as _, symlink};

        let root = fixture("unix-mode");
        let _ = fs::remove_dir_all(&root);
        let locked = root.join("layer/work/work");
        fs::create_dir_all(&locked).expect("create locked directory");
        fs::write(locked.join("index"), b"value").expect("write locked child");
        let outside = root.with_extension("outside");
        let _ = fs::remove_file(&outside);
        fs::write(&outside, b"keep").expect("write outside canary");
        fs::set_permissions(&outside, fs::Permissions::from_mode(0o000))
            .expect("lock outside canary");
        symlink(&outside, root.join("outside-link")).expect("create outside symlink");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000))
            .expect("lock cleanup directory");

        remove_tree(&root).expect("remove mode-000 tree");
        assert!(!root.exists());
        assert_eq!(
            fs::metadata(&outside)
                .expect("outside canary remains")
                .permissions()
                .mode()
                & 0o777,
            0o000
        );
        fs::set_permissions(&outside, fs::Permissions::from_mode(0o600))
            .expect("unlock outside canary");
        fs::remove_file(outside).expect("remove outside canary");
    }

    #[cfg(windows)]
    #[test]
    fn clears_readonly_files_before_removal() {
        let root = fixture("windows-readonly");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("nested")).expect("create readonly fixture");
        let file = root.join("nested/file");
        fs::write(&file, b"value").expect("write readonly fixture");
        let mut permissions = fs::metadata(&file)
            .expect("readonly metadata")
            .permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&file, permissions).expect("mark fixture readonly");

        remove_tree(&root).expect("remove readonly tree");
        assert!(!root.exists());
    }

    #[cfg(windows)]
    #[test]
    fn does_not_traverse_directory_reparse_points() {
        let root = fixture("windows-reparse");
        let outside = fixture("windows-reparse-outside");
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
        fs::create_dir_all(&root).expect("create reparse fixture root");
        fs::create_dir_all(&outside).expect("create reparse fixture outside");
        let canary = outside.join("readonly-canary");
        fs::write(&canary, b"keep").expect("write reparse canary");
        let mut permissions = fs::metadata(&canary)
            .expect("reparse canary metadata")
            .permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&canary, permissions).expect("mark reparse canary readonly");

        let junction = root.join("outside-junction");
        let status = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&junction)
            .arg(&outside)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("run mklink junction fixture");
        assert!(status.success(), "mklink /J fixture failed: {status}");

        remove_tree(&root).expect("remove tree containing junction");
        assert!(!root.exists());
        assert!(outside.is_dir(), "cleanup traversed the junction target");
        assert!(
            fs::metadata(&canary)
                .expect("outside canary remains")
                .permissions()
                .readonly(),
            "cleanup changed permissions through the junction"
        );
        remove_tree(&outside).expect("remove outside fixture");
    }
}
