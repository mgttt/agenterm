//! Host filesystem conventions without product-specific directory names.

#[cfg(all(test, feature = "file-identity", not(feature = "filesystem")))]
use std::io;
use std::path::{Path, PathBuf};
#[cfg(feature = "filesystem")]
use std::{
    ffi::OsString,
    fs::OpenOptions,
    io::{self, Write as _},
    sync::atomic::{AtomicU64, Ordering},
};

use crate::selected;

#[cfg(feature = "file-identity")]
pub use crate::file_identity::{FileIdentity, file_identity, path_identity};

#[cfg(feature = "filesystem")]
pub use crate::filesystem_entry::metadata_is_link_like;

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum FilesystemError {
    Unsupported { reason: &'static str },
    Failed { code: &'static str, message: String },
}

impl std::fmt::Display for FilesystemError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported { reason } => write!(formatter, "filesystem unsupported: {reason}"),
            Self::Failed { code, message } => {
                write!(formatter, "filesystem failed ({code}): {message}")
            }
        }
    }
}

impl std::error::Error for FilesystemError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostDirectories {
    pub config: PathBuf,
    pub local_data: PathBuf,
}

pub fn host_directories() -> Result<HostDirectories, FilesystemError> {
    selected::filesystem::host_directories()
}

/// Current user's host home directory according to the selected OS convention.
///
/// Windows reads `USERPROFILE`; Linux and macOS read `HOME`. Empty values are
/// rejected, and no cross-OS fallback is attempted.
pub fn user_home_directory() -> Result<PathBuf, FilesystemError> {
    selected::filesystem::user_home_directory()
}

pub(crate) fn home_directory_from_env(
    value: Option<std::ffi::OsString>,
) -> Result<PathBuf, FilesystemError> {
    value
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or(FilesystemError::Unsupported {
            reason: "home-directory-unavailable",
        })
}

#[must_use]
pub fn executable_name(base: &str) -> String {
    selected::filesystem::executable_name(base)
}

#[must_use]
pub fn sibling_executable(current_executable: &Path, base: &str) -> PathBuf {
    current_executable.with_file_name(executable_name(base))
}

/// Lexically normalize a path without resolving filesystem entries.
///
/// Absolute paths never pop past their host root. Relative leading `..`
/// components are preserved, so callers can decide whether escaping their
/// own logical root is valid. This helper deliberately does not canonicalize
/// symlinks or touch the filesystem.
#[must_use]
pub fn lexical_normalize(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    let mut root_components = 0;
    for component in path.components() {
        match component {
            std::path::Component::Prefix(_) | std::path::Component::RootDir => {
                result.push(component.as_os_str());
                root_components += 1;
            }
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if result.components().count() > root_components {
                    result.pop();
                } else if root_components == 0 {
                    result.push(component.as_os_str());
                }
            }
            std::path::Component::Normal(value) => result.push(value),
        }
    }
    result
}

#[cfg(feature = "filesystem")]
pub fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    selected::filesystem_publish::replace_file(source, destination)
}

#[cfg(feature = "filesystem")]
pub fn sync_parent(parent: &Path) -> io::Result<()> {
    selected::filesystem_publish::sync_parent(parent)
}

/// Restrict an existing directory to the current user.
///
/// Unix requests owner-only mode `0700`. Windows replaces inherited access
/// with a protected current-user-only ACL that propagates to child objects.
#[cfg(feature = "filesystem")]
pub fn protect_private_directory(path: &Path) -> io::Result<()> {
    selected::filesystem::protect_private_directory(path)
}

/// Build exclusive-create options for a private state file.
///
/// Unix adapters additionally request mode `0600`; Windows relies on the
/// protected current-user-only ACL of the caller-owned private directory.
#[must_use]
#[cfg(feature = "filesystem")]
pub fn private_create_new_options() -> OpenOptions {
    selected::filesystem::private_create_new_options()
}

