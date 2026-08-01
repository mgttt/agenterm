//! Logical-byte accounting for caller-selected filesystem trees.

use std::{io, path::Path};

/// Sum logical bytes without traversing host link-like directory entries.
///
/// A missing root contributes zero. Regular directories contribute only their
/// descendants; files, symbolic links, and Windows reparse points contribute
/// their own `symlink_metadata` length. Hard-linked entries are counted once
/// per directory entry. This is neither allocated-byte accounting nor a claim
/// about how many physical bytes deleting the tree would reclaim.
pub fn logical_tree_size(path: &Path) -> io::Result<u64> {
    crate::selected::filesystem_usage::logical_tree_size(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fixture(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "agenterm-platform-usage-{label}-{}",
            std::process::id()
        ))
    }

    #[test]
    fn sums_nested_files_and_accepts_a_missing_root() {
        let root = fixture("normal");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("nested")).expect("create usage fixture");
        fs::write(root.join("a"), b"abc").expect("write usage fixture");
        fs::write(root.join("nested/b"), b"12345").expect("write nested usage fixture");

        assert_eq!(logical_tree_size(&root).expect("measure tree"), 8);
        assert_eq!(
            logical_tree_size(&root.join("missing")).expect("measure missing root"),
            0
        );
        fs::remove_dir_all(root).expect("remove usage fixture");
    }

    #[test]
    fn counts_each_hard_link_directory_entry() {
        let root = fixture("hard-link");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create hard-link fixture");
        let original = root.join("original");
        fs::write(&original, b"value").expect("write hard-link fixture");
        fs::hard_link(&original, root.join("alias")).expect("create hard-link alias");

        assert_eq!(logical_tree_size(&root).expect("measure hard links"), 10);
        fs::remove_dir_all(root).expect("remove hard-link fixture");
    }

    #[cfg(unix)]
    #[test]
    fn does_not_traverse_symbolic_links() {
        use std::os::unix::fs::symlink;

        let root = fixture("unix-link");
        let outside = fixture("unix-link-outside");
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
        fs::create_dir_all(&root).expect("create usage root");
        fs::create_dir_all(&outside).expect("create outside usage fixture");
        fs::write(root.join("local"), b"abc").expect("write local usage file");
        fs::write(outside.join("large"), vec![0_u8; 64 * 1024])
            .expect("write outside usage canary");
        let link = root.join("outside-link");
        symlink(&outside, &link).expect("create outside symlink");
        let link_bytes = fs::symlink_metadata(&link)
            .expect("read link metadata")
            .len();

        assert_eq!(
            logical_tree_size(&root).expect("measure link tree"),
            3 + link_bytes
        );
        fs::remove_dir_all(root).expect("remove usage root");
        fs::remove_dir_all(outside).expect("remove outside usage fixture");
    }

    #[cfg(windows)]
    #[test]
    fn does_not_traverse_directory_reparse_points() {
        let root = fixture("windows-reparse");
        let outside = fixture("windows-reparse-outside");
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
        fs::create_dir_all(&root).expect("create usage root");
        fs::create_dir_all(&outside).expect("create outside usage fixture");
        fs::write(root.join("local"), b"abc").expect("write local usage file");
        fs::write(outside.join("large"), vec![0_u8; 64 * 1024])
            .expect("write outside usage canary");
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
        let junction_bytes = fs::symlink_metadata(&junction)
            .expect("read junction metadata")
            .len();

        assert_eq!(
            logical_tree_size(&root).expect("measure junction tree"),
            3 + junction_bytes
        );
        fs::remove_dir_all(root).expect("remove usage root");
        fs::remove_dir_all(outside).expect("remove outside usage fixture");
    }
}
