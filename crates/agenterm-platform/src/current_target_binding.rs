//! Enrollment and inspection of an opaque current provider/session binding.
//!
//! The digests are equality identifiers, not authenticators or MACs. The install
//! key and native facts never leave this facade.

use std::{
    fs,
    io::{self, Read as _, Write as _},
    path::{Path, PathBuf},
};

use sha2::{Digest as _, Sha256};

pub use crate::contract::current_target_binding::{
    CURRENT_TARGET_BINDING_VERSION, CURRENT_TARGET_ID_BYTES, CurrentTargetBinding,
    CurrentTargetBindingError, CurrentTargetBindingErrorKind, ProviderIdentity, SessionIdentity,
};
use crate::{
    CapabilityStatus,
    filesystem::{metadata_is_link_like, private_create_new_options, protect_private_directory},
    locking::{LockErrorKind, PathLock},
    selected,
};

const KEY_BYTES: usize = 32;
const KEY_FILE_NAME: &str = ".current-target-binding-key-v1";
const LOCK_FILE_NAME: &str = ".current-target-binding-key-v1.lock";
const PROVIDER_DOMAIN: &[u8] = b"agenterm-platform/current-target-provider/v1\0";
const SESSION_DOMAIN: &[u8] = b"agenterm-platform/current-target-session/v1\0";

#[must_use]
pub fn capability_status() -> CapabilityStatus {
    selected::current_target_binding::capability_status()
}

/// Enroll this caller-owned private state directory, or load its existing key.
///
/// This is the only operation that creates the install key. Query/load never
/// rotate a missing or corrupt key.
pub fn enroll_installation(
    private_state_dir: &Path,
) -> Result<ProviderIdentity, CurrentTargetBindingError> {
    prepare_directory(private_state_dir)?;
    let _lock = acquire_lock(private_state_dir)?;
    match load_key(private_state_dir) {
        Ok(key) => Ok(provider_identity(&key)),
        Err(failure) if failure.kind() == CurrentTargetBindingErrorKind::Missing => {
            let key = crate::entropy::secure_random_array::<KEY_BYTES>().map_err(|_| {
                error(
                    CurrentTargetBindingErrorKind::Entropy,
                    "install-key-entropy",
                    "secure installation key generation failed",
                )
            })?;
            create_key(private_state_dir, &key)?;
            let published = load_key(private_state_dir)?;
            if published != key {
                return Err(error(
                    CurrentTargetBindingErrorKind::Corrupt,
                    "install-key-publish-mismatch",
                    "published installation key did not match enrollment",
                ));
            }
            Ok(provider_identity(&published))
        }
        Err(failure) => Err(failure),
    }
}

/// Load the enrolled opaque provider identity without creating state.
pub fn load_provider_identity(
    private_state_dir: &Path,
) -> Result<ProviderIdentity, CurrentTargetBindingError> {
    prepare_directory(private_state_dir)?;
    let _lock = acquire_lock(private_state_dir)?;
    load_key(private_state_dir).map(|key| provider_identity(&key))
}

/// Inspect the current provider installation and exact interactive desktop session.
pub fn query(private_state_dir: &Path) -> Result<CurrentTargetBinding, CurrentTargetBindingError> {
    prepare_directory(private_state_dir)?;
    let _lock = acquire_lock(private_state_dir)?;
    let key = load_key(private_state_dir)?;
    let facts = selected::current_target_binding::current_session_facts()?;
    Ok(CurrentTargetBinding::new(
        provider_identity(&key),
        session_identity(&key, facts.as_bytes()),
    ))
}

fn prepare_directory(path: &Path) -> Result<(), CurrentTargetBindingError> {
    protect_private_directory(path).map_err(map_state_io)
}

