// Enforce the frozen v0.1.10 PowerShell migration ledger.
//
// Named task:
//   agenterm-script task run migration-audit --json

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

let repo = std::env::current_dir().display;
let ledger_path = std::path::join(
    repo,
    "scripts/powershell-migration.json"
).display;
let ledger = rhai::json::parse(std::fs::read_to_string(ledger_path));
require(ledger.schema_version == 1, "migration_ledger_schema");
require(ledger.baseline_count == 43, "migration_ledger_baseline_count");
require(ledger.entries.len == ledger.baseline_count, "migration_ledger_entry_count");

let known = #{};
let deleted = 0;
for entry in ledger.entries {
    require(entry.id != "", "migration_ledger_empty_id");
    require(entry.path.ends_with(".ps1"), "migration_ledger_non_ps1_path");
    require(!known.contains(entry.path), "migration_ledger_duplicate_path: " + entry.path);
    require(
        entry.status == "inventory" || entry.status == "deleted",
        "migration_ledger_status: " + entry.path
    );
    known[entry.path] = entry.status;
    if entry.status == "deleted" {
        deleted += 1;
    }
}

let command = std::process::command("git");
command.args(["ls-files", "--", "*.ps1"]);
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

let report = #{
    schema_version: 1,
    task_id: "migration-audit",
    baseline_count: ledger.baseline_count,
    deleted_count: deleted,
    remaining_count: current.keys().len,
    expected_remaining_count: ledger.baseline_count - deleted,
    drift: false
};
require(
    report.remaining_count == report.expected_remaining_count,
    "migration_remaining_count: actual=" + report.remaining_count +
        ", expected=" + report.expected_remaining_count
);
print(rhai::json::stringify_pretty(report));
