//! Frontend-side bootstrap and recovery for the independent AgenTerm server.
//!
//! Not a second server and not an IPC proxy. Product GUI/CLI clients use this
//! module to detect the control endpoint, spawn `agenterm server` when absent,
//! and apply limited reconnect restart policy. Session truth stays in
//! `server_app`; live UI I/O stays on the direct client↔server path after connect.

use crate::instances::intentional_shutdown_matches;
use crate::ui_client::UiClientModel;
use std::time::{Duration, Instant};

pub(crate) const GUI_FRONTEND_SERVER_START_TIMEOUT: Duration = Duration::from_secs(8);
pub(crate) const GUI_FRONTEND_SERVER_RESTART_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy)]
pub(crate) struct FrontendServerRecoveryState {
    server_restart_after: Instant,
    server_restart_suppressed: bool,
}

impl FrontendServerRecoveryState {
    pub(crate) fn new(now: Instant) -> Self {
        Self {
            server_restart_after: now,
            server_restart_suppressed: false,
        }
    }

    pub(crate) fn on_disconnected(&mut self, address: &str, server_pid: Option<u32>) {
        if server_pid.is_some_and(|pid| intentional_shutdown_matches(address, pid)) {
            self.server_restart_suppressed = true;
        }
    }

    pub(crate) fn on_reconnected(&mut self, now: Instant) {
        self.server_restart_after = now;
        self.server_restart_suppressed = false;
    }

