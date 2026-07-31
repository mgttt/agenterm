use std::{
    env, fs,
    io::{BufReader, Write as _},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

use crate::{
    build_identity::BuildIdentity,
    ipc_endpoint::{IpcEndpoint, LogicalInstance, ServerScopeId},
    ipc_transport::{IPC_RESPONSE_MAX_BYTES, IpcStream, read_bounded_ipc_line},
    protocol::{IpcRequest, IpcResponse},
    upgrade_identity::UpgradeIdentity,
};

const LEGACY_INSTANCE_SCHEMA_VERSION: u32 = 1;
const INSTANCE_SCHEMA_VERSION: u32 = 2;

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

pub(crate) struct InstanceRegistration {
    path: PathBuf,
    legacy_alias_path: Option<PathBuf>,
    pid: u32,
    address: String,
    endpoint: IpcEndpoint,
    server_scope_id: ServerScopeId,
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
            });
        if matches_registration {
            let _ = fs::remove_file(&self.path);
            if let Some(path) = &self.legacy_alias_path {
                let _ = fs::remove_file(path);
            }
        }
    }
}

#[cfg_attr(windows, allow(dead_code))]
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
    )
}

pub(crate) fn register_typed_instance(
    endpoint: IpcEndpoint,
    logical_instance: LogicalInstance,
    server_scope_id: ServerScopeId,
    workspace_path: &Path,
    session: &str,
) -> Result<InstanceRegistration> {
    let build = BuildIdentity::current();
    let address = endpoint
        .legacy_address()
        .unwrap_or_else(|| endpoint.to_string());
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

pub(crate) fn prune_instance(instance: &DiscoveredInstance) -> Result<()> {
    match fs::remove_file(&instance.path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => {
            Err(error).with_context(|| format!("failed to prune {}", instance.path.display()))
        }
    }
}

#[cfg(windows)]
pub(crate) fn instance_process_is_alive(pid: u32) -> bool {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, ERROR_INVALID_PARAMETER, GetLastError, STILL_ACTIVE},
        System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    };

    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return unsafe { GetLastError() } != ERROR_INVALID_PARAMETER;
    }
    let mut exit_code = 0;
    let queried = unsafe { GetExitCodeProcess(process, &mut exit_code) } != 0;
    unsafe {
        CloseHandle(process);
    }
    !queried || exit_code == STILL_ACTIVE as u32
}

#[cfg(unix)]
pub(crate) fn instance_process_is_alive(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    if pid <= 0 {
        return false;
    }
    // kill(pid, 0) returns 0 when the process exists and we may signal it.
    unsafe { libc::kill(pid, 0) == 0 }
}

fn instances_dir() -> PathBuf {
    if let Some(path) = env::var_os("AGENTERM_INSTANCE_DIR").filter(|path| !path.is_empty()) {
        return PathBuf::from(path);
    }
    #[cfg(windows)]
    {
        env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(env::temp_dir)
            .join("AgenTerm")
            .join("instances")
    }
    #[cfg(not(windows))]
    {
        env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(env::temp_dir)
            .join(".local")
            .join("share")
            .join("agenterm")
            .join("instances")
    }
}

fn register_instance_in(directory: &Path, record: InstanceRecord) -> Result<InstanceRegistration> {
    fs::create_dir_all(directory)
        .with_context(|| format!("failed to create {}", directory.display()))?;
    let path = directory.join(format!("{}.json", record.pid));
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
    })
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
        #[cfg(windows)]
        let endpoint = IpcEndpoint::NamedPipe(r"\\.\pipe\agenterm-migration-fixture".to_owned());
        #[cfg(unix)]
        let endpoint = IpcEndpoint::UnixSocket("/tmp/agenterm-migration-fixture.sock".to_owned());
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
            upgrade_identity: None,
        };
        assert_eq!(
            record.legacy_client_compatibility(),
            "unsupported_no_legacy_listener"
        );
    }
}
