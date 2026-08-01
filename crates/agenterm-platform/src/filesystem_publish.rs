//! Recoverable publication of caller-prepared filesystem directories.

use std::{
    fmt, fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static BACKUP_SERIAL: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DirectoryPublishErrorKind {
    InvalidInput,
    Inspect,
    Backup,
    Install,
    InstallRolledBack,
    Rollback,
}

#[derive(Debug)]
pub struct DirectoryPublishError {
    kind: DirectoryPublishErrorKind,
    detail: String,
    retained_backup: Option<PathBuf>,
}

impl DirectoryPublishError {
    pub const fn kind(&self) -> DirectoryPublishErrorKind {
        self.kind
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }

    /// Backup retained after both installation and rollback failed.
    pub fn retained_backup(&self) -> Option<&Path> {
        self.retained_backup.as_deref()
    }

    fn new(kind: DirectoryPublishErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
            retained_backup: None,
        }
    }

    fn with_backup(mut self, path: PathBuf) -> Self {
        self.retained_backup = Some(path);
        self
    }
}

impl fmt::Display for DirectoryPublishError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for DirectoryPublishError {}

#[derive(Debug)]
pub struct DirectoryPublishOutcome {
    replaced_existing: bool,
    retained_backup: Option<PathBuf>,
    cleanup_error: Option<String>,
}

impl DirectoryPublishOutcome {
    pub const fn replaced_existing(&self) -> bool {
        self.replaced_existing
    }

    /// Obsolete backup retained because post-install cleanup failed.
    pub fn retained_backup(&self) -> Option<&Path> {
        self.retained_backup.as_deref()
    }

    pub fn cleanup_error(&self) -> Option<&str> {
        self.cleanup_error.as_deref()
    }
}

/// Publish `staging` at `destination`, restoring an existing destination if
/// installation fails.
///
/// Both paths must name distinct entries in the same existing directory, and
/// `staging` must already be a directory. The caller owns and quiesces both
/// entries and must serialize competing publishers. Each rename is atomic on
/// the host filesystem, but replacement as a whole spans two renames and is
/// neither crash-atomic nor a durability barrier.
pub fn publish_directory(
    staging: &Path,
    destination: &Path,
) -> Result<DirectoryPublishOutcome, DirectoryPublishError> {
    validate(staging, destination)?;
    publish_with(
        staging,
        destination,
        |from, to| fs::rename(from, to),
        crate::filesystem_cleanup::remove_tree,
    )
}

fn validate(staging: &Path, destination: &Path) -> Result<(), DirectoryPublishError> {
    if staging == destination {
        return Err(DirectoryPublishError::new(
            DirectoryPublishErrorKind::InvalidInput,
            "staging and destination must be distinct directory entries",
        ));
    }
    let staging_parent = staging.parent().ok_or_else(|| {
        DirectoryPublishError::new(
            DirectoryPublishErrorKind::InvalidInput,
            "staging requires a parent directory",
        )
    })?;
    let destination_parent = destination.parent().ok_or_else(|| {
        DirectoryPublishError::new(
            DirectoryPublishErrorKind::InvalidInput,
            "destination requires a parent directory",
        )
    })?;
    let staging_parent = fs::canonicalize(staging_parent).map_err(|error| {
        DirectoryPublishError::new(
            DirectoryPublishErrorKind::Inspect,
            format!("inspect staging parent failed: {error}"),
        )
    })?;
    let destination_parent = fs::canonicalize(destination_parent).map_err(|error| {
        DirectoryPublishError::new(
            DirectoryPublishErrorKind::Inspect,
            format!("inspect destination parent failed: {error}"),
        )
    })?;
    if staging_parent != destination_parent {
        return Err(DirectoryPublishError::new(
            DirectoryPublishErrorKind::InvalidInput,
            "staging and destination must share one physical parent directory",
        ));
    }
    let metadata = fs::symlink_metadata(staging).map_err(|error| {
        DirectoryPublishError::new(
            DirectoryPublishErrorKind::Inspect,
            format!("inspect staging directory failed: {error}"),
        )
    })?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(DirectoryPublishError::new(
            DirectoryPublishErrorKind::InvalidInput,
            "staging must be a real directory entry",
        ));
    }
    match fs::symlink_metadata(destination) {
        Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
            return Err(DirectoryPublishError::new(
                DirectoryPublishErrorKind::InvalidInput,
                "an existing destination must be a real directory entry",
            ));
        }
        Ok(_) => {
            let staging_identity = fs::canonicalize(staging).map_err(|error| {
                DirectoryPublishError::new(
                    DirectoryPublishErrorKind::Inspect,
                    format!("resolve staging directory failed: {error}"),
                )
            })?;
            let destination_identity = fs::canonicalize(destination).map_err(|error| {
                DirectoryPublishError::new(
                    DirectoryPublishErrorKind::Inspect,
                    format!("resolve destination directory failed: {error}"),
                )
            })?;
            if staging_identity == destination_identity {
                return Err(DirectoryPublishError::new(
                    DirectoryPublishErrorKind::InvalidInput,
                    "staging and destination resolve to the same directory",
                ));
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(DirectoryPublishError::new(
                DirectoryPublishErrorKind::Inspect,
                format!("inspect destination failed: {error}"),
            ));
        }
    }
    Ok(())
}