    pub(crate) fn maybe_recover(&mut self, now: Instant) -> FrontendServerRecovery {
        let recovery = maybe_recover_frontend_server(
            now,
            self.server_restart_after,
            self.server_restart_suppressed,
        );
        if matches!(
            recovery,
            FrontendServerRecovery::Started | FrontendServerRecovery::Failed(_)
        ) {
            self.server_restart_after = next_frontend_server_restart_after(now);
        }
        recovery
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FrontendServerRecovery {
    NoAction,
    Started,
    Failed(String),
}

impl FrontendServerRecovery {
    #[allow(dead_code)]
    pub(crate) fn contract_state(self) -> crate::frontend::FrontendContractState {
        match self {
            Self::NoAction | Self::Started => crate::frontend::FrontendContractState::Supported,
            Self::Failed(_) => crate::frontend::FrontendContractState::Failed,
        }
    }
}

pub(crate) fn connect_or_start_frontend_gui_client(
    client_id: &str,
) -> Result<UiClientModel, String> {
    match UiClientModel::connect(client_id.to_owned()) {
        Ok(client) => return Ok(client),
        Err(error) => {
            // Endpoint is up but rejected this UI client — do not spawn a twin.
            if is_frontend_server_endpoint_listening() {
                return Err(format!("running server rejected replaceable UI: {error}"));
            }
        }
    }

    // Resolved default pipe/socket is down. Before spawning a second authority
    // (which would re-spawn empty shells from workspace.json and look like a
    // full session reset), attach any live registration for this logical
    // instance (e.g. another `main` still holding agent tabs after Keep Server).
    if let Some(endpoint) = discover_live_peer_endpoint()? {
        pin_client_endpoint(&endpoint);
        match UiClientModel::connect(client_id.to_owned()) {
            Ok(client) => return Ok(client),
            Err(error) => {
                return Err(format!(
                    "live AgenTerm server for this instance is reachable at {endpoint} but UI connect failed: {error}"
                ));
            }
        }
    }

    start_frontend_server_process()?;
    let deadline = Instant::now() + GUI_FRONTEND_SERVER_START_TIMEOUT;
    let mut last_error = None;
    while Instant::now() < deadline {
        match UiClientModel::connect(client_id.to_owned()) {
            Ok(client) => return Ok(client),
            Err(error) => last_error = Some(format!("{error}")),
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err(format!(
        "could not start independent AgenTerm server: {}",
        last_error.unwrap_or_else(|| "server did not become ready".to_owned())
    ))
}

fn discover_live_peer_endpoint() -> Result<Option<crate::ipc_endpoint::IpcEndpoint>, String> {
    let resolved = crate::client::resolved_ipc_endpoint().map_err(|error| error.to_string())?;
    crate::instances::find_live_endpoint_for_logical_instance(
        &resolved.logical_instance,
        Some(&resolved.endpoint),
        Duration::from_millis(250),
    )
    .map_err(|error| error.to_string())
}

/// Pin the process IPC selector so subsequent control calls target the live peer.
fn pin_client_endpoint(endpoint: &crate::ipc_endpoint::IpcEndpoint) {
    // Keep current logical-instance identity when only the endpoint peer changes.
    let instance = crate::client::resolved_ipc_endpoint()
        .ok()
        .map(|resolved| resolved.logical_instance.canonical_name());
    let _ = pin_client_peer_for_gui(endpoint, instance.as_deref());
}

/// Pin GUI/CLI process selectors to a discovered peer endpoint.
///
/// Must update [`crate::client::set_ipc_selectors`] (not only env vars): launch
/// `--instance` lives in `IPC_SELECTOR_OVERRIDE` and would otherwise win forever.
/// When `instance` is provided it is kept as logical identity even though the
/// authority is the explicit endpoint (endpoint-only used to force `main`).
pub(crate) fn pin_client_peer_for_gui(
    endpoint: &crate::ipc_endpoint::IpcEndpoint,
    instance: Option<&str>,
) -> Result<(), String> {
    use crate::ipc_endpoint::EndpointSelectorArgs;
    let selectors = EndpointSelectorArgs {
        endpoint: Some(endpoint.to_string()),
        address: None,
        instance: instance.map(str::to_owned),
    };
    crate::client::set_ipc_selectors(selectors).map_err(|error| error.to_string())?;
    // SAFETY: single-threaded at GUI bootstrap / UI-action attach before races.
    unsafe {
        std::env::set_var("AGENTERM_IPC_ENDPOINT", endpoint.to_string());
        std::env::remove_var("AGENTERM_IPC_ADDRESS");
        if let Some(instance) = instance {
            std::env::set_var("AGENTERM_INSTANCE", instance);
        } else {
            std::env::remove_var("AGENTERM_INSTANCE");
        }
    }
    Ok(())
}

/// Back-compat name for peer pin without identity change.
pub(crate) fn pin_client_endpoint_for_gui(endpoint: &crate::ipc_endpoint::IpcEndpoint) {
    let _ = pin_client_peer_for_gui(endpoint, None);
}

pub(crate) fn maybe_recover_frontend_server(
    now: Instant,
    server_restart_after: Instant,
    server_restart_suppressed: bool,
) -> FrontendServerRecovery {
    if server_restart_suppressed
        || now < server_restart_after
        || is_frontend_server_endpoint_listening()
    {
        return FrontendServerRecovery::NoAction;
    }

    // A peer under the same logical instance may still be live on another
    // endpoint (historical dual-main). Prefer pinning to it over spawning.
    if let Ok(Some(endpoint)) = discover_live_peer_endpoint() {
        pin_client_endpoint(&endpoint);
        if is_frontend_server_endpoint_listening() {
            return FrontendServerRecovery::NoAction;
        }
    }

    match start_frontend_server_process() {
        Ok(()) => FrontendServerRecovery::Started,
        Err(error) => FrontendServerRecovery::Failed(error),
    }
}

pub(crate) fn next_frontend_server_restart_after(now: Instant) -> Instant {
    now + GUI_FRONTEND_SERVER_RESTART_INTERVAL
}

pub(crate) fn is_frontend_server_endpoint_listening() -> bool {
    crate::client::ipc_endpoint().is_ok_and(|endpoint| {
        crate::ipc_transport::IpcStream::connect(&endpoint, Duration::from_millis(100)).is_ok()
    })
}

pub(crate) fn start_frontend_server_process() -> Result<(), String> {
    let resolved = crate::client::resolved_ipc_endpoint().map_err(|error| error.to_string())?;
    // Never mint a second Fleet authority while one already answers protocol
    // for this logical instance — that path is what wipes agent tabs via
    // workspace re-spawn of empty shells.
    if let Some(endpoint) = crate::instances::find_live_endpoint_for_logical_instance(
        &resolved.logical_instance,
        Some(&resolved.endpoint),
        Duration::from_millis(250),
    )
    .map_err(|error| error.to_string())?
    {
        pin_client_endpoint(&endpoint);
        return Err(format!(
            "refusing to start a second AgenTerm server: live instance `{}` already at {endpoint}. \
             Reopen attaches to that server; stop it explicitly if you need a clean slate.",
            resolved.logical_instance.canonical_name()
        ));
    }
    let parameter = frontend_server_spawn_parameter(&resolved);
    let started = crate::platform::process::autostart_server(parameter.0, &parameter.1)
        .map_err(|error| error.to_string())?;
    if !started {
        Err("independent AgenTerm server autostart is unavailable".to_owned())
    } else {
        Ok(())
    }
}

// The spawned server re-resolves its own identity from the one CLI selector
// it receives, and an endpoint selector cannot carry a logical-instance name
// (the scope hash is one-way, and a CLI selector suppresses inherited
// environment selectors by design). Passing the scope-default native endpoint
// as `--endpoint` therefore registered every autostarted server as `main`
// even when the client was launched with `--instance custom:...`. Hand the
// instance name across the spawn boundary whenever the endpoint is exactly
// the one derived from it; the child re-derives the identical endpoint.
pub(crate) fn frontend_server_spawn_parameter(
    resolved: &crate::ipc_endpoint::ResolvedIpcEndpoint,
) -> (&'static str, String) {
    if let Some(address) = resolved.endpoint.legacy_address() {
        ("--address", address)
    } else if resolved.endpoint
        == crate::platform::ipc::default_native_endpoint(&resolved.server_scope_id)
    {
        ("--instance", resolved.logical_instance.canonical_name())
    } else {
        ("--endpoint", resolved.endpoint.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        FrontendServerRecovery, GUI_FRONTEND_SERVER_RESTART_INTERVAL,
        next_frontend_server_restart_after,
    };
    use std::time::{Duration, Instant};

    #[test]
    fn spawn_parameter_carries_the_instance_name_for_scope_default_endpoints() {
        let logical_instance = crate::ipc_endpoint::LogicalInstance::Custom("work".to_owned());
        let server_scope_id = crate::ipc_endpoint::ServerScopeId::current(&logical_instance)
            .expect("current OS-user scope");
        let resolved = crate::ipc_endpoint::ResolvedIpcEndpoint {
            endpoint: crate::platform::ipc::default_native_endpoint(&server_scope_id),
            logical_instance,
            server_scope_id,
            explicit: true,
        };
        assert_eq!(
            super::frontend_server_spawn_parameter(&resolved),
            ("--instance", "custom:work".to_owned())
        );
    }

    #[test]
    fn spawn_parameter_preserves_explicit_endpoint_and_legacy_address_authority() {
        let logical_instance = crate::ipc_endpoint::LogicalInstance::Main;
        let server_scope_id = crate::ipc_endpoint::ServerScopeId::current(&logical_instance)
            .expect("current OS-user scope");
        let custom = crate::ipc_endpoint::ResolvedIpcEndpoint {
            endpoint: crate::ipc_endpoint::IpcEndpoint::NamedPipe("agenterm-custom".to_owned()),
            logical_instance: logical_instance.clone(),
            server_scope_id: server_scope_id.clone(),
            explicit: true,
        };
        let (custom_name, custom_value) = super::frontend_server_spawn_parameter(&custom);
        assert_eq!(custom_name, "--endpoint");
        assert!(custom_value.contains("agenterm-custom"));
        let legacy = crate::ipc_endpoint::ResolvedIpcEndpoint {
            endpoint: crate::ipc_endpoint::IpcEndpoint::Tcp {
                host: "127.0.0.1".to_owned(),
                port: 48815,
            },
            logical_instance,
            server_scope_id,
            explicit: true,
        };
        assert_eq!(
            super::frontend_server_spawn_parameter(&legacy),
            ("--address", "127.0.0.1:48815".to_owned())
        );
    }

    #[test]
    fn recovery_delay_is_computable() {
        let now = Instant::now();
        let later = next_frontend_server_restart_after(now);
        assert_eq!(
            later
                .checked_duration_since(now)
                .unwrap_or_else(|| Duration::from_millis(0)),
            GUI_FRONTEND_SERVER_RESTART_INTERVAL
        );
    }

    #[test]
    fn recovery_contract_state_maps_expected_states() {
        assert!(matches!(
            FrontendServerRecovery::NoAction.contract_state(),
            crate::frontend::FrontendContractState::Supported
        ));
        assert!(matches!(
            FrontendServerRecovery::Started.contract_state(),
            crate::frontend::FrontendContractState::Supported
        ));
        assert!(matches!(
            FrontendServerRecovery::Failed("x".to_owned()).contract_state(),
            crate::frontend::FrontendContractState::Failed
        ));
    }
}