fn acquire_lock(path: &Path) -> Result<PathLock, CurrentTargetBindingError> {
    PathLock::try_acquire(&path.join(LOCK_FILE_NAME)).map_err(|failure| {
        let (kind, code) = match failure.kind() {
            LockErrorKind::Contended => (
                CurrentTargetBindingErrorKind::Contended,
                "install-key-contended",
            ),
            LockErrorKind::InvalidInput => (
                CurrentTargetBindingErrorKind::Corrupt,
                "install-key-lock-invalid",
            ),
            LockErrorKind::Open | LockErrorKind::Wait => (
                CurrentTargetBindingErrorKind::Permission,
                "install-key-lock-failed",
            ),
        };
        error(kind, code, "installation key lock could not be acquired")
    })
}

fn key_path(directory: &Path) -> PathBuf {
    directory.join(KEY_FILE_NAME)
}

fn load_key(directory: &Path) -> Result<[u8; KEY_BYTES], CurrentTargetBindingError> {
    let path = key_path(directory);
    let metadata = fs::symlink_metadata(&path).map_err(map_key_io)?;
    if metadata_is_link_like(&metadata) || !metadata.is_file() {
        return Err(error(
            CurrentTargetBindingErrorKind::Corrupt,
            "install-key-not-regular",
            "installation key is not a regular non-link file",
        ));
    }
    validate_private_key_metadata(&metadata)?;
    selected::current_target_binding::validate_private_key_file(&path)?;
    let mut file = fs::File::open(&path).map_err(map_key_io)?;
    let mut key = [0_u8; KEY_BYTES];
    file.read_exact(&mut key).map_err(|failure| {
        if failure.kind() == io::ErrorKind::UnexpectedEof {
            error(
                CurrentTargetBindingErrorKind::Corrupt,
                "install-key-size",
                "installation key has the wrong size",
            )
        } else {
            map_key_io(failure)
        }
    })?;
    let mut trailing = [0_u8; 1];
    if file.read(&mut trailing).map_err(map_key_io)? != 0 {
        return Err(error(
            CurrentTargetBindingErrorKind::Corrupt,
            "install-key-size",
            "installation key has the wrong size",
        ));
    }
    Ok(key)
}

fn create_key(directory: &Path, key: &[u8; KEY_BYTES]) -> Result<(), CurrentTargetBindingError> {
    let path = key_path(directory);
    let mut pending = PendingKey::new(path.clone());
    let mut file = private_create_new_options()
        .open(&path)
        .map_err(map_key_io)?;
    file.write_all(key).map_err(map_key_io)?;
    file.sync_all().map_err(map_key_io)?;
    drop(file);
    crate::filesystem::sync_parent(directory).map_err(map_key_io)?;
    pending.disarm();
    Ok(())
}

