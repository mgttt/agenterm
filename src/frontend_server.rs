//! Frontend-side bootstrap and recovery for the independent AgenTerm server.
//!
//! Not a second server and not an IPC proxy. Product GUI/CLI clients use this
//! module to detect the control endpoint, spawn `agenterm-server` when absent,
//! and apply limited reconnect restart policy. Session truth stays in
//! `server_app`; live UI I/O stays on the direct client↔server path after connect.

use crate::client::ipc_endpoint;
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
            if is_frontend_server_endpoint_listening() {
                return Err(format!("running server rejected replaceable UI: {error}"));
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
    let endpoint = ipc_endpoint().map_err(|error| error.to_string())?;
    let parameter = if let Some(address) = endpoint.legacy_address() {
        ("--address", address)
    } else {
        ("--endpoint", endpoint.to_string())
    };
    let started = crate::platform::process::autostart_server(parameter.0, &parameter.1)
        .map_err(|error| error.to_string())?;
    if !started {
        Err("independent AgenTerm server autostart is unavailable".to_owned())
    } else {
        Ok(())
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
