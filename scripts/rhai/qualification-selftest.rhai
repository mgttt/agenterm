// Prove qualification manifests, results, provenance, and retained diagnostics
// fail closed before the production qualification caller switches to Rhai.
//
// Args: REPO CLI_EXE

import "scripts/rhai/lib/qualification" as qualification;
import "scripts/rhai/lib/test_harness" as test_harness;

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

fn expect_rejection(expected, action) {
    let rejected = false;
    try {
        action.call();
    } catch (error) {
        let message = error.to_string();
        require(
            message.contains(expected),
            "qualification_selftest_wrong_rejection:" +
                expected + ":" + message
        );
        rejected = true;
    }
    require(
        rejected,
        "qualification_selftest_expected_rejection:" + expected
    );
}

fn zeros(count) {
    let value = "";
    for index in 0..count {
        value += "0";
    }
    value
}

fn remove_owned_scratch(root, path) {
    let owned = std::path::absolute(root).display;
    let resolved = std::path::absolute(path).display;
    require(
        std::path::parent(resolved).display.to_lower() ==
            owned.to_lower(),
        "qualification_selftest_scratch_outside_owned_root:" +
            resolved
    );
    if std::fs::exists(resolved) {
        std::fs::remove_dir_all(resolved);
    }
}

fn evidence_lines(gate) {
    let output = [];
    for id in gate.evidence {
        output.push("EVIDENCE " + id);
    }
    output
}

require(args.len == 2, "expected: REPO CLI_EXE");
let repo = std::path::absolute(args[0]).display;
let cli = std::path::absolute(args[1]).display;
require(std::fs::exists(cli), "qualification_selftest_cli_missing");
let manifest_path = std::path::join(
    repo,
    "scripts/qualification-gates.json"
).display;

expect_rejection(
    "qualification_manifest_missing",
    || qualification::read_manifest(
        std::path::join(
            repo,
            "target/qualification/missing-gates.json"
        ).display
    )
);

for rejected_status in ["skipped", "failed"] {
    let context = qualification::new_context(
        manifest_path,
        false,
        false
    );
    for gate in context.manifest.required_gates {
        let status =
            if gate.id == "rustfmt" {
                rejected_status
            } else {
                "passed"
            };
        context = qualification::add_result(
            context,
            gate.id,
            status,
            0,
            evidence_lines(gate)
        );
    }
    expect_rejection(
        "qualification_gate_not_passed:rustfmt:" +
            rejected_status,
        || qualification::assert_results(context)
    );
}

let no_stress = qualification::new_context(
    manifest_path,
    false,
    false
);
for gate in no_stress.manifest.required_gates {
    no_stress = qualification::add_result(
        no_stress,
        gate.id,
        "passed",
        0,
        evidence_lines(gate)
    );
}
expect_rejection(
    "qualification_stress_required",
    || qualification::assert_results(no_stress)
);

let missing_process = qualification::new_context(
    manifest_path,
    false,
    true
);
expect_rejection(
    "qualification_process_observation_missing",
    || qualification::assert_process_observation(missing_process)
);
let observed_process = qualification::add_process_observation(
    missing_process,
    1,
    [],
    false
);
qualification::assert_process_observation(observed_process);
let forbidden_process = qualification::add_process_observation(
    observed_process,
    1,
    ["pwsh.exe:123"],
    false
);
expect_rejection(
    "qualification_process_observation_powershell",
    || qualification::assert_process_observation(forbidden_process)
);

let check_source = std::fs::read_to_string(
    std::path::join(repo, "scripts/rhai/check.rhai").display
);
let artifact_offset = check_source.index_of(
    `"artifact-build",`
);
let lint_offset = check_source.index_of(
    "let lint_gate = run_gate("
);
let rustfmt_offset = check_source.index_of(
    "let rustfmt_gate = run_gate("
);
let declaration_offset = check_source.index_of(
    "sequence = assert_declarations("
);
let smoke_offset = check_source.index_of(
    `let smoke_ids = [`
);
require(
    artifact_offset >= 0
        && lint_offset >= 0
        && rustfmt_offset >= 0
        && declaration_offset >= 0
        && smoke_offset >= 0
        && lint_offset < declaration_offset
        && declaration_offset < rustfmt_offset
        && rustfmt_offset < artifact_offset
        && artifact_offset < smoke_offset,
    "qualification_selftest_declaration_order"
);

let scratch_root = std::path::join(
    repo,
    "target/qualification"
).display;
std::fs::create_dir_all(scratch_root);
let scratch = std::path::join(
    scratch_root,
    "selftest-" +
        std::time::SystemTime::now().unix_millis + "-" +
        std::process::id()
).display;
std::fs::create_dir_all(scratch);
let harness_context = ();
let failure = "";

try {
    let metadata = rhai::json::parse_file(
        std::path::join(repo, "dist/agenterm.json").display
    );
    metadata.git_commit = qualification::command_output(
        "git",
        ["rev-parse", "HEAD"],
        repo,
        "qualification_selftest_git"
    ).to_lower();
    metadata.git_dirty = true;
    metadata.cargo_lock_sha256 = qualification::sha256(
        std::path::join(repo, "Cargo.lock").display
    );
    metadata.artifact_manifest_sha256 = zeros(64);
    let executables = [];
    for entry in metadata.executables {
        entry.sha256 = qualification::sha256(
            std::path::join(
                std::path::join(repo, "dist").display,
                entry.name
            ).display
        );
        executables.push(entry);
    }
    metadata.executables = executables;
    let metadata_path = std::path::join(
        scratch,
        "mismatched-agenterm.json"
    ).display;
    rhai::runtime::atomic_write(
        metadata_path,
        rhai::json::stringify_pretty(metadata) + "\n"
    );
    expect_rejection(
        "qualification_artifact_manifest_hash",
        || qualification::provenance(
            repo,
            metadata_path,
            std::path::join(
                repo,
                "scripts/artifacts.json"
            ).display,
            std::path::join(repo, "dist").display,
            false
        )
    );

    harness_context = test_harness::new_context(
        repo,
        "qualification-selftest"
    );
    let result = test_harness::run_cli(
        harness_context,
        cli,
        ["__qualification_invalid_command__"],
        true,
        []
    );
    require(
        !result.success,
        "qualification_selftest_cli_failure_not_injected"
    );
    test_harness::complete_quiet(
        harness_context,
        [],
        false,
        "qualification CLI failure injection\n" +
            result.stdout + result.stderr
    );
    require(
        std::fs::exists(harness_context.manifest_path),
        "qualification_selftest_manifest_missing"
    );
    let bundle = rhai::json::parse_file(
        harness_context.manifest_path
    );
    require(
        bundle.privacy.pane_capture == "disabled for this suite"
            && !bundle.diagnostics.contains("capture-pane.txt")
            && std::fs::exists(harness_context.command_log_path),
        "qualification_selftest_diagnostics_contract"
    );
} catch (error) {
    failure = error.to_string();
}

if harness_context != () {
    test_harness::remove_run(harness_context);
}
remove_owned_scratch(scratch_root, scratch);
if failure != "" {
    throw failure;
}
print("PASS: qualification fail-closed self-test");
