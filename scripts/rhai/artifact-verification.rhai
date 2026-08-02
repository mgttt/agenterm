// Verify staged executable roles, budgets, provenance, and offline catalogs.
//
// Args:
//   REPO [release|development]

import "scripts/rhai/lib/artifact_manifest" as artifact_manifest;

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

fn offline_endpoint() {
    `pipe:\\.\pipe\agenterm-artifact-verification-` + std::process::id()
}

fn output(program, arguments, repo, code) {
    let command = std::process::command(program);
    command.args(arguments);
    command.current_dir(repo);
    command.timeout(std::time::Duration::from_secs(30));
    command.capture_limit(262144);
    let offline_id = "agenterm-artifact-verification-" + std::process::id();
    command.env("AGENTERM_IPC_ENDPOINT", offline_endpoint());
    command.env_remove("AGENTERM_IPC_ADDRESS");
    command.env(
        "AGENTERM_INSTANCE_DIR",
        std::path::join(repo, "target/" + offline_id + "/instances").display
    );
    command.env(
        "AGENTERM_WORKSPACE_PATH",
        std::path::join(repo, "target/" + offline_id + "/workspace.json").display
    );
    command.env("AGENTERM_NO_ACTIVATE", "1");
    let result = command.output();
    require(
        result.success,
        code + ":" + result.stdout_text() + result.stderr_text()
    );
    let text = result.stdout_text();
    text.trim();
    text
}

fn u16_le(bytes, offset) {
    require(offset + 1 < bytes.len, "artifact_pe_u16_bounds");
    bytes.get(offset) + bytes.get(offset + 1) * 256
}

fn u32_le(bytes, offset) {
    require(offset + 3 < bytes.len, "artifact_pe_u32_bounds");
    bytes.get(offset)
        + bytes.get(offset + 1) * 256
        + bytes.get(offset + 2) * 65536
        + bytes.get(offset + 3) * 16777216
}

fn pe_subsystem(path) {
    let bytes = std::fs::read(path);
    require(bytes.len >= 64, "artifact_pe_too_small:" + path);
    let pe_offset = u32_le(bytes, 0x3c);
    require(
        pe_offset + 24 + 69 < bytes.len,
        "artifact_pe_header_bounds:" + path
    );
    u16_le(bytes, pe_offset + 24 + 68)
}

fn find_metadata(metadata, name) {
    let found = ();
    for entry in metadata.executables {
        if entry.name == name {
            require(found == (), "artifact_metadata_duplicate:" + name);
            found = entry;
        }
    }
    found
}

require(
    args.len == 1 || args.len == 2,
    "expected: REPO [release|development]"
);
let repo = std::path::absolute(args[0]).display;
let release = args.len == 2 && args[1] == "release";
if args.len == 2 {
    require(
        args[1] == "release" || args[1] == "development",
        "artifact_verification_mode"
    );
}
let dist = std::path::join(repo, "dist").display;
let manifest_path =
    std::path::join(repo, "scripts/artifacts.json").display;
let manifest = artifact_manifest::load(manifest_path);
let metadata_path = std::path::join(dist, "agenterm.json").display;
let metadata = rhai::json::parse_file(metadata_path);
require(
    metadata.schema_version == 2,
    "artifact_metadata_schema"
);

