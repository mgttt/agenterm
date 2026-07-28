// Fail closed when an internal-only product version becomes publishable.
//
// Usage:
//   agenterm-cli script run scripts/rhai/internal-version-policy.rhai \
//     --profile local --timeout-ms 10000 -- REPO

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

require(args.len == 1, "expected: REPO");
let repo = std::path::absolute(args[0]).display;
let cargo = std::fs::read_to_string(std::path::join(repo, "Cargo.toml").display);

let in_package = false;
let version = "";
for source_line in cargo.split("\n") {
    let line = source_line;
    line.trim();
    if line.starts_with("[") {
        in_package = line == "[package]";
        continue;
    }
    if in_package && line.starts_with("version") {
        let parts = line.split("\"");
        if parts.len >= 2 {
            version = parts[1];
            break;
        }
    }
}
require(version != "", "Could not read the package version from Cargo.toml.");

if version != "0.1.7" {
    print("PASS: " + version + " is not governed by the internal 0.1.7 policy.");
} else {
    let tags = std::process::command("git");
    tags.args(["-C", repo, "tag", "--list", "v0.1.7"]);
    tags.timeout(std::time::Duration::from_secs(2));
    let tag_output = tags.output();
    require(tag_output.success, tag_output.error("internal-version-git-tags"));
    let tag_text = tag_output.stdout_text();
    tag_text.trim();
    require(
        tag_text.len == 0,
        "Internal-only v0.1.7 must not have a local Git tag."
    );

    let release_source = std::fs::read_to_string(
        std::path::join(repo, "release.ps1").display
    );
    let workflow_source = std::fs::read_to_string(
        std::path::join(repo, ".github/workflows/release.yml").display
    );
    require(
        release_source.contains("version -eq '0.1.7'") &&
        workflow_source.contains("expected -eq 'v0.1.7'"),
        "The local coordinator and tag workflow must both reject v0.1.7."
    );

    print(
        "PASS: v0.1.7 is internal-only, untagged, and rejected by both " +
        "publication entry points."
    );
}
