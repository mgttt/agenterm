//! Removal of caller-owned, quiescent filesystem trees.

use std::{io, path::Path};

/// Remove a caller-owned tree after restoring the access required for deletion.
///
/// Missing paths are accepted. Traversal does not intentionally follow Unix
/// symbolic links or Windows reparse points. The caller must ensure the tree is
/// quiescent: this helper is not a defense against a concurrent path-replacement
/// attacker and does not decide whether a root is safe to delete.
pub fn remove_tree(path: &Path) -> io::Result<()> {
    crate::selected::filesystem_cleanup::remove_tree(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

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
        let create_junction = || {
            let status = std::process::Command::new("cmd.exe")
                .args(["/d", "/c", "mklink", "/J"])
                .arg(&junction)
                .arg(&outside)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .expect("run mklink junction fixture");
            assert!(status.success(), "mklink /J fixture failed: {status}");
        };
        create_junction();

        remove_tree(&junction).expect("remove junction as cleanup root");
        assert!(!junction.exists());
        assert!(
            outside.is_dir(),
            "root cleanup traversed the junction target"
        );
        create_junction();

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
