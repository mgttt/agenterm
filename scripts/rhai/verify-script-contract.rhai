// First repository dogfood for agenterm-script.
//
// PowerShell still prepares the machine catalog fixture and remains the
// qualification fallback. This script owns the data-oriented contract checks
// so the Rhai runtime is exercised by AgenTerm's real development loop.

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

require(args.len == 2, "expected: <runtime-spec.md> <script-catalog.json>");

let specification = std::fs::read_to_string(args[0]);
require(
    specification.starts_with("# AgenTerm Script Runtime Specification"),
    "runtime specification must be the English public specification"
);
require(
    specification.contains("## 1. Complete public object and interface tree"),
    "runtime specification must open with the complete object tree"
);
require(
    specification.contains("The Rhai surface is the product contract."),
    "runtime specification must declare the Rhai surface as the product contract"
);

let catalog = rhai::json::parse(std::fs::read_to_string(args[1]));
require(catalog.schema_version == 3, "unexpected script catalog schema");
require(catalog.api_version == 2, "unexpected Script API major");
require(catalog.entries.len > 0, "script catalog must not be empty");
require(catalog.comparison.schema_version == 1, "unexpected comparison schema");
require(
    catalog.comparison.reviewed_on == "2026-07-29",
    "comparison review date drifted"
);

let stable_count = 0;
let reserved_count = 0;
let legacy_count = 0;

for entry in catalog.entries {
    require(entry.stable_id != "", "catalog entry is missing stable_id");
    require(entry.surface_path != "", "catalog entry is missing surface_path");
    require(entry.designed_on != "", "catalog entry is missing designed_on");
    require(entry.since != "", "catalog entry is missing since");
    require(
        entry.comparisons.nodejs.reviewed_on == "2026-07-29",
        "Node.js comparison is not reviewed: " + entry.stable_id
    );
    require(
        entry.comparisons.bun.reviewed_on == "2026-07-29",
        "Bun comparison is not reviewed: " + entry.stable_id
    );
    require(
        entry.comparisons.nodejs.relationship == "similar" ||
        entry.comparisons.nodejs.relationship == "agenterm_specific",
        "Node.js comparison is not classified: " + entry.stable_id
    );
    require(
        entry.comparisons.bun.relationship == "similar" ||
        entry.comparisons.bun.relationship == "agenterm_specific",
        "Bun comparison is not classified: " + entry.stable_id
    );

    if entry.stability == "stable" {
        stable_count += 1;
    } else if entry.stability == "reserved" {
        reserved_count += 1;
    } else if entry.stability == "legacy" {
        legacy_count += 1;
    } else {
        throw "catalog entry has unknown stability: " + entry.stable_id;
    }
}

require(stable_count > 0, "catalog must expose stable entries");
require(reserved_count > 0, "catalog must expose reserved entries");
require(legacy_count == 0, "Script API v2 must not retain legacy agent aliases");

print(
    "PASS: Rhai verified the Script API contract (" +
    stable_count + " stable, " +
    reserved_count + " reserved, " +
    legacy_count + " legacy)"
);
