// Prove exact child ownership, cleanup evidence, and original-failure retention.
//
// Args:
//   REPO

import "scripts/rhai/lib/test_harness" as test_harness;

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

require(args.len == 1, "expected: REPO");
let context = test_harness::new_context(
    args[0],
    "harness-cleanup-selftest"
);
let owned_command = std::process::command("cmd.exe");
owned_command.args([
    "/d", "/s", "/c",
    "ping -n 60 127.0.0.1 >nul"
]);
owned_command.timeout(std::time::Duration::from_secs(60));
let unowned_command = std::process::command("cmd.exe");
unowned_command.args([
    "/d", "/s", "/c",
    "ping -n 60 127.0.0.1 >nul"
]);
unowned_command.timeout(std::time::Duration::from_secs(60));
let owned = owned_command.start();
let unowned = unowned_command.start();
let owned_pid = owned.id;
let unowned_pid = unowned.id;
let marker = "original-fixture-failure-must-survive-cleanup";
let succeeded = false;
let failure = "";

try {
    let verbose_command = std::process::command("cmd.exe");
    verbose_command.args([
        "/d", "/s", "/c",
        "for /L %i in (1,1,1000) do @echo diagnostic-line-%i-abcdefghijklmnopqrstuvwxyz"
    ]);
    verbose_command.capture_limit(65536);
    let verbose = verbose_command.output();
    verbose.require_success("harness_verbose_fixture");
    test_harness::append_command_record(
        context,
        ["verbose-diagnostic-fixture"],
        verbose,
        false,
        []
    );
    let command_log = std::fs::read_to_string(context.command_log_path);
    require(
        command_log.contains("<agenterm-command-output-truncated>")
            && std::fs::metadata(context.command_log_path).len < 8192,
        "harness_command_record_not_bounded"
    );

    let proof = test_harness::complete(
        context,
        [owned],
        false,
        marker
    );
    require(owned.state == "exited", "harness_owned_child_survived");
    require(unowned.state == "running", "harness_killed_unowned_child");
    require(
        proof.cleanup.orphan_free
            && proof.cleanup.forced_pids.contains(owned_pid)
            && !proof.cleanup.forced_pids.contains(unowned_pid)
            && proof.manifest.failure.contains(marker),
        "harness_cleanup_proof_incomplete"
    );
    let cleanup = rhai::json::parse_file(context.cleanup_path);
    let manifest = rhai::json::parse_file(context.manifest_path);
    require(
        cleanup.orphan_free
            && cleanup.forced_pids.contains(owned_pid)
            && !cleanup.forced_pids.contains(unowned_pid)
            && manifest.cleanup.orphan_free
            && manifest.failure.contains(marker),
        "harness_persisted_proof_incomplete"
    );
    succeeded = true;
} catch (error) {
    failure = error.to_string();
}

if unowned.state == "running" {
    unowned.kill();
    unowned.wait_with_output(std::time::Duration::from_secs(2));
}
if owned.state == "running" {
    owned.kill();
    owned.wait_with_output(std::time::Duration::from_secs(2));
}
test_harness::remove_run(context);
if !succeeded {
    throw failure;
}
print("PASS: harness cleanup ownership and orphan proof");