let expected_names = [];
for artifact in manifest.executables {
    expected_names.push(artifact.name);
    let path = std::path::join(dist, artifact.name).display;
    require(std::fs::exists(path), "artifact_missing:" + artifact.name);
    require(
        pe_subsystem(path) == artifact.pe_subsystem,
        "artifact_pe_subsystem:" + artifact.name
    );
    if release {
        let actual_bytes = std::fs::metadata(path).len;
        require(
            actual_bytes <= artifact.release_budget_bytes,
            "artifact_release_budget:" + artifact.name +
                ":actual=" + actual_bytes +
                ":budget=" + artifact.release_budget_bytes
        );
    }
    let entry = find_metadata(metadata, artifact.name);
    require(
        entry != ()
            && entry.sha256 == rhai::crypto::sha256_file(path),
        "artifact_metadata_hash:" + artifact.name
    );
}
for entry in std::fs::read_dir(dist) {
    let name = entry.file_name.to_lower();
    require(
        !(
            entry.is_file
                && name.starts_with("agentermctl")
                && name.ends_with(".exe")
        ),
        "artifact_obsolete_cli:" + entry.file_name
    );
}
require(
    metadata.executables.len == expected_names.len,
    "artifact_metadata_count"
);
for entry in metadata.executables {
    require(
        expected_names.contains(entry.name),
        "artifact_metadata_unexpected:" + entry.name
    );
}
for feature in ["codex-launcher", "tab-environment", "mux-frontend"] {
    require(
        metadata.features.contains(feature),
        "artifact_metadata_feature:" + feature
    );
}
let head = output("git", ["rev-parse", "HEAD"], repo, "artifact_git_head");
require(metadata.git_commit == head, "artifact_metadata_commit");
require(
    metadata.cargo_lock_sha256
        == rhai::crypto::sha256_file(
            std::path::join(repo, "Cargo.lock").display
        ),
    "artifact_metadata_cargo_lock"
);
require(
    metadata.artifact_manifest_sha256
        == rhai::crypto::sha256_file(manifest_path),
    "artifact_metadata_manifest"
);
require(metadata.rust_version != "", "artifact_metadata_rust_version");
require(!release || !metadata.git_dirty, "artifact_release_dirty");

let cli = std::path::join(dist, "agenterm-cli.exe").display;
let mux = std::path::join(dist, "agenterm-mux.exe").display;
let rhai = std::path::join(dist, "agenterm-rhai.exe").display;
let mcp = std::path::join(dist, "agenterm-mcp.exe").display;
let control_center =
    std::path::join(dist, "agenterm-cc.exe").display;
require(
    output(rhai, ["--version"], repo, "artifact_rhai_version")
        == "agenterm-rhai " + metadata.version,
    "artifact_rhai_version_mismatch"
);
require(
    output(cli, ["--version"], repo, "artifact_cli_version")
        == "agenterm-cli " + metadata.version,
    "artifact_cli_version_mismatch"
);
require(
    output(mux, ["--version"], repo, "artifact_mux_version")
        == "agenterm-mux " + metadata.version +
            " (AgenTerm compatibility frontend)",
    "artifact_mux_version_mismatch"
);
require(
    output(mcp, ["--version"], repo, "artifact_mcp_version")
        == "agenterm-mcp " + metadata.version,
    "artifact_mcp_version_mismatch"
);
require(
    output(
        control_center,
        ["--version"],
        repo,
        "artifact_control_center_version"
    ) == "agenterm-cc " + metadata.version,
    "artifact_control_center_version_mismatch"
);
let control_center_capabilities = rhai::json::parse(
    output(
        control_center,
        ["capabilities", "--json"],
        repo,
        "artifact_control_center_capabilities"
    )
);
require(
    control_center_capabilities.schema_version == 1
        && control_center_capabilities.executable == "agenterm-cc"
        && control_center_capabilities.role == "isolated_projection"
        && !control_center_capabilities.owns_terminal_authority
        && control_center_capabilities.process_reuse
        && control_center_capabilities.no_activate
        && control_center_capabilities.views.len == 4,
    "artifact_control_center_capabilities_invalid"
);
let control_center_snapshot = rhai::json::parse(
    output(
        control_center,
        ["snapshot", "--json", "--endpoint", offline_endpoint()],
        repo,
        "artifact_control_center_snapshot"
    )
);
require(
    control_center_snapshot.schema_version == 1
        && control_center_snapshot.executable == "agenterm-cc"
        && control_center_snapshot.connected_server == ()
        && control_center_snapshot.views.len == 4,
    "artifact_control_center_snapshot_invalid"
);
let mcp_capabilities = rhai::json::parse(
    output(
        mcp,
        ["capabilities", "--json"],
        repo,
        "artifact_mcp_capabilities"
    )
);
require(
    mcp_capabilities.protocol_revision == "2025-11-25"
        && mcp_capabilities.transports.len == 1
        && mcp_capabilities.transports[0] == "stdio"
        && mcp_capabilities.resources.len == 4
        && mcp_capabilities.tools.len == 1
        && mcp_capabilities.tools[0].name == "agenterm_wait"
        && mcp_capabilities.tools[0].read_only,
    "artifact_mcp_capabilities_invalid"
);

print(
    "PASS: verified " + expected_names.len +
        " staged executable roles, hashes, budgets, and offline catalogs"
);