fn publish_with<R, C>(
    staging: &Path,
    destination: &Path,
    mut rename: R,
    mut cleanup: C,
) -> Result<DirectoryPublishOutcome, DirectoryPublishError>
where
    R: FnMut(&Path, &Path) -> io::Result<()>,
    C: FnMut(&Path) -> io::Result<()>,
{
    let destination_exists = match fs::symlink_metadata(destination) {
        Ok(_) => true,
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => {
            return Err(DirectoryPublishError::new(
                DirectoryPublishErrorKind::Inspect,
                format!("inspect destination failed: {error}"),
            ));
        }
    };
    if !destination_exists {
        rename(staging, destination).map_err(|error| {
            DirectoryPublishError::new(
                DirectoryPublishErrorKind::Install,
                format!("install prepared directory failed: {error}"),
            )
        })?;
        return Ok(DirectoryPublishOutcome {
            replaced_existing: false,
            retained_backup: None,
            cleanup_error: None,
        });
    }

    let backup = unique_backup(destination)?;
    rename(destination, &backup).map_err(|error| {
        DirectoryPublishError::new(
            DirectoryPublishErrorKind::Backup,
            format!("backup existing directory failed: {error}"),
        )
    })?;
    if let Err(install_error) = rename(staging, destination) {
        if let Err(rollback_error) = rename(&backup, destination) {
            return Err(DirectoryPublishError::new(
                DirectoryPublishErrorKind::Rollback,
                format!(
                    "install prepared directory failed: {install_error}; restore backup failed: {rollback_error}"
                ),
            )
            .with_backup(backup));
        }
        return Err(DirectoryPublishError::new(
            DirectoryPublishErrorKind::InstallRolledBack,
            format!(
                "install prepared directory failed; existing directory restored: {install_error}"
            ),
        ));
    }

    match cleanup(&backup) {
        Ok(()) => Ok(DirectoryPublishOutcome {
            replaced_existing: true,
            retained_backup: None,
            cleanup_error: None,
        }),
        Err(error) => Ok(DirectoryPublishOutcome {
            replaced_existing: true,
            retained_backup: Some(backup),
            cleanup_error: Some(error.to_string()),
        }),
    }
}