/// Atomically publish private bytes inside an already-private directory.
///
/// The caller owns directory creation and protection. Temporary files are
/// created exclusively with owner-only Unix mode or the protected Windows
/// parent ACL, flushed, atomically promoted on the same volume, and removed on
/// failure. An error from the final parent sync may occur after publication.
#[cfg(feature = "filesystem")]
pub fn write_private_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(1);

    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "private atomic target requires a parent directory",
        )
    })?;
    let file_name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "private atomic target requires a file name",
        )
    })?;
    let mut created = None;
    for _ in 0..32 {
        let mut temporary_name = OsString::from(".");
        temporary_name.push(file_name);
        temporary_name.push(format!(
            ".platform-private-{}-{}",
            std::process::id(),
            NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed)
        ));
        let temporary = parent.join(temporary_name);
        match private_create_new_options().open(&temporary) {
            Ok(file) => {
                created = Some((temporary, file));
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    let (temporary, mut file) = created.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "private atomic temporary name attempts exhausted",
        )
    })?;
    let mut cleanup = TemporaryFile::new(temporary.clone());
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    replace_file(&temporary, path)?;
    cleanup.disarm();
    sync_parent(parent)
}

#[cfg(feature = "filesystem")]
struct TemporaryFile {
    path: PathBuf,
    armed: bool,
}

