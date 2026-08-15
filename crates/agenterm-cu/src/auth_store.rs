//! Persistent, bounded computer-use grants.
//!
//! Mutations use an explicit generation comparison and publish a fully
//! validated document from a same-directory temporary file. Cooperating
//! writers take a non-blocking cross-process lock on a stable sibling sidecar,
//! then re-read the generation while holding that lock before publication.
//! The sidecar is never renamed or removed: locking the JSON file itself would
//! be incorrect on Unix because replacement changes the locked inode.
//! Publication uses one same-directory replacement rename. On the pinned
//! Windows toolchain `std::fs::rename` maps to replace-existing semantics;
//! adding a destination-to-backup fallback would introduce a crash window.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{self, Write as _},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};

use agenterm_platform::locking::{LockErrorKind, PathLock};

use crate::{
    auth::Grant,
    target_binding::{TARGET_BINDING_VERSION, TargetBinding},
};

const SCHEMA_VERSION: u32 = 2;
const MAX_RECORDS: usize = 4_096;
const MAX_TEXT_BYTES: usize = 512;
const MAX_OBSERVE_USES: u64 = 10_000;
const MAX_ACTUATE_USES: u64 = 100;
const MAX_OBSERVE_LIFETIME_MS: i64 = 24 * 60 * 60 * 1_000;
const MAX_ACTUATE_LIFETIME_MS: i64 = 60 * 60 * 1_000;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GrantRecord {
    #[serde(default)]
    pub binding_version: u16,
    pub grant_id: String,
    pub target_id: String,
    pub tier: String,
    pub session_binding: Option<String>,
    pub scopes: BTreeSet<Grant>,
    pub issued_at_utc_ms: i64,
    pub not_before_utc_ms: i64,
    pub expires_at_utc_ms: i64,
    pub max_uses: u64,
    pub consumed_uses: u64,
    pub revocation_epoch: u64,
    pub revoked_at_utc_ms: Option<i64>,
    pub one_shot: bool,
    pub session_bound: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GrantSpec {
    binding_version: u16,
    grant_id: String,
    target_id: String,
    tier: String,
    session_binding: Option<String>,
    scopes: BTreeSet<Grant>,
    issued_at_utc_ms: i64,
    not_before_utc_ms: i64,
    expires_at_utc_ms: i64,
    max_uses: u64,
    one_shot: bool,
    session_bound: bool,
}

impl GrantSpec {
    pub fn new(
        grant_id: impl Into<String>,
        binding: &TargetBinding,
        scopes: BTreeSet<Grant>,
        issued_at_utc_ms: i64,
        not_before_utc_ms: i64,
        expires_at_utc_ms: i64,
        max_uses: u64,
    ) -> Self {
        Self {
            binding_version: TARGET_BINDING_VERSION,
            grant_id: grant_id.into(),
            target_id: binding.target_id().to_owned(),
            tier: binding.tier().as_str().to_owned(),
            session_binding: Some(binding.session_binding().to_owned()),
            scopes,
            issued_at_utc_ms,
            not_before_utc_ms,
            expires_at_utc_ms,
            max_uses,
            one_shot: max_uses == 1,
            session_bound: true,
        }
    }
}

impl From<GrantSpec> for GrantRecord {
    fn from(spec: GrantSpec) -> Self {
        Self {
            binding_version: spec.binding_version,
            grant_id: spec.grant_id,
            target_id: spec.target_id,
            tier: spec.tier,
            session_binding: spec.session_binding,
            scopes: spec.scopes,
            issued_at_utc_ms: spec.issued_at_utc_ms,
            not_before_utc_ms: spec.not_before_utc_ms,
            expires_at_utc_ms: spec.expires_at_utc_ms,
            max_uses: spec.max_uses,
            consumed_uses: 0,
            revocation_epoch: 0,
            revoked_at_utc_ms: None,
            one_shot: spec.one_shot,
            session_bound: spec.session_bound,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GrantAttempt {
    grant_id: String,
    target_id: String,
    tier: String,
    session_binding: Option<String>,
    scope: Grant,
}

impl GrantAttempt {
    pub fn new(grant_id: impl Into<String>, binding: &TargetBinding, scope: Grant) -> Self {
        Self {
            grant_id: grant_id.into(),
            target_id: binding.target_id().to_owned(),
            tier: binding.tier().as_str().to_owned(),
            session_binding: Some(binding.session_binding().to_owned()),
            scope,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GrantReservation {
    pub grant_id: String,
    pub consumed_uses: u64,
    pub remaining_uses: u64,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GrantDenialKind {
    NotFound,
    NotYetValid,
    Expired,
    Revoked,
    Exhausted,
    TargetMismatch,
    ScopeMissing,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GrantDenial {
    pub grant_id: String,
    pub kind: GrantDenialKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GrantDecision {
    Authorized(GrantReservation),
    Denied(GrantDenial),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RevokeDecision {
    Revoked(GrantRecord),
    AlreadyRevoked(GrantRecord),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthStoreErrorKind {
    Read,
    Parse,
    Validate,
    Serialize,
    Prepare,
    Write,
    Publish,
    Sync,
    GenerationConflict,
    GenerationOverflow,
    DuplicateGrant,
    GrantNotFound,
    LegacyUnverified,
    LockContended,
    LockUnavailable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthStoreError {
    pub kind: AuthStoreErrorKind,
    pub message: String,
    pub expected_generation: Option<u64>,
    pub actual_generation: Option<u64>,
    /// The replacement name was installed, but a later durability step failed.
    pub published: bool,
}

impl AuthStoreError {
    fn new(kind: AuthStoreErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            expected_generation: None,
            actual_generation: None,
            published: false,
        }
    }

    fn after_publish(kind: AuthStoreErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            expected_generation: None,
            actual_generation: None,
            published: true,
        }
    }

    fn conflict(expected: u64, actual: u64) -> Self {
        Self {
            kind: AuthStoreErrorKind::GenerationConflict,
            message: format!(
                "grant store generation conflict: expected {expected}, found {actual}"
            ),
            expected_generation: Some(expected),
            actual_generation: Some(actual),
            published: false,
        }
    }
}

impl std::fmt::Display for AuthStoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for AuthStoreError {}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoreDocument {
    schema_version: u32,
    generation: u64,
    records: BTreeMap<String, GrantRecord>,
}

impl Default for StoreDocument {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            generation: 0,
            records: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct AuthStore {
    path: PathBuf,
    document: StoreDocument,
}

impl AuthStore {
    /// Opens `path`; a missing file is an empty generation-zero store while a
    /// malformed or semantically invalid file is a typed error.
    pub fn open_at(path: impl Into<PathBuf>) -> Result<Self, AuthStoreError> {
        let path = path.into();
        let document = read_document(&path)?.unwrap_or_default();
        Ok(Self { path, document })
    }

    pub fn generation(&self) -> u64 {
        self.document.generation
    }

    pub fn list(&self) -> Vec<GrantRecord> {
        self.document.records.values().cloned().collect()
    }

    pub fn create(&mut self, spec: GrantSpec) -> Result<GrantRecord, AuthStoreError> {
        let record = GrantRecord::from(spec);
        validate_record(&record)?;
        if self.document.records.contains_key(&record.grant_id) {
            return Err(AuthStoreError::new(
                AuthStoreErrorKind::DuplicateGrant,
                format!("grant id already exists: {:?}", record.grant_id),
            ));
        }
        if self.document.records.len() >= MAX_RECORDS {
            return Err(AuthStoreError::new(
                AuthStoreErrorKind::Validate,
                format!("grant store may contain at most {MAX_RECORDS} records"),
            ));
        }
        let expected = self.document.generation;
        let mut candidate = self.document.clone();
        candidate
            .records
            .insert(record.grant_id.clone(), record.clone());
        self.commit_candidate(expected, candidate)?;
        Ok(record)
    }

    /// Reserves one execution attempt and persists its consumption before the
    /// caller performs the effect. There is intentionally no refund API: once
    /// authorized, a failed downstream effect still consumes a one-shot grant.
    pub fn reserve_attempt(
        &mut self,
        attempt: &GrantAttempt,
        now_utc_ms: i64,
    ) -> Result<GrantDecision, AuthStoreError> {
        let Some(record) = self.document.records.get(&attempt.grant_id) else {
            return Ok(denied(&attempt.grant_id, GrantDenialKind::NotFound));
        };
        let denial = if record.target_id != attempt.target_id
            || record.tier != attempt.tier
            || record.session_binding != attempt.session_binding
        {
            Some(GrantDenialKind::TargetMismatch)
        } else if !record.scopes.contains(&attempt.scope) {
            Some(GrantDenialKind::ScopeMissing)
        } else if record.revoked_at_utc_ms.is_some() {
            Some(GrantDenialKind::Revoked)
        } else if now_utc_ms < record.not_before_utc_ms {
            Some(GrantDenialKind::NotYetValid)
        } else if now_utc_ms >= record.expires_at_utc_ms {
            Some(GrantDenialKind::Expired)
        } else if record.consumed_uses >= record.max_uses {
            Some(GrantDenialKind::Exhausted)
        } else {
            None
        };
        if let Some(kind) = denial {
            return Ok(denied(&attempt.grant_id, kind));
        }

        let expected = self.document.generation;
        let mut candidate = self.document.clone();
        let candidate_record = candidate
            .records
            .get_mut(&attempt.grant_id)
            .expect("record was resolved above");
        candidate_record.consumed_uses += 1;
        let consumed_uses = candidate_record.consumed_uses;
        let remaining_uses = candidate_record.max_uses - consumed_uses;
        self.commit_candidate(expected, candidate)?;
        Ok(GrantDecision::Authorized(GrantReservation {
            grant_id: attempt.grant_id.clone(),
            consumed_uses,
            remaining_uses,
            generation: self.document.generation,
        }))
    }

    pub fn revoke(
        &mut self,
        grant_id: &str,
        now_utc_ms: i64,
    ) -> Result<RevokeDecision, AuthStoreError> {
        let Some(record) = self.document.records.get(grant_id) else {
            return Err(AuthStoreError::new(
                AuthStoreErrorKind::GrantNotFound,
                format!("grant id not found: {grant_id:?}"),
            ));
        };
        if record.revoked_at_utc_ms.is_some() {
            return Ok(RevokeDecision::AlreadyRevoked(record.clone()));
        }
        if now_utc_ms < 0 {
            return Err(AuthStoreError::new(
                AuthStoreErrorKind::Validate,
                "revocation time must be a non-negative UTC millisecond value",
            ));
        }
        let expected = self.document.generation;
        let mut candidate = self.document.clone();
        let candidate_record = candidate
            .records
            .get_mut(grant_id)
            .expect("record was resolved above");
        candidate_record.revocation_epoch = candidate_record
            .revocation_epoch
            .checked_add(1)
            .ok_or_else(|| {
                AuthStoreError::new(
                    AuthStoreErrorKind::GenerationOverflow,
                    "grant revocation epoch overflow",
                )
            })?;
        candidate_record.revoked_at_utc_ms = Some(now_utc_ms);
        let revoked = candidate_record.clone();
        self.commit_candidate(expected, candidate)?;
        Ok(RevokeDecision::Revoked(revoked))
    }

    /// Publishes the current in-memory document only if both this handle and
    /// the latest disk document have `expected_generation`.
    pub fn save_if_generation(&mut self, expected_generation: u64) -> Result<(), AuthStoreError> {
        if self.document.generation != expected_generation {
            return Err(AuthStoreError::conflict(
                expected_generation,
                self.document.generation,
            ));
        }
        self.commit_candidate(expected_generation, self.document.clone())
    }

    fn commit_candidate(
        &mut self,
        expected_generation: u64,
        mut candidate: StoreDocument,
    ) -> Result<(), AuthStoreError> {
        let lock_path = lock_path(&self.path)?;
        let _lock = PathLock::try_acquire(&lock_path).map_err(|error| {
            let kind = if error.kind() == LockErrorKind::Contended {
                AuthStoreErrorKind::LockContended
            } else {
                AuthStoreErrorKind::LockUnavailable
            };
            AuthStoreError::new(
                kind,
                format!("lock grant store {}: {}", self.path.display(), error),
            )
        })?;
        let actual_generation = read_document(&self.path)?
            .map(|document| document.generation)
            .unwrap_or(0);
        if actual_generation != expected_generation {
            return Err(AuthStoreError::conflict(
                expected_generation,
                actual_generation,
            ));
        }
        candidate.generation = expected_generation.checked_add(1).ok_or_else(|| {
            AuthStoreError::new(
                AuthStoreErrorKind::GenerationOverflow,
                "grant store generation overflow",
            )
        })?;
        validate_document(&candidate)?;
        match publish_document(&self.path, &candidate) {
            Ok(()) => {
                self.document = candidate;
                Ok(())
            }
            Err(error) if error.published => {
                // The name now resolves to `candidate`; retain that generation
                // locally even though directory durability is uncertain.
                self.document = candidate;
                Err(error)
            }
            Err(error) => Err(error),
        }
    }
}

fn lock_path(path: &Path) -> Result<PathBuf, AuthStoreError> {
    let file_name = path.file_name().ok_or_else(|| {
        AuthStoreError::new(
            AuthStoreErrorKind::LockUnavailable,
            "grant store file name required for lock sidecar",
        )
    })?;
    let mut lock_name = file_name.to_os_string();
    lock_name.push(".lock");
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    Ok(parent.join(lock_name))
}

fn denied(grant_id: &str, kind: GrantDenialKind) -> GrantDecision {
    GrantDecision::Denied(GrantDenial {
        grant_id: grant_id.to_owned(),
        kind,
    })
}

fn read_document(path: &Path) -> Result<Option<StoreDocument>, AuthStoreError> {
    let raw = match fs::read(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(AuthStoreError::new(
                AuthStoreErrorKind::Read,
                format!("read {}: {error}", path.display()),
            ));
        }
    };
    let document: StoreDocument = serde_json::from_slice(&raw).map_err(|error| {
        AuthStoreError::new(
            AuthStoreErrorKind::Parse,
            format!("parse {}: {error}", path.display()),
        )
    })?;
    validate_document(&document)?;
    Ok(Some(document))
}

fn validate_document(document: &StoreDocument) -> Result<(), AuthStoreError> {
    if document.schema_version == 1 {
        return Err(AuthStoreError::new(
            AuthStoreErrorKind::LegacyUnverified,
            "grant store schema 1 contains unverified target identity and requires explicit migration",
        ));
    }
    if document.schema_version != SCHEMA_VERSION {
        return Err(AuthStoreError::new(
            AuthStoreErrorKind::Validate,
            format!(
                "unsupported grant store schema version {}",
                document.schema_version
            ),
        ));
    }
    if document.records.len() > MAX_RECORDS {
        return Err(AuthStoreError::new(
            AuthStoreErrorKind::Validate,
            format!("grant store may contain at most {MAX_RECORDS} records"),
        ));
    }
    for (key, record) in &document.records {
        if key != &record.grant_id {
            return Err(AuthStoreError::new(
                AuthStoreErrorKind::Validate,
                format!("record key {key:?} does not match its grant id"),
            ));
        }
        validate_record(record)?;
    }
    Ok(())
}

fn validate_record(record: &GrantRecord) -> Result<(), AuthStoreError> {
    if record.binding_version != TARGET_BINDING_VERSION {
        return Err(AuthStoreError::new(
            AuthStoreErrorKind::Validate,
            "grant binding version is unsupported",
        ));
    }
    for (name, value) in [
        ("grant_id", record.grant_id.as_str()),
        ("target_id", record.target_id.as_str()),
        ("tier", record.tier.as_str()),
    ] {
        if value.trim().is_empty() || value.len() > MAX_TEXT_BYTES {
            return Err(AuthStoreError::new(
                AuthStoreErrorKind::Validate,
                format!("{name} must contain 1..={MAX_TEXT_BYTES} bytes"),
            ));
        }
    }
    if !matches!(record.tier.as_str(), "current" | "ssh" | "vnc") {
        return Err(AuthStoreError::new(
            AuthStoreErrorKind::Validate,
            "tier must be current, ssh, or vnc; unavailable RDP cannot receive grants",
        ));
    }
    let Some(binding) = &record.session_binding else {
        return Err(AuthStoreError::new(
            AuthStoreErrorKind::Validate,
            "every grant must bind an exact target session identity",
        ));
    };
    if !valid_binding_id(&record.target_id, "agt-cu-tgt-v1-")
        || !valid_binding_id(binding, "agt-cu-ses-v1-")
    {
        return Err(AuthStoreError::new(
            AuthStoreErrorKind::Validate,
            "grant target/session identity is not a verified binding",
        ));
    }
    if !record.session_bound {
        return Err(AuthStoreError::new(
            AuthStoreErrorKind::Validate,
            "every grant must be session-bound",
        ));
    }
    if binding.trim().is_empty() || binding.len() > MAX_TEXT_BYTES {
        return Err(AuthStoreError::new(
            AuthStoreErrorKind::Validate,
            format!("session_binding must contain 1..={MAX_TEXT_BYTES} bytes"),
        ));
    }
    if record.scopes.is_empty() {
        return Err(AuthStoreError::new(
            AuthStoreErrorKind::Validate,
            "grant must contain at least one explicit scope",
        ));
    }
    if record.issued_at_utc_ms < 0
        || record.not_before_utc_ms < record.issued_at_utc_ms
        || record.expires_at_utc_ms <= record.not_before_utc_ms
    {
        return Err(AuthStoreError::new(
            AuthStoreErrorKind::Validate,
            "grant times must be non-negative with issued <= not_before < expires",
        ));
    }
    let (max_uses, max_lifetime_ms) = if record.scopes.contains(&Grant::Actuate) {
        (MAX_ACTUATE_USES, MAX_ACTUATE_LIFETIME_MS)
    } else {
        (MAX_OBSERVE_USES, MAX_OBSERVE_LIFETIME_MS)
    };
    if record.max_uses == 0 || record.max_uses > max_uses {
        return Err(AuthStoreError::new(
            AuthStoreErrorKind::Validate,
            format!("max_uses must be within 1..={max_uses} for these scopes"),
        ));
    }
    if record.expires_at_utc_ms - record.issued_at_utc_ms > max_lifetime_ms {
        return Err(AuthStoreError::new(
            AuthStoreErrorKind::Validate,
            format!("grant lifetime exceeds the {max_lifetime_ms} ms scope limit"),
        ));
    }
    if record.consumed_uses > record.max_uses {
        return Err(AuthStoreError::new(
            AuthStoreErrorKind::Validate,
            "consumed_uses exceeds max_uses",
        ));
    }
    if record.one_shot != (record.max_uses == 1) {
        return Err(AuthStoreError::new(
            AuthStoreErrorKind::Validate,
            "one_shot must be true exactly when max_uses is one",
        ));
    }
    match (record.revocation_epoch, record.revoked_at_utc_ms) {
        (0, None) | (1.., Some(_)) => {}
        _ => {
            return Err(AuthStoreError::new(
                AuthStoreErrorKind::Validate,
                "revocation_epoch and revoked_at_utc_ms are inconsistent",
            ));
        }
    }
    if record
        .revoked_at_utc_ms
        .is_some_and(|value| value < record.issued_at_utc_ms)
    {
        return Err(AuthStoreError::new(
            AuthStoreErrorKind::Validate,
            "revoked_at_utc_ms must not predate issuance",
        ));
    }
    Ok(())
}

fn valid_binding_id(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn publish_document(path: &Path, document: &StoreDocument) -> Result<(), AuthStoreError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|error| {
        AuthStoreError::new(
            AuthStoreErrorKind::Prepare,
            format!("create {}: {error}", parent.display()),
        )
    })?;
    let raw = serde_json::to_vec_pretty(document)
        .map_err(|error| AuthStoreError::new(AuthStoreErrorKind::Serialize, error.to_string()))?;
    let (temporary, mut file) = create_temporary(parent, path).map_err(|error| {
        AuthStoreError::new(
            AuthStoreErrorKind::Prepare,
            format!("prepare {}: {error}", path.display()),
        )
    })?;
    let mut cleanup = TemporaryFile::new(temporary.clone());
    file.write_all(&raw)
        .and_then(|()| file.flush())
        .and_then(|()| file.sync_all())
        .map_err(|error| {
            AuthStoreError::new(
                AuthStoreErrorKind::Write,
                format!("write {}: {error}", temporary.display()),
            )
        })?;
    drop(file);
    replace_prepared(&temporary, path).map_err(|error| {
        AuthStoreError::new(
            AuthStoreErrorKind::Publish,
            format!("publish {}: {error}", path.display()),
        )
    })?;
    cleanup.disarm();
    sync_parent(parent).map_err(|error| {
        AuthStoreError::after_publish(
            AuthStoreErrorKind::Sync,
            format!("sync {}: {error}", parent.display()),
        )
    })
}

static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(0);

fn create_temporary(parent: &Path, destination: &Path) -> io::Result<(PathBuf, File)> {
    let file_name = destination.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "grant store file name required",
        )
    })?;
    for _ in 0..32 {
        let temporary = parent.join(format!(
            ".{}.{}-{}.tmp",
            file_name.to_string_lossy(),
            std::process::id(),
            NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => return Ok((temporary, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "grant store temporary name attempts exhausted",
    ))
}

fn replace_prepared(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(unix)]
fn sync_parent(parent: &Path) -> io::Result<()> {
    File::open(parent)?.sync_all()
}

#[cfg(not(unix))]
fn sync_parent(_parent: &Path) -> io::Result<()> {
    Ok(())
}

struct TemporaryFile {
    path: PathBuf,
    armed: bool,
}

impl TemporaryFile {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{target::TargetRef, target_binding::TargetBinding};

    fn binding() -> TargetBinding {
        TargetBinding {
            tier: TargetRef::Current,
            target_id: format!("agt-cu-tgt-v1-{}", "1".repeat(64)),
            session_binding: format!("agt-cu-ses-v1-{}", "2".repeat(64)),
        }
    }

    fn spec(id: &str, scopes: &[Grant], max_uses: u64) -> GrantSpec {
        GrantSpec::new(
            id,
            &binding(),
            scopes.iter().copied().collect(),
            1_000,
            1_100,
            2_000,
            max_uses,
        )
    }

    fn attempt(id: &str, scope: Grant) -> GrantAttempt {
        GrantAttempt::new(id, &binding(), scope)
    }

    fn temporary_path(label: &str) -> PathBuf {
        static NEXT_TEST: AtomicU64 = AtomicU64::new(0);
        std::env::temp_dir().join(format!(
            "agenterm-cu-auth-store-{label}-{}-{}.json",
            std::process::id(),
            NEXT_TEST.fetch_add(1, Ordering::Relaxed)
        ))
    }

    struct TestPath(PathBuf);

    impl TestPath {
        fn new(label: &str) -> Self {
            Self(temporary_path(label))
        }
    }

    impl Drop for TestPath {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
            if let Ok(lock) = lock_path(&self.0) {
                let _ = fs::remove_file(lock);
            }
        }
    }

    #[test]
    fn missing_is_empty_and_second_save_reopens() {
        let path = TestPath::new("roundtrip");
        let mut store = AuthStore::open_at(&path.0).unwrap();
        assert_eq!(store.generation(), 0);
        assert!(store.list().is_empty());

        store.create(spec("grant-a", &[Grant::Observe], 2)).unwrap();
        store.create(spec("grant-b", &[Grant::Actuate], 1)).unwrap();
        assert_eq!(store.generation(), 2);

        let reopened = AuthStore::open_at(&path.0).unwrap();
        assert_eq!(reopened.generation(), 2);
        assert_eq!(reopened.list().len(), 2);
    }

    #[test]
    fn corrupt_and_semantically_unbounded_files_fail_open() {
        let corrupt = TestPath::new("corrupt");
        fs::write(&corrupt.0, b"{not json").unwrap();
        assert_eq!(
            AuthStore::open_at(&corrupt.0).unwrap_err().kind,
            AuthStoreErrorKind::Parse
        );

        let invalid = TestPath::new("unbounded");
        let mut record = GrantRecord::from(spec("grant", &[Grant::Observe], 1));
        record.max_uses = 0;
        record.one_shot = false;
        let document = StoreDocument {
            schema_version: SCHEMA_VERSION,
            generation: 1,
            records: BTreeMap::from([(record.grant_id.clone(), record)]),
        };
        fs::write(&invalid.0, serde_json::to_vec(&document).unwrap()).unwrap();
        assert_eq!(
            AuthStore::open_at(&invalid.0).unwrap_err().kind,
            AuthStoreErrorKind::Validate
        );

        let limits = TestPath::new("scope-limits");
        let mut store = AuthStore::open_at(&limits.0).unwrap();
        assert_eq!(
            store
                .create(spec(
                    "too-many-actuations",
                    &[Grant::Actuate],
                    MAX_ACTUATE_USES + 1,
                ))
                .unwrap_err()
                .kind,
            AuthStoreErrorKind::Validate
        );
        let mut too_long = spec("too-long", &[Grant::Actuate], 1);
        too_long.expires_at_utc_ms = too_long.issued_at_utc_ms + MAX_ACTUATE_LIFETIME_MS + 1;
        assert_eq!(
            store.create(too_long).unwrap_err().kind,
            AuthStoreErrorKind::Validate
        );
    }

    #[test]
    fn legacy_unverified_schema_is_rejected_without_rewriting_bytes() {
        let path = TestPath::new("legacy-schema");
        let legacy = serde_json::json!({
            "schema_version": 1,
            "generation": 1,
            "records": {
                "legacy": {
                    "grant_id": "legacy",
                    "target_id": "caller-supplied-target",
                    "tier": "current",
                    "session_binding": "caller-supplied-session",
                    "scopes": ["observe"],
                    "issued_at_utc_ms": 1000,
                    "not_before_utc_ms": 1000,
                    "expires_at_utc_ms": 2000,
                    "max_uses": 1,
                    "consumed_uses": 0,
                    "revocation_epoch": 0,
                    "revoked_at_utc_ms": null,
                    "one_shot": true,
                    "session_bound": true
                }
            }
        });
        let bytes = serde_json::to_vec_pretty(&legacy).unwrap();
        fs::write(&path.0, &bytes).unwrap();
        assert_eq!(
            AuthStore::open_at(&path.0).unwrap_err().kind,
            AuthStoreErrorKind::LegacyUnverified
        );
        assert_eq!(fs::read(&path.0).unwrap(), bytes);
    }

    #[test]
    fn schema_two_rejects_binding_text_that_did_not_come_from_the_sealed_provider() {
        let path = TestPath::new("unverified-binding");
        let mut record = GrantRecord::from(spec("grant", &[Grant::Observe], 1));
        record.target_id = format!("agt-cu-tgt-v1-{}", "A".repeat(64));
        let document = StoreDocument {
            schema_version: SCHEMA_VERSION,
            generation: 1,
            records: BTreeMap::from([(record.grant_id.clone(), record)]),
        };
        fs::write(&path.0, serde_json::to_vec(&document).unwrap()).unwrap();
        assert_eq!(
            AuthStore::open_at(&path.0).unwrap_err().kind,
            AuthStoreErrorKind::Validate
        );
    }

    #[test]
    fn one_shot_is_consumed_before_a_failed_effect_and_scopes_are_independent() {
        let path = TestPath::new("one-shot");
        let mut store = AuthStore::open_at(&path.0).unwrap();
        store.create(spec("grant", &[Grant::Actuate], 1)).unwrap();

        assert_eq!(
            store
                .reserve_attempt(&attempt("grant", Grant::Observe), 1_200)
                .unwrap(),
            denied("grant", GrantDenialKind::ScopeMissing)
        );
        let generation = store.generation();
        let reservation = store
            .reserve_attempt(&attempt("grant", Grant::Actuate), 1_200)
            .unwrap();
        assert!(matches!(reservation, GrantDecision::Authorized(_)));
        assert_eq!(store.generation(), generation + 1);

        // Simulate a downstream effect failure by intentionally doing nothing.
        let reopened = AuthStore::open_at(&path.0).unwrap();
        let mut reopened = reopened;
        assert_eq!(
            reopened
                .reserve_attempt(&attempt("grant", Grant::Actuate), 1_200)
                .unwrap(),
            denied("grant", GrantDenialKind::Exhausted)
        );
    }

    #[test]
    fn target_mismatch_does_not_consume() {
        let path = TestPath::new("target");
        let mut store = AuthStore::open_at(&path.0).unwrap();
        store.create(spec("grant", &[Grant::Observe], 2)).unwrap();
        let generation = store.generation();
        let mut wrong = attempt("grant", Grant::Observe);
        wrong.session_binding = Some(format!("agt-cu-ses-v1-{}", "3".repeat(64)));
        assert_eq!(
            store.reserve_attempt(&wrong, 1_200).unwrap(),
            denied("grant", GrantDenialKind::TargetMismatch)
        );
        assert_eq!(store.generation(), generation);
        assert_eq!(store.list()[0].consumed_uses, 0);
    }

    #[test]
    fn revocation_is_persisted_and_denies_without_consumption() {
        let path = TestPath::new("revoke");
        let mut store = AuthStore::open_at(&path.0).unwrap();
        store.create(spec("grant", &[Grant::Observe], 2)).unwrap();
        let revoked = store.revoke("grant", 1_250).unwrap();
        assert!(matches!(revoked, RevokeDecision::Revoked(_)));
        assert_eq!(store.list()[0].revocation_epoch, 1);
        assert_eq!(
            store
                .reserve_attempt(&attempt("grant", Grant::Observe), 1_300)
                .unwrap(),
            denied("grant", GrantDenialKind::Revoked)
        );
        assert_eq!(
            AuthStore::open_at(&path.0).unwrap().list()[0].consumed_uses,
            0
        );
    }

    #[test]
    fn not_before_and_expiry_boundaries_are_closed_open() {
        let path = TestPath::new("time");
        let mut store = AuthStore::open_at(&path.0).unwrap();
        store.create(spec("grant", &[Grant::Observe], 3)).unwrap();
        assert_eq!(
            store
                .reserve_attempt(&attempt("grant", Grant::Observe), 1_099)
                .unwrap(),
            denied("grant", GrantDenialKind::NotYetValid)
        );
        assert!(matches!(
            store
                .reserve_attempt(&attempt("grant", Grant::Observe), 1_100)
                .unwrap(),
            GrantDecision::Authorized(_)
        ));
        assert_eq!(
            store
                .reserve_attempt(&attempt("grant", Grant::Observe), 2_000)
                .unwrap(),
            denied("grant", GrantDenialKind::Expired)
        );
    }

    #[test]
    fn stale_generation_is_rejected_before_mutation() {
        let path = TestPath::new("cas");
        let mut first = AuthStore::open_at(&path.0).unwrap();
        let mut stale = AuthStore::open_at(&path.0).unwrap();
        first.create(spec("first", &[Grant::Observe], 1)).unwrap();
        let error = stale
            .create(spec("stale", &[Grant::Observe], 1))
            .unwrap_err();
        assert_eq!(error.kind, AuthStoreErrorKind::GenerationConflict);
        assert!(stale.list().is_empty());
        assert_eq!(AuthStore::open_at(&path.0).unwrap().list().len(), 1);
    }

    #[test]
    fn contended_sidecar_fails_without_publishing_and_release_allows_retry() {
        let path = TestPath::new("lock-contention");
        let sidecar = lock_path(&path.0).unwrap();
        let lock = PathLock::try_acquire(&sidecar).expect("own sidecar lock");
        let mut store = AuthStore::open_at(&path.0).unwrap();

        let error = store
            .create(spec("blocked", &[Grant::Actuate], 1))
            .unwrap_err();
        assert_eq!(error.kind, AuthStoreErrorKind::LockContended);
        assert!(!path.0.exists(), "contended writer must not publish");
        assert!(store.list().is_empty(), "contended writer must not mutate");

        drop(lock);
        store
            .create(spec("committed", &[Grant::Actuate], 1))
            .expect("released sidecar permits one commit");
        assert_eq!(AuthStore::open_at(&path.0).unwrap().list().len(), 1);
    }
}
