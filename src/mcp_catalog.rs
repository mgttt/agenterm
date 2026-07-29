use std::io::Write;

use serde::Serialize;

pub const MCP_PROTOCOL_REVISION: &str = "2025-11-25";
pub const MCP_CATALOG_SCHEMA_VERSION: u32 = 1;
pub const MCP_RESOURCE_SCHEMA_VERSION: u32 = 1;
pub const MCP_TOOL_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum McpAvailability {
    Shipped,
    Planned,
    Deferred,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct McpMethod {
    pub name: &'static str,
    pub availability: McpAvailability,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct McpResource {
    pub stable_id: &'static str,
    pub uri: &'static str,
    pub schema_id: &'static str,
    pub availability: McpAvailability,
    pub content_bearing: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct McpTool {
    pub stable_id: &'static str,
    pub name: &'static str,
    pub schema_id: &'static str,
    pub availability: McpAvailability,
    pub read_only: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct McpLimits {
    pub frame_bytes: u32,
    pub request_concurrency: u16,
    pub waiter_concurrency: u16,
    pub wait_timeout_ms_default: u32,
    pub wait_timeout_ms_maximum: u32,
    pub error_detail_bytes: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct McpUnavailableRole {
    pub stable_id: &'static str,
    pub availability: McpAvailability,
    pub reason: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct McpCapabilities {
    pub catalog_schema_version: u32,
    pub product: &'static str,
    pub product_version: &'static str,
    pub protocol_revision: &'static str,
    pub resource_schema_version: u32,
    pub tool_schema_version: u32,
    pub transports: Vec<&'static str>,
    pub methods: Vec<McpMethod>,
    pub resources: Vec<McpResource>,
    pub tools: Vec<McpTool>,
    pub limits: McpLimits,
    pub unavailable_roles: Vec<McpUnavailableRole>,
}

pub fn capabilities() -> McpCapabilities {
    McpCapabilities {
        catalog_schema_version: MCP_CATALOG_SCHEMA_VERSION,
        product: "agenterm-mcp",
        product_version: env!("CARGO_PKG_VERSION"),
        protocol_revision: MCP_PROTOCOL_REVISION,
        resource_schema_version: MCP_RESOURCE_SCHEMA_VERSION,
        tool_schema_version: MCP_TOOL_SCHEMA_VERSION,
        transports: vec!["stdio"],
        methods: [
            ("initialize", McpAvailability::Planned),
            ("notifications/initialized", McpAvailability::Planned),
            ("ping", McpAvailability::Planned),
            ("resources/list", McpAvailability::Planned),
            ("resources/read", McpAvailability::Planned),
            ("tools/list", McpAvailability::Planned),
            ("tools/call", McpAvailability::Planned),
            ("notifications/cancelled", McpAvailability::Planned),
        ]
        .into_iter()
        .map(|(name, availability)| McpMethod { name, availability })
        .collect(),
        resources: vec![
            McpResource {
                stable_id: "fleet.instances",
                uri: "agenterm://fleet/instances",
                schema_id: "agenterm.mcp.resource.instances.v1",
                availability: McpAvailability::Planned,
                content_bearing: false,
            },
            McpResource {
                stable_id: "fleet.workspace",
                uri: "agenterm://fleet/workspace",
                schema_id: "agenterm.mcp.resource.workspace.v1",
                availability: McpAvailability::Planned,
                content_bearing: false,
            },
            McpResource {
                stable_id: "fleet.tabs",
                uri: "agenterm://fleet/tabs",
                schema_id: "agenterm.mcp.resource.tabs.v1",
                availability: McpAvailability::Planned,
                content_bearing: false,
            },
            McpResource {
                stable_id: "fleet.snapshot",
                uri: "agenterm://fleet/snapshot",
                schema_id: "agenterm.mcp.resource.fleet-snapshot.v1",
                availability: McpAvailability::Planned,
                content_bearing: false,
            },
        ],
        tools: vec![McpTool {
            stable_id: "fleet.wait",
            name: "agenterm_wait",
            schema_id: "agenterm.mcp.tool.wait.v1",
            availability: McpAvailability::Planned,
            read_only: true,
        }],
        limits: McpLimits {
            frame_bytes: 1_048_576,
            request_concurrency: 16,
            waiter_concurrency: 8,
            wait_timeout_ms_default: 5_000,
            wait_timeout_ms_maximum: 60_000,
            error_detail_bytes: 16_384,
        },
        unavailable_roles: vec![
            McpUnavailableRole {
                stable_id: "transport.network",
                availability: McpAvailability::Deferred,
                reason: "v0.1.10 accepts stdio only and opens no listener",
            },
            McpUnavailableRole {
                stable_id: "resource.pane-content",
                availability: McpAvailability::Deferred,
                reason: "terminal and Composer content are private by default",
            },
            McpUnavailableRole {
                stable_id: "tool.control",
                availability: McpAvailability::Deferred,
                reason: "the first delivery exposes no mutation tools",
            },
            McpUnavailableRole {
                stable_id: "role.client-federation",
                availability: McpAvailability::Deferred,
                reason: "MCP client and federation roles require a later approval",
            },
            McpUnavailableRole {
                stable_id: "role.agent-runtime",
                availability: McpAvailability::Deferred,
                reason: "brain, flow, scheduling and autonomous roles are out of scope",
            },
        ],
    }
}

pub fn run_mcp_entry_with_args(arguments: Vec<String>) -> i32 {
    match arguments.as_slice() {
        [argument] if argument == "--version" || argument == "-V" => {
            println!("agenterm-mcp {}", env!("CARGO_PKG_VERSION"));
            0
        }
        [] => {
            print_help();
            0
        }
        [argument] if argument == "--help" || argument == "-h" => {
            print_help();
            0
        }
        [command, format] if command == "capabilities" && format == "--json" => {
            let stdout = std::io::stdout();
            let mut output = stdout.lock();
            if serde_json::to_writer_pretty(&mut output, &capabilities()).is_err()
                || output.write_all(b"\n").is_err()
            {
                eprintln!("mcp_output_failed: could not write capabilities");
                return 2;
            }
            0
        }
        [command] if command == "capabilities" => {
            eprintln!("mcp_capabilities_format: use `capabilities --json`");
            2
        }
        [command, transport] if command == "serve" && transport == "--stdio" => {
            eprintln!(
                "mcp_transport_unavailable: stdio lifecycle is cataloged but not implemented"
            );
            2
        }
        _ => {
            eprintln!("unknown agenterm-mcp command; use --help");
            2
        }
    }
}

fn print_help() {
    println!(
        "AgenTerm MCP read-only sidecar\n\
         \n\
         Usage:\n\
           agenterm-mcp --help\n\
           agenterm-mcp --version\n\
           agenterm-mcp capabilities --json\n\
           agenterm-mcp serve --stdio\n\
         \n\
         Only offline discovery is shipped in this implementation slice.\n\
         No network listener or mutation tool is available."
    );
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn first_delivery_catalog_is_read_only_and_bounded() {
        let catalog = capabilities();
        assert_eq!(catalog.protocol_revision, "2025-11-25");
        assert_eq!(catalog.transports, vec!["stdio"]);
        assert_eq!(catalog.resources.len(), 4);
        assert!(catalog.resources.iter().all(|item| !item.content_bearing));
        assert_eq!(catalog.tools.len(), 1);
        assert_eq!(catalog.tools[0].name, "agenterm_wait");
        assert!(catalog.tools[0].read_only);
        assert!(catalog.limits.frame_bytes > 0);
        assert!(catalog.limits.wait_timeout_ms_default <= catalog.limits.wait_timeout_ms_maximum);
    }

    #[test]
    fn catalog_stable_ids_uris_and_method_names_are_unique() {
        let catalog = capabilities();
        assert_eq!(
            catalog
                .methods
                .iter()
                .map(|item| item.name)
                .collect::<HashSet<_>>()
                .len(),
            catalog.methods.len()
        );
        assert_eq!(
            catalog
                .resources
                .iter()
                .map(|item| item.stable_id)
                .collect::<HashSet<_>>()
                .len(),
            catalog.resources.len()
        );
        assert_eq!(
            catalog
                .resources
                .iter()
                .map(|item| item.uri)
                .collect::<HashSet<_>>()
                .len(),
            catalog.resources.len()
        );
        assert_eq!(
            catalog
                .tools
                .iter()
                .map(|item| item.stable_id)
                .collect::<HashSet<_>>()
                .len(),
            catalog.tools.len()
        );
    }

    #[test]
    fn implementation_slice_does_not_overclaim_protocol_methods() {
        let catalog = capabilities();
        assert!(
            catalog
                .methods
                .iter()
                .all(|method| method.availability == McpAvailability::Planned)
        );
        assert!(
            catalog
                .resources
                .iter()
                .all(|resource| resource.availability == McpAvailability::Planned)
        );
        assert_eq!(catalog.tools[0].availability, McpAvailability::Planned);
        assert!(
            catalog
                .unavailable_roles
                .iter()
                .all(|role| role.availability == McpAvailability::Deferred)
        );
    }
}