#[cfg(feature = "filesystem")]
impl TemporaryFile {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

#[cfg(feature = "filesystem")]
impl Drop for TemporaryFile {
    fn drop(&mut self) {
        if self.armed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sibling_name_replaces_the_current_filename() {
        let actual = sibling_executable(Path::new("bin/current"), "worker");
        assert_eq!(
            actual.file_stem().and_then(|value| value.to_str()),
            Some("worker")
        );
    }

    #[test]
    fn lexical_normalize_preserves_root_and_relative_parent_semantics() {
        let current = std::env::current_dir().expect("read current directory");
        let mut anchor = PathBuf::new();
        for component in current.components() {
            match component {
                std::path::Component::Prefix(_) | std::path::Component::RootDir => {
                    anchor.push(component.as_os_str())
                }
                _ => break,
            }
        }
        assert_eq!(
            lexical_normalize(&anchor.join("..").join("conventions-root")),
            anchor.join("conventions-root")
        );
        assert_eq!(
            lexical_normalize(Path::new("../conventions-relative")),
            PathBuf::from("../conventions-relative")
        );
    }

    #[test]
    fn home_directory_value_must_be_present_and_non_empty() {
        assert_eq!(
            home_directory_from_env(Some(std::ffi::OsString::from("home")))
                .expect("non-empty home"),
            PathBuf::from("home")
        );
        for value in [None, Some(std::ffi::OsString::new())] {
            assert_eq!(
                home_directory_from_env(value).expect_err("reject absent home"),
                FilesystemError::Unsupported {
                    reason: "home-directory-unavailable"
                }
            );
        }
    }

    #[cfg(feature = "file-identity")]
    #[test]
    fn full_filesystem_keeps_file_identity_compatibility_exports() {
        let _: fn(&std::fs::File) -> io::Result<FileIdentity> = file_identity;
        let _: fn(&Path) -> io::Result<FileIdentity> = path_identity;
    }

    #[cfg(feature = "filesystem")]
    #[test]
    fn private_create_is_exclusive() {
        let path = temp_root().join(format!(
            "agenterm-platform-private-create-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        private_create_new_options()
            .open(&path)
            .expect("first exclusive create");
        assert_eq!(
            private_create_new_options()
                .open(&path)
                .expect_err("second exclusive create")
                .kind(),
            io::ErrorKind::AlreadyExists
        );
        std::fs::remove_file(path).expect("remove private-create fixture");
    }

    #[cfg(feature = "filesystem")]
    #[test]
    fn private_directory_protection_rejects_a_file() {
        let path = temp_root().join(format!(
            "agenterm-platform-private-not-directory-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        std::fs::File::create(&path).expect("create non-directory fixture");
        assert_eq!(
            protect_private_directory(&path)
                .expect_err("private directory protection must reject a file")
                .kind(),
            io::ErrorKind::InvalidInput
        );
        std::fs::remove_file(path).expect("remove non-directory fixture");
    }

    /// Temporary root with symlinked prefixes resolved.
    ///
    /// macOS points TMPDIR at /var/... where /var is a symlink, and private
    /// directory protection refuses symlinked ancestry, so fixtures must start
    /// from the resolved path to test their own behaviour.
    fn temp_root() -> std::path::PathBuf {
        let base = std::env::temp_dir();
        std::fs::canonicalize(&base).unwrap_or(base)
    }

    #[cfg(all(unix, feature = "filesystem"))]
    #[test]
    fn private_directory_protection_rejects_a_symbolic_link() {
        use std::os::unix::fs::PermissionsExt as _;
        use std::os::unix::fs::symlink;

        let root = temp_root().join(format!(
            "agenterm-platform-private-link-{}",
            std::process::id()
        ));
        let target = root.join("target");
        let link = root.join("link");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&target).expect("create symlink target");
        symlink(&target, &link).expect("create private directory symlink");
        let mode_before = std::fs::metadata(&target)
            .expect("target metadata before protection")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(
            protect_private_directory(&link)
                .expect_err("private directory protection must reject a symlink")
                .kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            std::fs::metadata(&target)
                .expect("target metadata")
                .permissions()
                .mode()
                & 0o777,
            mode_before
        );
        std::fs::remove_dir_all(root).expect("remove symlink fixture");
    }

    #[cfg(all(unix, feature = "filesystem"))]
    #[test]
    fn private_directory_protection_rejects_symbolic_link_ancestry() {
        use std::os::unix::fs::PermissionsExt as _;
        use std::os::unix::fs::symlink;

        let root = temp_root().join(format!(
            "agenterm-platform-private-link-ancestry-{}",
            std::process::id()
        ));
        let target = root.join("target");
        let link = root.join("link");
        let nested = link.join("nested");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(target.join("nested")).expect("create ancestry target");
        symlink(&target, &link).expect("create ancestry symlink");
        let mode_before = std::fs::metadata(target.join("nested"))
            .expect("target metadata before protection")
            .permissions()
            .mode()
            & 0o777;

        assert_eq!(
            protect_private_directory(&nested)
                .expect_err("private directory protection must reject symlink ancestry")
                .kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            std::fs::metadata(target.join("nested"))
                .expect("target metadata")
                .permissions()
                .mode()
                & 0o777,
            mode_before
        );
        std::fs::remove_dir_all(root).expect("remove ancestry fixture");
    }

    #[cfg(feature = "filesystem")]
    #[test]
    fn private_atomic_write_replaces_and_leaves_no_temporary() {
        let root = temp_root().join(format!(
            "agenterm-platform-private-atomic-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir(&root).expect("create atomic fixture");
        protect_private_directory(&root).expect("protect atomic fixture");
        let path = root.join("state.json");

        write_private_atomic(&path, b"first").expect("publish first value");
        write_private_atomic(&path, b"second").expect("replace value");
        assert_eq!(std::fs::read(&path).expect("read value"), b"second");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                std::fs::metadata(&path)
                    .expect("private atomic metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        assert_eq!(
            std::fs::read_dir(&root)
                .expect("read fixture")
                .filter_map(Result::ok)
                .count(),
            1,
            "atomic publication left a temporary file"
        );
        std::fs::remove_dir_all(root).expect("remove atomic fixture");
    }

    #[cfg(all(windows, feature = "filesystem"))]
    #[test]
    fn private_atomic_write_tolerates_concurrent_readers() {
        let root = temp_root().join(format!(
            "agenterm-platform-private-atomic-reader-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir(&root).expect("create concurrent atomic fixture");
        protect_private_directory(&root).expect("protect concurrent atomic fixture");
        let path = root.join("state.json");
        write_private_atomic(&path, b"left").expect("publish initial value");

        let barrier = std::sync::Barrier::new(2);
        std::thread::scope(|scope| {
            let writer_barrier = &barrier;
            let writer_path = &path;
            scope.spawn(move || {
                writer_barrier.wait();
                for index in 0..64 {
                    let value: &[u8] = if index % 2 == 0 { b"right" } else { b"left" };
                    write_private_atomic(writer_path, value)
                        .expect("replace while readers open the destination");
                }
            });
            barrier.wait();
            for _ in 0..256 {
                let value = std::fs::read(&path).expect("read complete atomic value");
                assert!(value == b"left" || value == b"right");
            }
        });

        std::fs::remove_dir_all(root).expect("remove concurrent atomic fixture");
    }

    #[cfg(all(unix, feature = "filesystem"))]
    #[test]
    fn private_file_and_directory_use_owner_only_modes() {
        use std::os::unix::fs::PermissionsExt as _;

        let root = temp_root().join(format!(
            "agenterm-platform-private-mode-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir(&root).expect("create mode fixture");
        protect_private_directory(&root).expect("protect directory");
        assert_eq!(
            std::fs::metadata(&root)
                .expect("directory metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        let file = root.join("state");
        private_create_new_options()
            .open(&file)
            .expect("create private file");
        assert_eq!(
            std::fs::metadata(&file)
                .expect("file metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        std::fs::remove_dir_all(root).expect("remove mode fixture");
    }
}
