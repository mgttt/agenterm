// Validate the isolated agenterm-net research workspace without linking it
// into the AgenTerm product graph or stable artifact manifest.
//
// Args: REPO

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

fn cargo(research, arguments, timeout_seconds, code) {
    let command = std::process::command("cargo");
    command.args(arguments);
    command.current_dir(research);
    command.env("CARGO_TERM_COLOR", "never");
    command.timeout(std::time::Duration::from_secs(timeout_seconds));
    command.capture_limit(262144);
    let output = command.output();
    require(
        output.success,
        code + ":\n" + output.stdout_text() + output.stderr_text()
    );
    output
}

require(args.len == 1, "expected: REPO");
let repo = std::path::absolute(args[0]).display;
let research = std::path::join(repo, "research/agenterm-net").display;
require(
    std::fs::exists(std::path::join(research, "Cargo.lock").display),
    "agenterm_net_lock_missing"
);

cargo(
    research,
    ["clippy", "--locked", "--all-targets", "--", "-D", "warnings"],
    300,
    "agenterm_net_clippy"
);
cargo(
    research,
    ["test", "--locked", "--quiet"],
    300,
    "agenterm_net_tests"
);
let self_test = cargo(
    research,
    ["run", "--locked", "--quiet", "--", "self-test", "--json"],
    120,
    "agenterm_net_self_test"
);
let receipt = rhai::json::parse(self_test.stdout_text());
require(receipt.schema == "agenterm-net/result/v1", "agenterm_net_schema");
require(
    receipt.state == "complete" && receipt.result.status == "research-pass",
    "agenterm_net_state"
);
require(
    receipt.result.process.child_exit_clean
        && receipt.result.process.orphan_cleanup_armed
        && receipt.result.process.forced_cleanup_reaped
        && receipt.result.process.forced_cleanup_pid > 0
        && receipt.result.process.listener_pid != receipt.result.process.connector_pid,
    "agenterm_net_process_isolation"
);
require(
    receipt.result.resources.measurement_complete
        && receipt.result.resources.peak_child_rss_bytes > 0
        && receipt.result.resources.max_observed_child_threads > 0
        && receipt.result.resources.process_samples.len == 2,
    "agenterm_net_resource_evidence"
);
require(
    receipt.result.block.round_trip_verified
        && receipt.result.block.corruption_rejected
        && receipt.result.block.store_removed,
    "agenterm_net_block_evidence"
);

print(
    "PASS: isolated agenterm-net libp2p/CID/block-store research contract"
);
