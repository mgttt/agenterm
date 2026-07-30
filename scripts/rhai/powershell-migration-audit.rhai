// Enforce the frozen v0.1.10 PowerShell migration ledger.
//
// Named task:
//   agenterm-script task run migration-audit --json

fn require(condition, message) {
    if !condition {
        throw message;
    }
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
require(ledger.schema_version == 1, "migration_ledger_schema");
require(ledger.baseline_count == 43, "migration_ledger_baseline_count");
require(ledger.entries.len == ledger.baseline_count, "migration_ledger_entry_count");

let known = #{};
let deleted = 0;
let deleted_paths = [];
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

let automation = std::process::command("git");
automation.args([
    "-C", repo, "grep", "-n", "-i",
    "-e", "shell: pwsh",
    "-e", "shell: powershell",
    "-e", "powershell.exe",
    "-e", "pwsh.exe",
    "-e", ".ps1",
    "--",
    ".github", "build.bat", "check.cmd", "lint.cmd", "release.cmd"
]);
automation.timeout(std::time::Duration::from_secs(10));
let automation_output = automation.output();
require(
    automation_output.exit_code == 1,
    if automation_output.exit_code == 0 {
        "migration_powershell_automation_reference:" +
            automation_output.stdout_text()
    } else {
        automation_output.error("migration-powershell-automation-scan")
    }
);

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
        `@set "AGENTERM_BOOTSTRAP_TASK=` + aliases[path] +
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
        "cargo build --quiet --locked --bin agenterm-script"
    ),
    "migration_batch_bootstrap_worker_build"
);
require(
    bootstrap_source.split("\n").len <= 40,
    "migration_batch_bootstrap_size"
);

let report = #{
    schema_version: 1,
    task_id: "migration-audit",
    baseline_count: ledger.baseline_count,
    deleted_count: deleted,
    remaining_count: current.keys().len,
    expected_remaining_count: ledger.baseline_count - deleted,
    powershell_automation_references: 0,
    batch_entry_count: batch_paths.len,
    batch_alias_count: aliases.keys().len,
    batch_business_logic_references: 0,
    drift: false
};
require(
    report.remaining_count == report.expected_remaining_count,
    "migration_remaining_count: actual=" + report.remaining_count +
        ", expected=" + report.expected_remaining_count
);
print(rhai::json::stringify_pretty(report));
