//! Stable multi-instance identity for window chrome and ui-snapshot (S2).

use crate::client::resolved_ipc_endpoint;
use crate::ipc_endpoint::{IpcEndpoint, LogicalInstance};

/// Public identity fields for title / status bar / `ui-snapshot.window`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct InstanceIdentity {
    pub instance: String,
    pub instance_label: String,
    pub endpoint: String,
    pub endpoint_short: String,
    pub server_pid: Option<u32>,
}

impl InstanceIdentity {
    pub(crate) fn from_resolved(
        logical: &LogicalInstance,
        endpoint: &IpcEndpoint,
        server_pid: Option<u32>,
    ) -> Self {
        let username = std::env::var("USERNAME")
            .or_else(|_| std::env::var("USER"))
            .unwrap_or_else(|_| "user".to_owned());
        let endpoint_s = endpoint.to_string();
        Self {
            instance: logical.canonical_name(),
            instance_label: logical.display_label(&username),
            endpoint_short: short_endpoint(&endpoint_s),
            endpoint: endpoint_s,
            server_pid,
        }
    }

    /// Best-effort from process env selectors; used when endpoint resolution works.
    pub(crate) fn from_process(server_pid: Option<u32>) -> Option<Self> {
        let resolved = resolved_ipc_endpoint().ok()?;
        Some(Self::from_resolved(
            &resolved.logical_instance,
            &resolved.endpoint,
            server_pid,
        ))
    }

    pub(crate) fn status_line(&self) -> String {
        match self.server_pid {
            Some(pid) => format!(
                "Connected · {} · server PID {pid} · {}",
                self.instance, self.endpoint_short
            ),
            None => format!("Connected · {} · {}", self.instance, self.endpoint_short),
        }
    }

    pub(crate) fn snapshot_window_fields(&self) -> serde_json::Value {
        serde_json::json!({
            "instance": self.instance,
            "instance_label": self.instance_label,
            "endpoint": self.endpoint,
            "endpoint_short": self.endpoint_short,
            "server_pid": self.server_pid,
        })
    }
}

fn short_endpoint(endpoint: &str) -> String {
    // pipe:\\.\pipe\agenterm-agt-v1-<hash> → agt-v1-<12>
    if let Some(idx) = endpoint.rfind("agenterm-") {
        let tail = &endpoint[idx + "agenterm-".len()..];
        if tail.len() > 16 {
            return format!("{}…", &tail[..16]);
        }
        return tail.to_owned();
    }
    if endpoint.len() > 24 {
        format!("{}…", &endpoint[..24])
    } else {
        endpoint.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc_endpoint::{IpcEndpoint, LogicalInstance};

    #[test]
    fn identity_fields_are_stable_for_main_and_work() {
        let main = InstanceIdentity::from_resolved(
            &LogicalInstance::Main,
            &IpcEndpoint::NamedPipe("agenterm-agt-v1-deadbeefcafebabe".to_owned()),
            Some(42),
        );
        assert_eq!(main.instance, "main");
        assert!(main.instance_label.contains("main"));
        assert_eq!(main.server_pid, Some(42));
        assert!(main.status_line().contains("main"));
        assert!(main.status_line().contains("42"));
        let snap = main.snapshot_window_fields();
        assert_eq!(snap["instance"], "main");
        assert_eq!(snap["server_pid"], 42);

        let work = InstanceIdentity::from_resolved(
            &LogicalInstance::Custom("work".to_owned()),
            &IpcEndpoint::NamedPipe("agenterm-agt-v1-0123456789abcdef".to_owned()),
            Some(99),
        );
        // Custom instances keep the product-stable canonical form.
        assert!(
            work.instance == "work" || work.instance == "custom:work",
            "unexpected work instance id {}",
            work.instance
        );
        assert_ne!(main.instance, work.instance);
        assert!(work.endpoint_short.contains("agt-v1") || work.endpoint_short.contains("…"));
    }

    #[test]
    fn short_endpoint_truncates_long_pipe_names() {
        let short = short_endpoint(r"pipe:\\.\pipe\agenterm-agt-v1-abcdefghijklmnopqrstuvwxyz");
        assert!(short.len() < 40);
        assert!(short.contains('…') || short.starts_with("agt-v1"));
    }
}
