// Enforce the frozen v0.1.10 PowerShell migration ledger.
//
// Named task:
//   agenterm-rhai task run migration-audit --json

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

fn require_nonempty_text(value, field, path) {
    require(
        type_of(value) == "string" && value.trim() != "",
        "migration_ledger_" + field + ":" + path
    );
}

fn require_nonempty_text_array(value, field, path) {
    require(
        type_of(value) == "array" && value.len > 0 && value.len <= 32,
        "migration_ledger_" + field + ":" + path
    );
    let seen = #{};
    for item in value {
        require_nonempty_text(item, field, path);
        require(
            item.len <= 512,
            "migration_ledger_" + field + "_item_too_long:" + path
        );
        require(
            !seen.contains(item),
            "migration_ledger_" + field + "_duplicate:" + path + ":" + item
        );
        seen[item] = true;
    }
}

fn automation_reference(line) {
    let value = line.to_lower();
    let trimmed = value;
    trimmed.trim();
    value.contains(".ps1")
        || value.contains("powershell.exe")
        || value.contains("pwsh.exe")
        || value.contains(`"powershell"`)
        || value.contains(`"pwsh"`)
        || value.contains("'powershell'")
        || value.contains("'pwsh'")
        || value.contains("shell: powershell")
        || value.contains("shell: pwsh")
        || trimmed.starts_with("powershell ")
        || trimmed.starts_with("pwsh ")
        || trimmed.starts_with("run: powershell ")
        || trimmed.starts_with("run: pwsh ")
}

fn allowed_automation_reference(path, line) {
    let value = line.to_lower();
    value.trim();
    if path == "scripts/rhai/powershell-migration-audit.rhai" {
        return !value.contains("std::process::command(")
            && !value.starts_with("powershell ")
            && !value.starts_with("pwsh ");
    }
    if path == "scripts/rhai/check.rhai" {
        return value ==
            `if name != "powershell.exe" && name != "pwsh.exe" {`;
    }
    if path == "scripts/rhai/fresh-clone-rehearsal.rhai" {
        return value == `"powershell.exe"` || value == `"pwsh.exe"`;
    }
    if path == "scripts/rhai/qualification-selftest.rhai" {
        return value == `["pwsh.exe:123"],`;
    }
    path == "scripts/rhai/remote-ui-smoke.rhai"
        && (
            value ==
                `"--", "powershell.exe", "-nologo", "-noprofile"`
            || value ==
                `"--", "powershell.exe", "-nologo", "-noprofile",`
            || value == `"alternate-wheel.ps1"`
        )
}

require(args.len <= 1, "expected: [REPO]");
let repo = if args.len == 1 {
    std::path::absolute(args[0]).display
} else {
    std::env::current_dir().display
};
let ledger_path = std::path::join(
    repo,
    "scripts/powershell-migration.json"
).display;
let ledger = rhai::json::parse(std::fs::read_to_string(ledger_path));
require(ledger.schema_version == 2, "migration_ledger_schema");
require(ledger.baseline_count == 43, "migration_ledger_baseline_count");
require(ledger.entries.len == ledger.baseline_count, "migration_ledger_entry_count");

let known = #{};
let known_parity = #{};
let deleted = 0;
let deleted_paths = [];
for entry in ledger.entries {
    require_nonempty_text(entry.id, "empty_id", entry.path);
    require(entry.path.ends_with(".ps1"), "migration_ledger_non_ps1_path");
    require_nonempty_text(entry.responsibility, "responsibility", entry.path);
    require_nonempty_text(entry.replacement_task, "replacement_task", entry.path);
    require_nonempty_text(entry.replacement, "replacement", entry.path);
    require_nonempty_text(entry.evidence, "evidence", entry.path);
    require_nonempty_text_array(entry.callers, "callers", entry.path);
    require_nonempty_text_array(entry.inputs, "inputs", entry.path);
    require_nonempty_text_array(entry.outputs, "outputs", entry.path);
    require_nonempty_text_array(entry.side_effects, "side_effects", entry.path);
    require_nonempty_text_array(entry.platforms, "platforms", entry.path);
    for platform in entry.platforms {
        require(
            platform == "windows" || platform == "linux" || platform == "macos",
            "migration_ledger_platform:" + entry.path + ":" + platform
        );
    }
    for effect in entry.side_effects {
        require(
            [
                "none", "repository_read", "process_spawn", "artifact_write",
                "gui_spawn", "network_loopback", "terminal_compatibility_payload",
                "git_mutation", "remote_publish"
            ].contains(effect),
            "migration_ledger_side_effect:" + entry.path + ":" + effect
        );
    }
    require(
        type_of(entry.budgets) == "map"
            && entry.budgets.timeout_ms > 0
            && entry.budgets.timeout_ms <= 3600000
            && entry.budgets.max_output_bytes > 0
            && entry.budgets.max_output_bytes <= 8388608,
        "migration_ledger_budgets:" + entry.path
    );
    require(
        type_of(entry.parity_evidence) == "array"
            && entry.parity_evidence.len > 0,
        "migration_ledger_parity_evidence:" + entry.path
    );
    for parity in entry.parity_evidence {
        require(
            type_of(parity) == "map",
            "migration_ledger_parity_evidence_item:" + entry.path
        );
        require_nonempty_text(parity.id, "parity_id", entry.path);
        require_nonempty_text(parity.owner, "parity_owner", entry.path);
        require_nonempty_text(parity.proof, "parity_proof", entry.path);
        require(
            parity.id.starts_with("migration-parity:"),
            "migration_ledger_parity_id:" + entry.path + ":" + parity.id
        );
        require(
            !known_parity.contains(parity.id),
            "migration_ledger_parity_id_duplicate:" + entry.path + ":" +
                parity.id
        );
        known_parity[parity.id] = true;
    }
    require(!known.contains(entry.path), "migration_ledger_duplicate_path: " + entry.path);
    require(
        entry.status == "inventory" || entry.status == "deleted",
        "migration_ledger_status: " + entry.path
    );
    known[entry.path] = entry.status;
    if entry.status == "deleted" {
        deleted += 1;
        deleted_paths.push(entry.path);
    }
}

