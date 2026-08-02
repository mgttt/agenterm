use std::{
    env, fs,
    io::{BufReader, Write as _},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

use crate::{
    build_identity::BuildIdentity,
    ipc_endpoint::{IpcEndpoint, LogicalInstance, ServerScopeId},
    ipc_transport::{IPC_RESPONSE_MAX_BYTES, IpcStream, read_bounded_ipc_line},
    platform::{
        paths,
        process::{self, ProcessObservation},
    },
    protocol::{IpcRequest, IpcResponse},
    upgrade_identity::UpgradeIdentity,
};

const LEGACY_INSTANCE_SCHEMA_VERSION: u32 = 1;
const INSTANCE_SCHEMA_VERSION: u32 = 2;
const CLEANUP_RECEIPT_SCHEMA_VERSION: u32 = 1;
static NEXT_LEASE_NONCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Deserialize, Serialize)]
struct IntentionalShutdown {
    pid: u32,
    #[serde(default)]
    address: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    endpoint: Option<IpcEndpoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    server_scope_id: Option<ServerScopeId>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct InstanceRecord {
    pub schema_version: u32,
    pub pid: u32,
    /// Legacy TCP address retained while schema-v1 clients coexist with v2.
    pub address: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<IpcEndpoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logical_instance: Option<LogicalInstance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_scope_id: Option<ServerScopeId>,
    pub version: String,
    pub session: String,
    pub workspace_path: String,
    pub started_at_unix_ms: u128,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_start_identity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_nonce: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_epoch: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub test_fixture: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upgrade_identity: Option<UpgradeIdentity>,
}

impl InstanceRecord {
    /// Resolve the typed endpoint for both schema-v2 and legacy schema-v1
    /// registrations without rewriting the on-disk record.
    pub(crate) fn resolved_endpoint(&self) -> Option<IpcEndpoint> {
        self.endpoint.clone().or_else(|| {
            (self.schema_version == LEGACY_INSTANCE_SCHEMA_VERSION)
                .then(|| IpcEndpoint::from_legacy_address(&self.address).ok())
                .flatten()
        })
    }

    pub(crate) fn resolved_logical_instance(&self) -> LogicalInstance {
        self.logical_instance.clone().unwrap_or_default()
    }

    pub(crate) fn legacy_client_compatibility(&self) -> &'static str {
        if self
            .resolved_endpoint()
            .is_some_and(|endpoint| endpoint.legacy_address().is_some())
        {
            "same_tcp_endpoint"
        } else {
            "unsupported_no_legacy_listener"
        }
    }
}

