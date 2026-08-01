//! Lightweight host filesystem entry classification.

use std::fs::{File, Metadata};

/// Product-neutral facts for one already-resolved filesystem object.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FilesystemEntryFacts {
    directory: bool,
    regular_file: bool,
    link_like: bool,
}

impl FilesystemEntryFacts {
    #[must_use]
    pub const fn is_directory(self) -> bool {
        self.directory
    }

    #[must_use]
    pub const fn is_file(self) -> bool {
        self.regular_file
    }

    #[must_use]
    pub const fn is_link_like(self) -> bool {
        self.link_like
    }

    #[must_use]
    pub const fn is_real_directory(self) -> bool {
        self.directory && !self.link_like
    }

    #[must_use]
    pub const fn is_real_file(self) -> bool {
        self.regular_file && !self.link_like
    }
}

/// Classify metadata without resolving another path.
#[must_use]
pub fn metadata_entry_facts(metadata: &Metadata) -> FilesystemEntryFacts {
    FilesystemEntryFacts {
        directory: metadata.is_dir(),
        regular_file: metadata.is_file(),
        link_like: crate::selected::filesystem_entry::metadata_is_link_like(metadata),
    }
}

/// Classify the object referenced by an already-open file handle.
///
/// The caller chooses whether the native open followed the final link. For
/// component-wise traversal, open the link object itself (`O_NOFOLLOW`/an
/// equivalent reparse-point option) before calling this function. No path is
/// reopened here, so classification cannot race a second name resolution.
pub fn opened_file_entry_facts(file: &File) -> std::io::Result<FilesystemEntryFacts> {
    file.metadata()
        .map(|metadata| metadata_entry_facts(&metadata))
}

/// Whether metadata obtained without following the final component describes
/// a host link-like entry.
///
/// Unix classifies symbolic links. Windows classifies every reparse point,
/// including junctions and symbolic links, because generic recursive callers
/// must not assume that any of them is an ordinary directory.
#[must_use]
pub fn metadata_is_link_like(metadata: &Metadata) -> bool {
    metadata_entry_facts(metadata).is_link_like()
}

/// Whether metadata describes an ordinary directory that is safe to treat as
/// a directory entry rather than as a host link-like object.
#[must_use]
pub fn metadata_is_real_directory(metadata: &Metadata) -> bool {
    metadata_entry_facts(metadata).is_real_directory()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf};

    fn fixture(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "agenterm-platform-entry-{label}-{}",
            std::process::id()
        ))
    }

    #[test]
    fn distinguishes_ordinary_files_and_directories() {
        let root = fixture("ordinary");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir(&root).expect("create entry fixture");
        let file = root.join("file");
        fs::write(&file, b"value").expect("write entry fixture");

        let directory = fs::symlink_metadata(&root).expect("directory metadata");
        let file = fs::symlink_metadata(&file).expect("file metadata");
        assert!(metadata_is_real_directory(&directory));
        assert!(!metadata_is_link_like(&directory));
        assert!(!metadata_is_real_directory(&file));
        assert!(!metadata_is_link_like(&file));
        let opened = fs::File::open(root.join("file")).expect("open entry fixture");
        assert!(
            opened_file_entry_facts(&opened)
                .expect("classify opened file")
                .is_real_file()
        );
        fs::remove_dir_all(root).expect("remove entry fixture");
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_link_is_link_like_not_a_real_directory() {
        use std::os::unix::fs::symlink;

        let root = fixture("unix-link");
        let outside = fixture("unix-link-outside");
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
        fs::create_dir(&root).expect("create link fixture");
        fs::create_dir(&outside).expect("create link target");
        let link = root.join("link");
        symlink(&outside, &link).expect("create directory symlink");

        let metadata = fs::symlink_metadata(&link).expect("link metadata");
        assert!(metadata_is_link_like(&metadata));
        assert!(!metadata_is_real_directory(&metadata));
        fs::remove_dir_all(root).expect("remove link fixture");
        fs::remove_dir_all(outside).expect("remove link target");
    }

    #[cfg(windows)]
    #[test]
    fn directory_junction_is_link_like_not_a_real_directory() {
        let root = fixture("windows-junction");
        let outside = fixture("windows-junction-outside");
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
        fs::create_dir(&root).expect("create junction fixture");
        fs::create_dir(&outside).expect("create junction target");
        let junction = root.join("junction");
        let status = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&junction)
            .arg(&outside)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("run mklink junction fixture");
        assert!(status.success(), "mklink /J fixture failed: {status}");

        let metadata = fs::symlink_metadata(&junction).expect("junction metadata");
        assert!(metadata_is_link_like(&metadata));
        assert!(!metadata_is_real_directory(&metadata));

        use std::os::windows::fs::OpenOptionsExt as _;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        let opened = fs::OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
            .open(&junction)
            .expect("open junction itself");
        let opened = opened_file_entry_facts(&opened).expect("classify opened junction");
        assert!(opened.is_link_like());
        assert!(!opened.is_real_directory());
        assert!(!opened.is_real_file());
        fs::remove_dir(&junction).expect("remove junction");
        fs::remove_dir(&root).expect("remove junction fixture");
        fs::remove_dir_all(outside).expect("remove junction target");
    }
}