let command = std::process::command("git");
command.args(["-C", repo, "ls-files", "--", "*.ps1"]);
command.timeout(std::time::Duration::from_secs(10));
let output = command.output();
require(output.success, output.error("migration-git-ls-files"));
let source = output.stdout_text();
let current = #{};
for source_line in source.split("\n") {
    let path = source_line;
    path.trim();
    if path == "" {
        continue;
    }
    require(known.contains(path), "migration_unplanned_ps1: " + path);
    require(known[path] != "deleted", "migration_deleted_ps1_returned: " + path);
    current[path] = true;
}

for entry in ledger.entries {
    if entry.status == "inventory" {
        require(current.contains(entry.path), "migration_unrecorded_removal: " + entry.path);
    }
}

let deleted_patterns = "";
for path in deleted_paths {
    deleted_patterns += path + "\n";
}
let references = std::process::command("git");
references.args([
    "-C", repo, "grep", "-n", "-F", "-f", "-", "--",
    ".github", "build.bat", "check.cmd", "lint.cmd", "release.cmd",
    "scripts", "tests"
]);
references.stdin_text(deleted_patterns);
references.timeout(std::time::Duration::from_secs(10));
let reference_output = references.output();
require(
    reference_output.exit_code == 0 || reference_output.exit_code == 1,
    reference_output.error("migration-git-grep-deleted-references")
);
if reference_output.exit_code == 0 {
    for source_line in reference_output.stdout_text().split("\n") {
        let line = source_line;
        line.trim();
        if line == ""
                || line.starts_with("scripts/powershell-migration.json:") {
            continue;
        }
        throw "migration_deleted_ps1_reference:" + line;
    }
}

let automation_inventory = std::process::command("git");
automation_inventory.args([
    "-C", repo, "ls-files", "--",
    "*.bat", "*.cmd", "*.sh", "*.rhai", ".github/workflows"
]);
automation_inventory.timeout(std::time::Duration::from_secs(10));
let automation_inventory_output = automation_inventory.output();
require(
    automation_inventory_output.success,
    automation_inventory_output.error("migration-automation-inventory")
);
let automation_paths = [];
for source_line in automation_inventory_output.stdout_text().split("\n") {
    let path = source_line;
    path.trim();
    if path == "" {
        continue;
    }
    automation_paths.push(path);
    let source = std::fs::read_to_string(std::path::join(repo, path).display);
    let line_number = 0;
    for line in source.split("\n") {
        line_number += 1;
        if automation_reference(line)
                && !allowed_automation_reference(path, line) {
            throw "migration_powershell_automation_reference:" + path + ":" +
                line_number;
        }
    }
}

let batch = std::process::command("git");
batch.args(["-C", repo, "ls-files", "--", "*.bat", "*.cmd"]);
batch.timeout(std::time::Duration::from_secs(10));
let batch_output = batch.output();
require(batch_output.success, batch_output.error("migration-git-ls-batch"));
let batch_paths = [];
for source_line in batch_output.stdout_text().split("\n") {
    let path = source_line;
    path.trim();
    if path != "" {
        batch_paths.push(path);
    }
}
let expected_batch = [
    "build.bat",
    "check.cmd",
    "lint.cmd",
    "release.cmd",
    "scripts/bootstrap.cmd"
];
require(
    batch_paths.len == expected_batch.len,
    "migration_batch_inventory_count:" + batch_paths.len
);
for path in expected_batch {
    require(
        batch_paths.contains(path),
        "migration_batch_inventory_missing:" + path
    );
}