fn unique_backup(destination: &Path) -> Result<PathBuf, DirectoryPublishError> {
    let parent = destination.parent().expect("validated destination parent");
    let name = destination
        .file_name()
        .ok_or_else(|| {
            DirectoryPublishError::new(
                DirectoryPublishErrorKind::InvalidInput,
                "destination requires a final path component",
            )
        })?
        .to_string_lossy();
    for _ in 0..1024 {
        let serial = BACKUP_SERIAL.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(
            ".{name}.platform-backup-{}-{serial}",
            std::process::id()
        ));
        match fs::symlink_metadata(&candidate) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(candidate),
            Ok(_) => continue,
            Err(error) => {
                return Err(DirectoryPublishError::new(
                    DirectoryPublishErrorKind::Inspect,
                    format!("inspect backup candidate failed: {error}"),
                ));
            }
        }
    }
    Err(DirectoryPublishError::new(
        DirectoryPublishErrorKind::Backup,
        "could not allocate a unique sibling backup name",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    static FIXTURE_SERIAL: AtomicU64 = AtomicU64::new(0);

    fn fixture(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "agenterm-platform-publish-{label}-{}-{}",
            std::process::id(),
            FIXTURE_SERIAL.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn prepared(root: &Path, value: &[u8]) -> PathBuf {
        fs::create_dir_all(root).expect("create fixture root");
        let staging = root.join("staging");
        fs::create_dir(&staging).expect("create staging");
        fs::write(staging.join("value"), value).expect("write staging value");
        staging
    }

    #[test]
    fn installs_new_and_replaces_existing_directories() {
        let root = fixture("replace");
        let staging = prepared(&root, b"first");
        let destination = root.join("live");
        let first = publish_directory(&staging, &destination).expect("first publish");
        assert!(!first.replaced_existing());
        assert_eq!(fs::read(destination.join("value")).unwrap(), b"first");

        let staging = root.join("staging-2");
        fs::create_dir(&staging).unwrap();
        fs::write(staging.join("value"), b"second").unwrap();
        let second = publish_directory(&staging, &destination).expect("replacement publish");
        assert!(second.replaced_existing());
        assert!(second.retained_backup().is_none());
        assert_eq!(fs::read(destination.join("value")).unwrap(), b"second");
        crate::filesystem_cleanup::remove_tree(&root).unwrap();
    }

    #[test]
    fn install_failure_restores_existing_directory() {
        let root = fixture("rollback");
        let staging = prepared(&root, b"new");
        let destination = root.join("live");
        fs::create_dir(&destination).unwrap();
        fs::write(destination.join("value"), b"old").unwrap();
        let mut calls = 0;
        let error = publish_with(
            &staging,
            &destination,
            |from, to| {
                calls += 1;
                if calls == 2 {
                    Err(io::Error::new(io::ErrorKind::PermissionDenied, "injected"))
                } else {
                    fs::rename(from, to)
                }
            },
            crate::filesystem_cleanup::remove_tree,
        )
        .expect_err("install must fail");
        assert_eq!(
            error.kind(),
            DirectoryPublishErrorKind::InstallRolledBack
        );
        assert!(error.retained_backup().is_none());
        assert_eq!(fs::read(destination.join("value")).unwrap(), b"old");
        crate::filesystem_cleanup::remove_tree(&root).unwrap();
    }

    #[test]
    fn rollback_failure_reports_the_retained_backup() {
        let root = fixture("rollback-failure");
        let staging = prepared(&root, b"new");
        let destination = root.join("live");
        fs::create_dir(&destination).unwrap();
        fs::write(destination.join("value"), b"old").unwrap();
        let mut calls = 0;
        let error = publish_with(
            &staging,
            &destination,
            |from, to| {
                calls += 1;
                match calls {
                    1 => fs::rename(from, to),
                    _ => Err(io::Error::new(io::ErrorKind::PermissionDenied, "injected")),
                }
            },
            crate::filesystem_cleanup::remove_tree,
        )
        .expect_err("install and rollback must fail");
        assert_eq!(error.kind(), DirectoryPublishErrorKind::Rollback);
        assert!(error.retained_backup().is_some());
        crate::filesystem_cleanup::remove_tree(&root).unwrap();
    }

    #[test]
    fn cleanup_failure_is_a_successful_publish_with_a_warning() {
        let root = fixture("cleanup-warning");
        let staging = prepared(&root, b"new");
        let destination = root.join("live");
        fs::create_dir(&destination).unwrap();
        fs::write(destination.join("value"), b"old").unwrap();
        let outcome = publish_with(
            &staging,
            &destination,
            |from, to| fs::rename(from, to),
            |_path| Err(io::Error::new(io::ErrorKind::PermissionDenied, "injected")),
        )
        .expect("installation succeeds");
        assert!(outcome.replaced_existing());
        assert!(outcome.retained_backup().is_some());
        assert_eq!(outcome.cleanup_error(), Some("injected"));
        assert_eq!(fs::read(destination.join("value")).unwrap(), b"new");
        crate::filesystem_cleanup::remove_tree(&root).unwrap();
    }

    #[test]
    fn rejects_different_parents_and_non_directory_staging() {
        let left = fixture("left");
        let right = fixture("right");
        fs::create_dir_all(&left).unwrap();
        fs::create_dir_all(&right).unwrap();
        let staging = left.join("staging");
        fs::write(&staging, b"file").unwrap();
        let non_directory = publish_directory(&staging, &left.join("live")).unwrap_err();
        assert_eq!(
            non_directory.kind(),
            DirectoryPublishErrorKind::InvalidInput
        );
        fs::remove_file(&staging).unwrap();
        fs::create_dir(&staging).unwrap();
        let destination_file = left.join("live-file");
        fs::write(&destination_file, b"file").unwrap();
        let non_directory_destination = publish_directory(&staging, &destination_file).unwrap_err();
        assert_eq!(
            non_directory_destination.kind(),
            DirectoryPublishErrorKind::InvalidInput
        );
        let different_parent = publish_directory(&staging, &right.join("live")).unwrap_err();
        assert_eq!(
            different_parent.kind(),
            DirectoryPublishErrorKind::InvalidInput
        );
        crate::filesystem_cleanup::remove_tree(&left).unwrap();
        crate::filesystem_cleanup::remove_tree(&right).unwrap();
    }
}