#[derive(Debug)]
pub(crate) struct DiscoveredInstance {
    pub record: InstanceRecord,
    path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RegistrationOwnerState {
    ConfirmedLive {
        observed_start_identity: String,
    },
    Dead {
        reason: String,
    },
    PidReused {
        observed_start_identity: String,
    },
    OwnerUnknown {
        reason: String,
        observed_start_identity: Option<String>,
    },
}

impl RegistrationOwnerState {
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::ConfirmedLive { .. } => "confirmed_live",
            Self::Dead { .. } => "dead",
            Self::PidReused { .. } => "pid_reused",
            Self::OwnerUnknown { .. } => "owner_unknown",
        }
    }

    pub(crate) fn reason(&self) -> &str {
        match self {
            Self::ConfirmedLive { .. } => "process_start_identity_matched",
            Self::Dead { reason } | Self::OwnerUnknown { reason, .. } => reason,
            Self::PidReused { .. } => "process_start_identity_mismatch",
        }
    }

    pub(crate) fn observed_start_identity(&self) -> Option<&str> {
        match self {
            Self::ConfirmedLive {
                observed_start_identity,
            }
            | Self::PidReused {
                observed_start_identity,
            } => Some(observed_start_identity),
            Self::OwnerUnknown {
                observed_start_identity,
                ..
            } => observed_start_identity.as_deref(),
            Self::Dead { .. } => None,
        }
    }

    pub(crate) fn is_stale(&self) -> bool {
        matches!(self, Self::Dead { .. } | Self::PidReused { .. })
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct InstanceCleanupReceipt {
    pub schema_version: u32,
    pub operation: &'static str,
    pub target_path: String,
    pub pid: u32,
    pub endpoint: Option<String>,
    pub server_scope_id: Option<String>,
    pub expected_process_start_identity: Option<String>,
    pub expected_lease_nonce: Option<String>,
    pub expected_server_epoch: Option<String>,
    pub confirmed_owner_state: String,
    pub reason: String,
    pub result: String,
    pub legacy_alias_result: String,
    pub completed_at_unix_ms: u128,
}

impl InstanceCleanupReceipt {
    pub(crate) fn failed(&self) -> bool {
        self.result == "failed" || self.legacy_alias_result == "failed"
    }
}

pub(crate) struct InstanceRegistration {
    path: PathBuf,
    legacy_alias_path: Option<PathBuf>,
    pid: u32,
    address: String,
    endpoint: IpcEndpoint,
    server_scope_id: ServerScopeId,
    process_start_identity: String,
    lease_nonce: String,
    server_epoch: String,
}

impl InstanceRegistration {
    pub(crate) fn address(&self) -> &str {
        &self.address
    }
}

impl Drop for InstanceRegistration {
    fn drop(&mut self) {
        let matches_registration = fs::read(&self.path)
            .ok()
            .and_then(|content| serde_json::from_slice::<InstanceRecord>(&content).ok())
            .is_some_and(|record| {
                record.pid == self.pid
                    && record.address == self.address
                    && record.resolved_endpoint().as_ref() == Some(&self.endpoint)
                    && record.server_scope_id.as_ref() == Some(&self.server_scope_id)
                    && record.process_start_identity.as_deref()
                        == Some(self.process_start_identity.as_str())
                    && record.lease_nonce.as_deref() == Some(self.lease_nonce.as_str())
                    && record.server_epoch.as_deref() == Some(self.server_epoch.as_str())
            });
        if matches_registration {
            let _ = fs::remove_file(&self.path);
            if let Some(path) = &self.legacy_alias_path {
                let matches_alias = fs::read(path)
                    .ok()
                    .and_then(|content| serde_json::from_slice::<InstanceRecord>(&content).ok())
                    .is_some_and(|record| {
                        record.pid == self.pid
                            && record.process_start_identity.as_deref()
                                == Some(self.process_start_identity.as_str())
                            && record.lease_nonce.as_deref() == Some(self.lease_nonce.as_str())
                            && record.server_epoch.as_deref() == Some(self.server_epoch.as_str())
                    });
                if matches_alias {
                    let _ = fs::remove_file(path);
                }
            }
        }
    }
}

#[allow(dead_code)] // Registry registration is intentionally unavailable to some callers.
pub(crate) fn register_instance(
    address: &str,
    workspace_path: &Path,
    session: &str,
) -> Result<InstanceRegistration> {
    let endpoint = address
        .parse::<IpcEndpoint>()
        .or_else(|_| IpcEndpoint::from_legacy_address(address))
        .map_err(|error| anyhow::anyhow!("invalid IPC endpoint {address:?}: {error}"))?;
    let logical_instance = env::var("AGENTERM_INSTANCE")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(|value| {
            value
                .parse::<LogicalInstance>()
                .map_err(|error| anyhow::anyhow!("invalid AGENTERM_INSTANCE {value:?}: {error}"))
        })
        .transpose()?
        .unwrap_or_default();
    let server_scope_id = ServerScopeId::current(&logical_instance)
        .context("failed to derive the current OS-user server scope")?;
    register_typed_instance(
        endpoint,
        logical_instance,
        server_scope_id,
        workspace_path,
        session,
        "legacy-registration-epoch",
    )
}

pub(crate) fn register_typed_instance(
    endpoint: IpcEndpoint,
    logical_instance: LogicalInstance,
    server_scope_id: ServerScopeId,
    workspace_path: &Path,
    session: &str,
    server_epoch: &str,
) -> Result<InstanceRegistration> {
    let build = BuildIdentity::current();
    let address = endpoint
        .legacy_address()
        .unwrap_or_else(|| endpoint.to_string());
    let process_start_identity = current_process_start_identity()
        .context("failed to determine AgenTerm server process start identity")?;
    let lease_nonce = new_lease_nonce();
    remove_intentional_shutdown_markers(&instances_dir(), &address, Some(&server_scope_id));
    register_instance_in(
        &instances_dir(),
        InstanceRecord {
            schema_version: INSTANCE_SCHEMA_VERSION,
            pid: std::process::id(),
            address,
            endpoint: Some(endpoint),
            logical_instance: Some(logical_instance),
            server_scope_id: Some(server_scope_id),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            session: session.to_owned(),
            workspace_path: workspace_path.display().to_string(),
            started_at_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
            process_start_identity: Some(process_start_identity),
            lease_nonce: Some(lease_nonce),
            server_epoch: Some(server_epoch.to_owned()),
            test_fixture: false,
            upgrade_identity: Some(UpgradeIdentity {
                protocol_version: Some(1),
                version: Some(env!("CARGO_PKG_VERSION").to_owned()),
                git_commit: known_build_value(build.git_commit),
                profile: known_build_value(build.profile),
                cargo_lock_sha256: known_build_value(build.cargo_lock_sha256),
                artifact_manifest_sha256: known_build_value(build.artifact_manifest_sha256),
            }),
        },
    )
}

pub(crate) fn mark_intentional_shutdown(address: &str) -> Result<()> {
    let logical_instance = env::var("AGENTERM_INSTANCE")
        .ok()
        .and_then(|value| value.parse::<LogicalInstance>().ok())
        .unwrap_or_default();
    let scope = ServerScopeId::current(&logical_instance).ok();
    let endpoint = address
        .parse::<IpcEndpoint>()
        .or_else(|_| IpcEndpoint::from_legacy_address(address))
        .ok();
    mark_intentional_shutdown_in(
        &instances_dir(),
        address,
        endpoint,
        scope,
        std::process::id(),
    )
}

pub(crate) fn intentional_shutdown_matches(address: &str, pid: u32) -> bool {
    let logical_instance = env::var("AGENTERM_INSTANCE")
        .ok()
        .and_then(|value| value.parse::<LogicalInstance>().ok())
        .unwrap_or_default();
    let scope = ServerScopeId::current(&logical_instance).ok();
    shutdown_marker_paths(&instances_dir(), address, scope.as_ref())
        .into_iter()
        .any(|path| {
            fs::read(path)
                .ok()
                .and_then(|content| serde_json::from_slice::<IntentionalShutdown>(&content).ok())
                .is_some_and(|marker| {
                    marker.pid == pid
                        && (marker.address == address
                            || scope.as_ref().is_some_and(|expected| {
                                marker.server_scope_id.as_ref() == Some(expected)
                            }))
                })
        })
}

fn mark_intentional_shutdown_in(
    directory: &Path,
    address: &str,
    endpoint: Option<IpcEndpoint>,
    server_scope_id: Option<ServerScopeId>,
    pid: u32,
) -> Result<()> {
    fs::create_dir_all(directory)
        .with_context(|| format!("failed to create {}", directory.display()))?;
    let path = server_scope_id
        .as_ref()
        .map(|scope| intentional_shutdown_scope_path(directory, scope))
        .unwrap_or_else(|| intentional_shutdown_path(directory, address));
    fs::write(
        &path,
        serde_json::to_vec(&IntentionalShutdown {
            pid,
            address: address.to_owned(),
            endpoint,
            server_scope_id,
        })?,
    )
    .with_context(|| format!("failed to write {}", path.display()))
}

fn remove_intentional_shutdown_markers(
    directory: &Path,
    address: &str,
    server_scope_id: Option<&ServerScopeId>,
) {
    for path in shutdown_marker_paths(directory, address, server_scope_id) {
        let _ = fs::remove_file(path);
    }
}

fn shutdown_marker_paths(
    directory: &Path,
    address: &str,
    server_scope_id: Option<&ServerScopeId>,
) -> Vec<PathBuf> {
    let mut paths = vec![intentional_shutdown_path(directory, address)];
    if let Some(scope) = server_scope_id {
        paths.push(intentional_shutdown_scope_path(directory, scope));
    }
    paths
}

fn intentional_shutdown_path(directory: &Path, address: &str) -> PathBuf {
    let safe_address = address
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    // Keep this out of the `*.json` instance-registration namespace consumed
    // by discovery and orphan accounting.
    directory.join(format!(".intentional-shutdown-{safe_address}.marker"))
}

fn intentional_shutdown_scope_path(directory: &Path, server_scope_id: &ServerScopeId) -> PathBuf {
    directory.join(format!(
        ".intentional-shutdown-scope-{}.marker",
        server_scope_id.as_str()
    ))
}

fn known_build_value(value: &str) -> Option<String> {
    (value != "unknown" && !value.trim().is_empty()).then(|| value.to_owned())
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn new_lease_nonce() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = NEXT_LEASE_NONCE.fetch_add(1, Ordering::Relaxed);
    format!("{:x}-{timestamp:x}-{sequence:x}", std::process::id())
}

fn current_process_start_identity() -> Result<String> {
    process::start_identity(std::process::id()).map_err(anyhow::Error::msg)
}

pub(crate) fn discover_instances() -> Result<Vec<DiscoveredInstance>> {
    discover_instances_in(&instances_dir())
}

/// Return the one legacy authority that is safe to reuse for implicit main.
///
/// A schema-v1 record has no logical-instance identity, so matching the exact
/// v0.1.10 default endpoint is essential: arbitrary explicit TCP servers must
/// not silently become the default main authority.
pub(crate) fn discover_healthy_legacy_main_endpoint(
    expected_default: &IpcEndpoint,
    timeout: Duration,
) -> Result<Option<IpcEndpoint>> {
    discover_healthy_legacy_main_endpoint_in(&instances_dir(), expected_default, timeout)
}

fn discover_healthy_legacy_main_endpoint_in(
    directory: &Path,
    expected_default: &IpcEndpoint,
    timeout: Duration,
) -> Result<Option<IpcEndpoint>> {
    let records = discover_instances_in(directory)?;
    for instance in records {
        if instance.record.schema_version != LEGACY_INSTANCE_SCHEMA_VERSION
            || !instance_process_is_alive(instance.record.pid)
            || instance.record.resolved_endpoint().as_ref() != Some(expected_default)
        {
            continue;
        }
        if legacy_protocol_probe(expected_default, timeout) {
            return Ok(Some(expected_default.clone()));
        }
    }
    Ok(None)
}

fn legacy_protocol_probe(endpoint: &IpcEndpoint, timeout: Duration) -> bool {
    let Ok(mut stream) = IpcStream::connect(endpoint, timeout) else {
        return false;
    };
    let request = IpcRequest {
        args: vec!["protocol-info".to_owned()],
        control: None,
    };
    let Ok(payload) = serde_json::to_vec(&request) else {
        return false;
    };
    if stream.write_all(&payload).is_err()
        || stream.write_all(b"\n").is_err()
        || stream.flush().is_err()
    {
        return false;
    }
    let mut reader = BufReader::new(stream);
    read_bounded_ipc_line(
        &mut reader,
        IPC_RESPONSE_MAX_BYTES,
        "legacy AgenTerm protocol probe",
    )
    .ok()
    .and_then(|line| serde_json::from_str::<IpcResponse>(&line).ok())
    .is_some_and(|response| response.ok)
}

pub(crate) fn registration_owner_state(record: &InstanceRecord) -> RegistrationOwnerState {
    match process::observe(record.pid) {
        ProcessObservation::Dead { reason } => RegistrationOwnerState::Dead { reason },
        ProcessObservation::Unknown { reason } => RegistrationOwnerState::OwnerUnknown {
            reason,
            observed_start_identity: None,
        },
        ProcessObservation::Live { start_identity } => {
            match (record.process_start_identity.as_deref(), start_identity) {
                (Some(expected), Some(observed)) if expected == observed => {
                    RegistrationOwnerState::ConfirmedLive {
                        observed_start_identity: observed,
                    }
                }
                (Some(_), Some(observed)) => RegistrationOwnerState::PidReused {
                    observed_start_identity: observed,
                },
                (None, observed) => RegistrationOwnerState::OwnerUnknown {
                    reason: "registration_missing_process_start_identity".to_owned(),
                    observed_start_identity: observed,
                },
                (Some(_), None) => RegistrationOwnerState::OwnerUnknown {
                    reason: "process_start_identity_unavailable".to_owned(),
                    observed_start_identity: None,
                },
            }
        }
        _ => RegistrationOwnerState::OwnerUnknown {
            reason: "unrecognized_process_observation".to_owned(),
            observed_start_identity: None,
        },
    }
}

pub(crate) fn cleanup_instance(instance: &DiscoveredInstance) -> InstanceCleanupReceipt {
    let owner = registration_owner_state(&instance.record);
    let mut receipt = cleanup_receipt(instance, &owner);
    if !owner.is_stale() {
        receipt.result = "not_eligible".to_owned();
        return receipt;
    }

    let current = match fs::read(&instance.path) {
        Ok(content) => match serde_json::from_slice::<InstanceRecord>(&content) {
            Ok(record) => record,
            Err(error) => {
                receipt.result = "identity_changed".to_owned();
                receipt.reason = format!("registration_unparseable_during_revalidation:{error}");
                return receipt;
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            receipt.result = "already_absent".to_owned();
            return receipt;
        }
        Err(error) => {
            receipt.result = "failed".to_owned();
            receipt.reason = format!("registration_revalidation_failed:{error}");
            return receipt;
        }
    };
    if !same_registration_generation(&instance.record, &current) {
        receipt.result = "identity_changed".to_owned();
        receipt.reason = "registration_identity_changed_during_cleanup".to_owned();
        return receipt;
    }
    let revalidated_owner = registration_owner_state(&current);
    receipt.confirmed_owner_state = revalidated_owner.name().to_owned();
    receipt.reason = revalidated_owner.reason().to_owned();
    if !revalidated_owner.is_stale() {
        receipt.result = "identity_changed".to_owned();
        return receipt;
    }

    match fs::remove_file(&instance.path) {
        Ok(()) => receipt.result = "deleted".to_owned(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            receipt.result = "already_absent".to_owned();
        }
        Err(error) => {
            receipt.result = "failed".to_owned();
            receipt.reason = format!("registration_delete_failed:{error}");
            return receipt;
        }
    }

    receipt.legacy_alias_result = cleanup_matching_legacy_alias(instance);
    if let Some(directory) = instance.path.parent() {
        remove_intentional_shutdown_markers(
            directory,
            &instance.record.address,
            instance.record.server_scope_id.as_ref(),
        );
    }
    receipt
}

fn cleanup_receipt(
    instance: &DiscoveredInstance,
    owner: &RegistrationOwnerState,
) -> InstanceCleanupReceipt {
    InstanceCleanupReceipt {
        schema_version: CLEANUP_RECEIPT_SCHEMA_VERSION,
        operation: "registration.cleanup",
        target_path: instance.path.display().to_string(),
        pid: instance.record.pid,
        endpoint: instance
            .record
            .resolved_endpoint()
            .map(|endpoint| endpoint.to_string()),
        server_scope_id: instance
            .record
            .server_scope_id
            .as_ref()
            .map(|scope| scope.to_string()),
        expected_process_start_identity: instance.record.process_start_identity.clone(),
        expected_lease_nonce: instance.record.lease_nonce.clone(),
        expected_server_epoch: instance.record.server_epoch.clone(),
        confirmed_owner_state: owner.name().to_owned(),
        reason: owner.reason().to_owned(),
        result: "not_eligible".to_owned(),
        legacy_alias_result: "not_attempted".to_owned(),
        completed_at_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    }
}

fn cleanup_matching_legacy_alias(instance: &DiscoveredInstance) -> String {
    if instance.record.schema_version != INSTANCE_SCHEMA_VERSION
        || !instance
            .record
            .resolved_endpoint()
            .is_some_and(|endpoint| endpoint.legacy_address().is_some())
    {
        return "not_applicable".to_owned();
    }
    let Some(directory) = instance.path.parent() else {
        return "not_applicable".to_owned();
    };
    let alias = directory.join(format!("{}-v1.json", instance.record.pid));
    let alias_record = match fs::read(&alias) {
        Ok(content) => serde_json::from_slice::<InstanceRecord>(&content).ok(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return "already_absent".to_owned();
        }
        Err(_) => return "failed".to_owned(),
    };
    if !alias_record.is_some_and(|record| {
        record.schema_version == LEGACY_INSTANCE_SCHEMA_VERSION
            && same_registered_authority(&instance.record, &record)
    }) {
        return "identity_changed".to_owned();
    }
    match fs::remove_file(alias) {
        Ok(()) => "deleted".to_owned(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => "already_absent".to_owned(),
        Err(_) => "failed".to_owned(),
    }
}

fn same_registration_generation(left: &InstanceRecord, right: &InstanceRecord) -> bool {
    left.schema_version == right.schema_version
        && left.pid == right.pid
        && left.address == right.address
        && left.resolved_endpoint() == right.resolved_endpoint()
        && left.logical_instance == right.logical_instance
        && left.server_scope_id == right.server_scope_id
        && left.session == right.session
        && left.workspace_path == right.workspace_path
        && left.started_at_unix_ms == right.started_at_unix_ms
        && left.process_start_identity == right.process_start_identity
        && left.lease_nonce == right.lease_nonce
        && left.server_epoch == right.server_epoch
        && left.test_fixture == right.test_fixture
}

pub(crate) fn instance_process_is_alive(pid: u32) -> bool {
    !matches!(process::observe(pid), ProcessObservation::Dead { .. })
}

fn instances_dir() -> PathBuf {
    paths::instance_registry_dir(env::var_os("AGENTERM_INSTANCE_DIR"))
}

fn register_instance_in(directory: &Path, record: InstanceRecord) -> Result<InstanceRegistration> {
    fs::create_dir_all(directory)
        .with_context(|| format!("failed to create {}", directory.display()))?;
    prune_replaced_stale_registrations(directory, &record)?;
    let path = registration_path(directory, &record);
    let temporary = directory.join(format!(
        ".{}.{}.tmp",
        record.pid,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    fs::write(&temporary, serde_json::to_vec_pretty(&record)?)
        .with_context(|| format!("failed to write {}", temporary.display()))?;
    if path.exists() {
        fs::remove_file(&path).with_context(|| format!("failed to replace {}", path.display()))?;
    }
    fs::rename(&temporary, &path)
        .with_context(|| format!("failed to publish {}", path.display()))?;
    let legacy_alias_path = record
        .resolved_endpoint()
        .and_then(|endpoint| endpoint.legacy_address())
        .map(|legacy_address| -> Result<PathBuf> {
            let alias = directory.join(format!("{}-v1.json", record.pid));
            let mut legacy = record.clone();
            legacy.schema_version = LEGACY_INSTANCE_SCHEMA_VERSION;
            legacy.address = legacy_address;
            legacy.endpoint = None;
            legacy.logical_instance = None;
            legacy.server_scope_id = None;
            fs::write(&alias, serde_json::to_vec_pretty(&legacy)?)
                .with_context(|| format!("failed to publish {}", alias.display()))?;
            Ok(alias)
        })
        .transpose()?;
    Ok(InstanceRegistration {
        path,
        legacy_alias_path,
        pid: record.pid,
        address: record.address,
        endpoint: record
            .endpoint
            .expect("schema-v2 registration requires a typed endpoint"),
        server_scope_id: record
            .server_scope_id
            .expect("schema-v2 registration requires a server scope"),
        process_start_identity: record
            .process_start_identity
            .expect("schema-v2 registration requires a process start identity"),
        lease_nonce: record
            .lease_nonce
            .expect("schema-v2 registration requires a lease nonce"),
        server_epoch: record
            .server_epoch
            .expect("schema-v2 registration requires a server epoch"),
    })
}

fn registration_path(directory: &Path, record: &InstanceRecord) -> PathBuf {
    record.lease_nonce.as_ref().map_or_else(
        || directory.join(format!("{}.json", record.pid)),
        |nonce| directory.join(format!("{}-{nonce}.json", record.pid)),
    )
}

fn prune_replaced_stale_registrations(directory: &Path, incoming: &InstanceRecord) -> Result<()> {
    let Some(incoming_scope) = incoming.server_scope_id.as_ref() else {
        return Ok(());
    };
    let Some(incoming_endpoint) = incoming.resolved_endpoint() else {
        return Ok(());
    };
    for instance in discover_instances_in(directory)? {
        if instance.record.schema_version == INSTANCE_SCHEMA_VERSION
            && instance.record.pid != incoming.pid
            && instance.record.server_scope_id.as_ref() == Some(incoming_scope)
            && instance.record.resolved_endpoint().as_ref() == Some(&incoming_endpoint)
            && registration_owner_state(&instance.record).is_stale()
        {
            let receipt = cleanup_instance(&instance);
            if receipt.failed() {
                anyhow::bail!(
                    "failed to remove replaced stale registration {}: {}",
                    receipt.target_path,
                    receipt.reason
                );
            }
        }
    }
    Ok(())
}

fn discover_instances_in(directory: &Path) -> Result<Vec<DiscoveredInstance>> {
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let mut instances: Vec<DiscoveredInstance> = Vec::new();
    for entry in fs::read_dir(directory)
        .with_context(|| format!("failed to read {}", directory.display()))?
    {
        let entry = entry?;
        if entry.path().extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let Ok(content) = fs::read(entry.path()) else {
            continue;
        };
        let Ok(record) = serde_json::from_slice::<InstanceRecord>(&content) else {
            continue;
        };
        if matches!(
            record.schema_version,
            LEGACY_INSTANCE_SCHEMA_VERSION | INSTANCE_SCHEMA_VERSION
        ) && record.resolved_endpoint().is_some()
        {
            let discovered = DiscoveredInstance {
                record,
                path: entry.path(),
            };
            if let Some(existing) = instances
                .iter_mut()
                .find(|existing| same_registered_authority(&existing.record, &discovered.record))
            {
                if discovered.record.schema_version > existing.record.schema_version {
                    *existing = discovered;
                }
            } else {
                instances.push(discovered);
            }
        }
    }
    instances.sort_by(|left, right| {
        left.record
            .resolved_endpoint()
            .map(|endpoint| endpoint.to_string())
            .cmp(
                &right
                    .record
                    .resolved_endpoint()
                    .map(|endpoint| endpoint.to_string()),
            )
    });
    Ok(instances)
}

fn same_registered_authority(left: &InstanceRecord, right: &InstanceRecord) -> bool {
    if let (Some(left_nonce), Some(right_nonce)) = (&left.lease_nonce, &right.lease_nonce) {
        return left.pid == right.pid
            && left_nonce == right_nonce
            && left.process_start_identity == right.process_start_identity
            && left.server_epoch == right.server_epoch;
    }
    let same_process_facts = left.pid == right.pid
        && left.started_at_unix_ms == right.started_at_unix_ms
        && left.session == right.session
        && left.workspace_path == right.workspace_path;
    same_process_facts
        && match (&left.server_scope_id, &right.server_scope_id) {
            (Some(left), Some(right)) => left == right,
            (None, _) | (_, None) => true,
        }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_v2_without_new_identity_fields_remains_readable() {
        let record: InstanceRecord = serde_json::from_str(
            r#"{
                "schema_version": 2,
                "pid": 42,
                "address": "127.0.0.1:49991",
                "endpoint": "tcp:127.0.0.1:49991",
                "logical_instance": "main",
                "server_scope_id": "agt-v1-00000000000000000000000000000000",
                "version": "0.1.11",
                "session": "legacy-v2",
                "workspace_path": "workspace.json",
                "started_at_unix_ms": 1
            }"#,
        )
        .unwrap();
        assert_eq!(record.schema_version, INSTANCE_SCHEMA_VERSION);
        assert!(record.process_start_identity.is_none());
        assert!(record.lease_nonce.is_none());
        assert!(record.server_epoch.is_none());
        assert!(!record.test_fixture);
    }

    #[test]
    fn cleanup_refuses_a_registration_generation_changed_after_discovery() {
        let directory = env::temp_dir().join(format!(
            "agenterm-instance-cleanup-race-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&directory).unwrap();
        let mut record = InstanceRecord {
            schema_version: INSTANCE_SCHEMA_VERSION,
            pid: u32::MAX,
            address: "127.0.0.1:49990".to_owned(),
            endpoint: Some(IpcEndpoint::from_legacy_address("127.0.0.1:49990").unwrap()),
            logical_instance: Some(LogicalInstance::Main),
            server_scope_id: Some(ServerScopeId::current(&LogicalInstance::Main).unwrap()),
            version: "test".to_owned(),
            session: "cleanup-race".to_owned(),
            workspace_path: "workspace.json".to_owned(),
            started_at_unix_ms: 1,
            process_start_identity: Some("dead-process".to_owned()),
            lease_nonce: Some("cleanup-race-original".to_owned()),
            server_epoch: Some("cleanup-race-epoch".to_owned()),
            test_fixture: true,
            upgrade_identity: None,
        };
        let path = registration_path(&directory, &record);
        fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
        let discovered = discover_instances_in(&directory).unwrap();
        assert_eq!(discovered.len(), 1);

        record.lease_nonce = Some("cleanup-race-replacement".to_owned());
        fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
        let receipt = cleanup_instance(&discovered[0]);
        assert_eq!(receipt.result, "identity_changed");
        assert!(path.exists());

        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn registration_is_discoverable_and_removed_on_drop() {
        let directory = env::temp_dir().join(format!(
            "agenterm-instance-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let record = InstanceRecord {
            schema_version: INSTANCE_SCHEMA_VERSION,
            pid: std::process::id(),
            address: "127.0.0.1:49999".to_owned(),
            endpoint: Some(IpcEndpoint::Tcp {
                host: "127.0.0.1".to_owned(),
                port: 49999,
            }),
            logical_instance: Some(LogicalInstance::Main),
            server_scope_id: Some(ServerScopeId::current(&LogicalInstance::Main).unwrap()),
            version: "test".to_owned(),
            session: "fleet".to_owned(),
            workspace_path: "workspace.json".to_owned(),
            started_at_unix_ms: 1,
            process_start_identity: Some(current_process_start_identity().unwrap()),
            lease_nonce: Some("test-registration-drop".to_owned()),
            server_epoch: Some("test-epoch".to_owned()),
            test_fixture: false,
            upgrade_identity: None,
        };
        let registration = register_instance_in(&directory, record).unwrap();
        let discovered = discover_instances_in(&directory).unwrap();
        assert_eq!(discovered.len(), 1);
        assert_eq!(discovered[0].record.address, "127.0.0.1:49999");
        drop(registration);
        assert!(discover_instances_in(&directory).unwrap().is_empty());
        fs::remove_dir(&directory).unwrap();
    }

    #[test]
    fn pruning_a_typed_tcp_registration_removes_its_matching_legacy_alias() {
        let directory = env::temp_dir().join(format!(
            "agenterm-instance-prune-alias-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let address = "127.0.0.1:49994";
        let record = InstanceRecord {
            schema_version: INSTANCE_SCHEMA_VERSION,
            pid: std::process::id(),
            address: address.to_owned(),
            endpoint: Some(IpcEndpoint::from_legacy_address(address).unwrap()),
            logical_instance: Some(LogicalInstance::Main),
            server_scope_id: Some(ServerScopeId::current(&LogicalInstance::Main).unwrap()),
            version: "test".to_owned(),
            session: "fleet".to_owned(),
            workspace_path: "workspace.json".to_owned(),
            started_at_unix_ms: 1,
            process_start_identity: Some("dead-process".to_owned()),
            lease_nonce: Some("test-registration-prune".to_owned()),
            server_epoch: Some("test-epoch".to_owned()),
            test_fixture: true,
            upgrade_identity: None,
        };
        let registration = register_instance_in(&directory, record).unwrap();
        let records = discover_instances_in(&directory).unwrap();
        assert_eq!(records.len(), 1);
        assert!(
            directory
                .join(format!("{}-v1.json", std::process::id()))
                .exists()
        );

        let receipt = cleanup_instance(&records[0]);
        assert_eq!(receipt.result, "deleted");
        assert_eq!(receipt.legacy_alias_result, "deleted");

        assert!(!registration.path.exists());
        assert!(
            !directory
                .join(format!("{}-v1.json", std::process::id()))
                .exists()
        );
        drop(registration);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn registration_replaces_only_a_dead_typed_record_for_the_same_authority() {
        let directory = env::temp_dir().join(format!(
            "agenterm-instance-takeover-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&directory).unwrap();
        let endpoint = IpcEndpoint::Tcp {
            host: "127.0.0.1".to_owned(),
            port: 49993,
        };
        let main_scope = ServerScopeId::current(&LogicalInstance::Main).unwrap();
        let dev_scope = ServerScopeId::current(&LogicalInstance::Dev).unwrap();
        let record = |pid, scope: ServerScopeId| InstanceRecord {
            schema_version: INSTANCE_SCHEMA_VERSION,
            pid,
            address: "127.0.0.1:49993".to_owned(),
            endpoint: Some(endpoint.clone()),
            logical_instance: Some(LogicalInstance::Main),
            server_scope_id: Some(scope),
            version: "test".to_owned(),
            session: "fleet".to_owned(),
            workspace_path: "workspace.json".to_owned(),
            started_at_unix_ms: u128::from(pid),
            process_start_identity: Some(format!("test-start-{pid}")),
            lease_nonce: Some(format!("test-nonce-{pid}")),
            server_epoch: Some(format!("test-epoch-{pid}")),
            test_fixture: true,
            upgrade_identity: None,
        };

        let stale = record(u32::MAX, main_scope.clone());
        fs::write(
            directory.join(format!("{}.json", stale.pid)),
            serde_json::to_vec(&stale).unwrap(),
        )
        .unwrap();
        let mut stale_alias = stale.clone();
        stale_alias.schema_version = LEGACY_INSTANCE_SCHEMA_VERSION;
        stale_alias.endpoint = None;
        stale_alias.logical_instance = None;
        stale_alias.server_scope_id = None;
        fs::write(
            directory.join(format!("{}-v1.json", stale.pid)),
            serde_json::to_vec(&stale_alias).unwrap(),
        )
        .unwrap();

        let mut live = record(std::process::id(), main_scope.clone());
        live.process_start_identity = Some(current_process_start_identity().unwrap());
        fs::write(
            directory.join(format!("{}.json", live.pid)),
            serde_json::to_vec(&live).unwrap(),
        )
        .unwrap();
        let unrelated = record(u32::MAX - 1, dev_scope);
        fs::write(
            directory.join(format!("{}.json", unrelated.pid)),
            serde_json::to_vec(&unrelated).unwrap(),
        )
        .unwrap();

        let incoming = record(u32::MAX - 2, main_scope);
        let registration = register_instance_in(&directory, incoming.clone()).unwrap();

        assert!(!directory.join(format!("{}.json", stale.pid)).exists());
        assert!(!directory.join(format!("{}-v1.json", stale.pid)).exists());
        assert!(directory.join(format!("{}.json", live.pid)).exists());
        assert!(directory.join(format!("{}.json", unrelated.pid)).exists());
        assert!(registration_path(&directory, &incoming).exists());

        drop(registration);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn process_liveness_only_rejects_a_definitively_dead_pid() {
        assert!(instance_process_is_alive(std::process::id()));
        assert!(!instance_process_is_alive(u32::MAX));
    }

    #[test]
    fn intentional_shutdown_marker_is_address_and_pid_scoped() {
        let directory = env::temp_dir().join(format!(
            "agenterm-shutdown-marker-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let address = "127.0.0.1:49998";
        mark_intentional_shutdown_in(&directory, address, None, None, 42).unwrap();
        let marker: IntentionalShutdown = serde_json::from_slice(
            &fs::read(intentional_shutdown_path(&directory, address)).unwrap(),
        )
        .unwrap();
        assert_eq!(marker.pid, 42);
        assert_eq!(marker.address, address);
        assert_ne!(
            intentional_shutdown_path(&directory, address),
            intentional_shutdown_path(&directory, "127.0.0.1:49999")
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn discovery_reads_legacy_and_typed_records_in_one_pass() {
        let directory = env::temp_dir().join(format!(
            "agenterm-mixed-instance-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("legacy.json"),
            br#"{
                "schema_version": 1,
                "pid": 41,
                "address": "127.0.0.1:49997",
                "version": "0.1.10",
                "session": "legacy",
                "workspace_path": "legacy.json",
                "started_at_unix_ms": 1
            }"#,
        )
        .unwrap();
        let scope = ServerScopeId::current(&LogicalInstance::Dev).unwrap();
        fs::write(
            directory.join("typed.json"),
            serde_json::to_vec(&InstanceRecord {
                schema_version: INSTANCE_SCHEMA_VERSION,
                pid: 42,
                address: r"pipe:\\.\pipe\agenterm-dev".to_owned(),
                endpoint: Some(IpcEndpoint::NamedPipe(r"\\.\pipe\agenterm-dev".to_owned())),
                logical_instance: Some(LogicalInstance::Dev),
                server_scope_id: Some(scope.clone()),
                version: "0.1.11".to_owned(),
                session: "dev".to_owned(),
                workspace_path: "dev.json".to_owned(),
                started_at_unix_ms: 2,
                process_start_identity: Some("test-start-42".to_owned()),
                lease_nonce: Some("test-nonce-42".to_owned()),
                server_epoch: Some("test-epoch-42".to_owned()),
                test_fixture: false,
                upgrade_identity: None,
            })
            .unwrap(),
        )
        .unwrap();
        fs::write(
            directory.join("typed-legacy-alias.json"),
            br#"{
                "schema_version": 1,
                "pid": 42,
                "address": "127.0.0.1:49996",
                "version": "0.1.10",
                "session": "dev",
                "workspace_path": "dev.json",
                "started_at_unix_ms": 2
            }"#,
        )
        .unwrap();

        let records = discover_instances_in(&directory).unwrap();
        assert_eq!(records.len(), 2);
        assert!(records.iter().any(|instance| {
            instance.record.schema_version == LEGACY_INSTANCE_SCHEMA_VERSION
                && instance.record.resolved_endpoint()
                    == Some(IpcEndpoint::Tcp {
                        host: "127.0.0.1".to_owned(),
                        port: 49997,
                    })
                && instance.record.resolved_logical_instance() == LogicalInstance::Main
        }));
        assert!(records.iter().any(|instance| {
            instance.record.schema_version == INSTANCE_SCHEMA_VERSION
                && instance.record.server_scope_id.as_ref() == Some(&scope)
                && instance.record.resolved_logical_instance() == LogicalInstance::Dev
        }));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn scope_marker_survives_endpoint_text_migration() {
        let directory = env::temp_dir().join(format!(
            "agenterm-scope-marker-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let scope = ServerScopeId::current(&LogicalInstance::Main).unwrap();
        mark_intentional_shutdown_in(
            &directory,
            "127.0.0.1:48815",
            Some(IpcEndpoint::Tcp {
                host: "127.0.0.1".to_owned(),
                port: 48815,
            }),
            Some(scope.clone()),
            42,
        )
        .unwrap();
        let marker: IntentionalShutdown = serde_json::from_slice(
            &fs::read(intentional_shutdown_scope_path(&directory, &scope)).unwrap(),
        )
        .unwrap();
        assert_eq!(marker.server_scope_id, Some(scope));
        assert_eq!(marker.pid, 42);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn implicit_main_migration_reuses_only_a_live_v1_default_authority() {
        use std::io::{BufRead as _, BufReader, Write as _};

        let directory = env::temp_dir().join(format!(
            "agenterm-v1-migration-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&directory).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let expected = IpcEndpoint::Tcp {
            host: address.ip().to_string(),
            port: address.port(),
        };
        let legacy = InstanceRecord {
            schema_version: LEGACY_INSTANCE_SCHEMA_VERSION,
            pid: std::process::id(),
            address: address.to_string(),
            endpoint: None,
            logical_instance: None,
            server_scope_id: None,
            version: "0.1.10".to_owned(),
            session: "legacy-main".to_owned(),
            workspace_path: "AgenTerm/workspace.json".to_owned(),
            started_at_unix_ms: 1,
            process_start_identity: None,
            lease_nonce: None,
            server_epoch: None,
            test_fixture: false,
            upgrade_identity: None,
        };
        fs::write(
            directory.join("legacy.json"),
            serde_json::to_vec(&legacy).unwrap(),
        )
        .unwrap();

        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut request)
                .unwrap();
            let request: IpcRequest = serde_json::from_str(&request).unwrap();
            assert_eq!(request.args, ["protocol-info"]);
            let response = IpcResponse::success(r#"{"protocol_version":1}"#);
            stream
                .write_all(serde_json::to_string(&response).unwrap().as_bytes())
                .unwrap();
            stream.write_all(b"\n").unwrap();
        });

        assert_eq!(
            discover_healthy_legacy_main_endpoint_in(&directory, &expected, Duration::from_secs(2))
                .unwrap(),
            Some(expected.clone())
        );
        server.join().unwrap();

        let other = IpcEndpoint::Tcp {
            host: "127.0.0.1".to_owned(),
            port: expected
                .legacy_address()
                .unwrap()
                .rsplit(':')
                .next()
                .unwrap()
                .parse::<u16>()
                .unwrap()
                .saturating_add(1),
        };
        assert_eq!(
            discover_healthy_legacy_main_endpoint_in(&directory, &other, Duration::from_millis(10))
                .unwrap(),
            None
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rollback_fixture_reports_native_server_as_unsupported_to_v1_tcp_clients() {
        let scope = ServerScopeId::current(&LogicalInstance::Main).unwrap();
        let endpoint = crate::platform::ipc_default_native_endpoint(&scope);
        let record = InstanceRecord {
            schema_version: INSTANCE_SCHEMA_VERSION,
            pid: std::process::id(),
            address: endpoint.to_string(),
            endpoint: Some(endpoint),
            logical_instance: Some(LogicalInstance::Main),
            server_scope_id: Some(scope),
            version: "0.1.11".to_owned(),
            session: "native-main".to_owned(),
            workspace_path: "AgenTerm/workspace.json".to_owned(),
            started_at_unix_ms: 1,
            process_start_identity: Some(current_process_start_identity().unwrap()),
            lease_nonce: Some("native-main-test".to_owned()),
            server_epoch: Some("native-main-epoch".to_owned()),
            test_fixture: false,
            upgrade_identity: None,
        };
        assert_eq!(
            record.legacy_client_compatibility(),
            "unsupported_no_legacy_listener"
        );
    }
}