let aliases = #{
    "build.bat": "build",
    "check.cmd": "check",
    "lint.cmd": "lint",
    "release.cmd": "release"
};
for path in aliases.keys() {
    let source = std::fs::read_to_string(std::path::join(repo, path).display);
    source.replace("\r\n", "\n");
    source.trim();
    let expected =
        `@setlocal && set "AGENTERM_BOOTSTRAP_TASK=` + aliases[path] +
        `" && call "%~dp0scripts\bootstrap.cmd" %*`;
    require(source == expected, "migration_batch_alias_not_thin:" + path);
}
let bootstrap_source = std::fs::read_to_string(
    std::path::join(repo, "scripts/bootstrap.cmd").display
);
bootstrap_source = bootstrap_source.to_lower();
for forbidden in [
    "release-fast",
    "stage-build",
    "include-stress",
    "package-qualified",
    "--timeout-ms",
    "--max-operations",
    "--max-collection-items",
    "--max-string-bytes",
    "--max-output-bytes",
    "cargo clean",
    "git push",
    "git tag"
] {
    require(
        !bootstrap_source.contains(forbidden),
        "migration_batch_bootstrap_business_rule:" + forbidden
    );
}
require(
    bootstrap_source.contains(`task run "%agenterm_bootstrap_task%"`),
    "migration_batch_bootstrap_task_forward"
);
require(
    bootstrap_source.contains(
        "cargo build --quiet --locked --bin agenterm-rhai"
    ),
    "migration_batch_bootstrap_worker_build"
);
require(
    bootstrap_source.split("\n").len <= 175,
    "migration_batch_bootstrap_size"
);

let shell_aliases = #{
    "build.sh": "build",
    "check.sh": "check",
    "lint.sh": "lint",
    "release.sh": "release"
};
for path in shell_aliases.keys() {
    require(
        std::fs::exists(std::path::join(repo, path).display),
        "migration_shell_alias_missing:" + path
    );
    let source = std::fs::read_to_string(std::path::join(repo, path).display);
    source.replace("\r\n", "\n");
    source.trim();
    let task = shell_aliases[path];
    let expected =
        "#!/usr/bin/env sh\n" +
        "set -eu\n" +
        "AGENTERM_BOOTSTRAP_TASK=" + task + "\n" +
        "export AGENTERM_BOOTSTRAP_TASK\n" +
        `exec "$(dirname "$0")/scripts/bootstrap.sh" "$@"`;
    require(source == expected, "migration_shell_alias_not_thin:" + path);
}
let shell_bootstrap_path =
    std::path::join(repo, "scripts/bootstrap.sh").display;
require(
    std::fs::exists(shell_bootstrap_path),
    "migration_shell_bootstrap_missing"
);
let shell_bootstrap = std::fs::read_to_string(shell_bootstrap_path);
shell_bootstrap = shell_bootstrap.to_lower();
let shell_bootstrap_operational_lines = 0;
for source_line in shell_bootstrap.split("\n") {
    let line = source_line;
    line.trim();
    if line != "" && (!line.starts_with("#") || line.starts_with("#!")) {
        shell_bootstrap_operational_lines += 1;
    }
}
for forbidden in [
    "release-fast",
    "stage-build",
    "include-stress",
    "package-qualified",
    "--timeout-ms",
    "--max-operations",
    "--max-collection-items",
    "--max-string-bytes",
    "--max-output-bytes",
    "cargo clean",
    "git push",
    "git tag"
] {
    require(
        !shell_bootstrap.contains(forbidden),
        "migration_shell_bootstrap_business_rule:" + forbidden
    );
}
require(
    shell_bootstrap.contains(
        `task run "$agenterm_bootstrap_task"`
    )
        && shell_bootstrap.contains(
            "cargo build --quiet --locked --bin agenterm-rhai"
        )
        && shell_bootstrap_operational_lines <= 160,
    "migration_shell_bootstrap_not_generic"
);
for timing_fact in [
    "agenterm_bootstrap_timing_schema",
    "agenterm_bootstrap_setup_ms",
    "agenterm_bootstrap_cargo_build_ms",
    "agenterm_bootstrap_worker_copy_ms",
    "agenterm_bootstrap_other_setup_ms",
    "agenterm_bootstrap_clock_resolution_ms",
    "agenterm_bootstrap_lock_wait_state",
    "agenterm_bootstrap_worker_state",
    "agenterm_bootstrap_fingerprint",
    "included_not_separable",
    "not_applicable",
    "compatibility.stamp",
    "git hash-object",
    "rustc -vv",
    "cargo -vv",
    "cargo.lock",
    "agenterm.tasks.json"
] {
    require(
        bootstrap_source.contains(timing_fact)
            && shell_bootstrap.contains(timing_fact),
        "migration_bootstrap_timing_fact:" + timing_fact
    );
}

let report = #{
    schema_version: 2,
    task_id: "migration-audit",
    baseline_count: ledger.baseline_count,
    deleted_count: deleted,
    remaining_count: current.keys().len,
    expected_remaining_count: ledger.baseline_count - deleted,
    powershell_automation_references: 0,
    automation_file_count: automation_paths.len,
    batch_entry_count: batch_paths.len,
    batch_alias_count: aliases.keys().len,
    batch_business_logic_references: 0,
    shell_alias_count: shell_aliases.keys().len,
    shell_business_logic_references: 0,
    drift: false
};
require(
    report.remaining_count == report.expected_remaining_count,
    "migration_remaining_count: actual=" + report.remaining_count +
        ", expected=" + report.expected_remaining_count
);
print(rhai::json::stringify_pretty(report));
