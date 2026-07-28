use serde::Serialize;

pub const UI_BRIDGE_SCHEMA_VERSION: u32 = 1;
pub const UI_BRIDGE_PROTOCOL_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UiOwnershipMode {
    CombinedGuiServer,
    SplitServerClient,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct UiProtocolRange {
    pub minimum: u32,
    pub maximum: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UiCompatibility {
    Compatible,
    ClientTooOld,
    ClientTooNew,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct UiBridgeFacts {
    pub schema_version: u32,
    pub protocol_version: u32,
    pub compatible_client_range: UiProtocolRange,
    pub ownership_mode: UiOwnershipMode,
    pub replaceable_ui: bool,
    pub server_executable: &'static str,
    pub target_server_executable: &'static str,
    pub bootstrap_snapshot: bool,
    pub ordered_deltas: bool,
    pub reconnect: bool,
    pub rollback_proven: bool,
}

pub const fn current_facts() -> UiBridgeFacts {
    UiBridgeFacts {
        schema_version: UI_BRIDGE_SCHEMA_VERSION,
        protocol_version: UI_BRIDGE_PROTOCOL_VERSION,
        compatible_client_range: UiProtocolRange {
            minimum: UI_BRIDGE_PROTOCOL_VERSION,
            maximum: UI_BRIDGE_PROTOCOL_VERSION,
        },
        ownership_mode: UiOwnershipMode::CombinedGuiServer,
        replaceable_ui: false,
        server_executable: "agenterm.exe",
        target_server_executable: "agenterm-server.exe",
        bootstrap_snapshot: false,
        ordered_deltas: false,
        reconnect: false,
        rollback_proven: false,
    }
}

pub const fn negotiate(client: UiProtocolRange, server_version: u32) -> UiCompatibility {
    if server_version < client.minimum {
        UiCompatibility::ClientTooNew
    } else if server_version > client.maximum {
        UiCompatibility::ClientTooOld
    } else {
        UiCompatibility::Compatible
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compatibility_is_explicit_in_both_directions() {
        let client = UiProtocolRange {
            minimum: 2,
            maximum: 4,
        };
        assert_eq!(negotiate(client, 1), UiCompatibility::ClientTooNew);
        assert_eq!(negotiate(client, 2), UiCompatibility::Compatible);
        assert_eq!(negotiate(client, 4), UiCompatibility::Compatible);
        assert_eq!(negotiate(client, 5), UiCompatibility::ClientTooOld);
    }

    #[test]
    fn current_facts_do_not_claim_the_planned_split() {
        let facts = current_facts();
        assert_eq!(facts.ownership_mode, UiOwnershipMode::CombinedGuiServer);
        assert!(!facts.replaceable_ui);
        assert!(!facts.reconnect);
        assert_eq!(facts.server_executable, "agenterm.exe");
        assert_eq!(facts.target_server_executable, "agenterm-server.exe");
    }
}