#[cfg(unix)]
fn validate_private_key_metadata(metadata: &fs::Metadata) -> Result<(), CurrentTargetBindingError> {
    use std::os::unix::fs::MetadataExt as _;

    let current = crate::user_identity::current_user_identity().map_err(map_state_io)?;
    let credentials = current.posix_credentials().ok_or_else(|| {
        error(
            CurrentTargetBindingErrorKind::Native,
            "install-key-owner-unavailable",
            "current key owner could not be determined",
        )
    })?;
    if metadata.uid() != credentials.effective_user_id
        || metadata.mode() & 0o777 != 0o600
        || metadata.nlink() != 1
    {
        return Err(error(
            CurrentTargetBindingErrorKind::Permission,
            "install-key-permissions",
            "installation key ownership, permissions, or link count are unsafe",
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn validate_private_key_metadata(_: &fs::Metadata) -> Result<(), CurrentTargetBindingError> {
    // `prepare_directory` installs a protected current-user-only inheritable ACL.
    Ok(())
}

fn provider_identity(key: &[u8; KEY_BYTES]) -> ProviderIdentity {
    ProviderIdentity::new(hash_parts(PROVIDER_DOMAIN, &[key]))
}

fn session_identity(key: &[u8; KEY_BYTES], facts: &[u8]) -> SessionIdentity {
    SessionIdentity::new(hash_parts(SESSION_DOMAIN, &[key, facts]))
}

fn hash_parts(domain: &[u8], parts: &[&[u8]]) -> [u8; CURRENT_TARGET_ID_BYTES] {
    let mut digest = Sha256::new();
    digest.update(domain);
    for part in parts {
        digest.update((part.len() as u64).to_le_bytes());
        digest.update(part);
    }
    digest.finalize().into()
}

fn map_state_io(failure: io::Error) -> CurrentTargetBindingError {
    let kind = if failure.kind() == io::ErrorKind::PermissionDenied {
        CurrentTargetBindingErrorKind::Permission
    } else {
        CurrentTargetBindingErrorKind::Native
    };
    error(
        kind,
        "private-state-invalid",
        "private state directory is unavailable",
    )
}

fn map_key_io(failure: io::Error) -> CurrentTargetBindingError {
    let (kind, code) = match failure.kind() {
        io::ErrorKind::NotFound => (
            CurrentTargetBindingErrorKind::Missing,
            "install-key-missing",
        ),
        io::ErrorKind::PermissionDenied => (
            CurrentTargetBindingErrorKind::Permission,
            "install-key-permission",
        ),
        io::ErrorKind::AlreadyExists => (
            CurrentTargetBindingErrorKind::Contended,
            "install-key-already-exists",
        ),
        _ => (CurrentTargetBindingErrorKind::Native, "install-key-io"),
    };
    error(kind, code, "installation key operation failed")
}

fn error(
    kind: CurrentTargetBindingErrorKind,
    code: &'static str,
    message: &'static str,
) -> CurrentTargetBindingError {
    CurrentTargetBindingError::new(kind, code, message)
}

struct PendingKey {
    path: PathBuf,
    armed: bool,
}

impl PendingKey {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for PendingKey {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "agenterm-platform-current-target-{}-{}",
                std::process::id(),
                NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).expect("create current target fixture");
            Self(path)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn domains_and_session_facts_are_distinct() {
        let key = [7_u8; KEY_BYTES];
        let provider = provider_identity(&key);
        let session = session_identity(&key, b"facts-a");
        assert_ne!(provider.as_bytes(), session.as_bytes());
        assert_ne!(session, session_identity(&key, b"facts-b"));
        assert_eq!(provider, provider_identity(&key));
        assert_eq!(format!("{provider:?}"), "ProviderIdentity(<opaque>)");
    }

    #[test]
    fn load_never_enrolls_a_missing_installation() {
        let fixture = Fixture::new();
        let failure = load_provider_identity(&fixture.0).expect_err("missing key must fail");
        assert_eq!(failure.kind(), CurrentTargetBindingErrorKind::Missing);
        assert!(!key_path(&fixture.0).exists());
    }

    #[test]
    fn enrollment_is_idempotent_and_persists_exactly_32_bytes() {
        let fixture = Fixture::new();
        let first = enroll_installation(&fixture.0).expect("enroll installation");
        let second = enroll_installation(&fixture.0).expect("load enrolled installation");
        assert_eq!(first, second);
        assert_eq!(
            fs::metadata(key_path(&fixture.0))
                .expect("key metadata")
                .len(),
            KEY_BYTES as u64
        );
        assert_eq!(
            load_provider_identity(&fixture.0).expect("load provider identity"),
            first
        );
    }

    #[test]
    fn malformed_key_is_rejected_without_rotation() {
        let fixture = Fixture::new();
        protect_private_directory(&fixture.0).expect("protect corrupt fixture");
        let path = key_path(&fixture.0);
        let mut file = private_create_new_options()
            .open(&path)
            .expect("create corrupt key");
        file.write_all(&[3_u8; KEY_BYTES - 1])
            .expect("write corrupt key");
        drop(file);

        let failure = enroll_installation(&fixture.0).expect_err("corrupt key must fail");
        assert_eq!(failure.kind(), CurrentTargetBindingErrorKind::Corrupt);
        assert_eq!(fs::metadata(path).expect("corrupt metadata").len(), 31);
    }
}
