// Stage the public Windows archive and versioned SBOM from exact qualified
// candidate bytes. This task never builds, tags, pushes, or publishes.
//
// Args: REPO RECEIPT_PATH OUTPUT_DIRECTORY

import "scripts/rhai/lib/package_qualified" as package_qualified;

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

require(
    args.len == 3,
    "expected: REPO RECEIPT_PATH OUTPUT_DIRECTORY"
);
let repo = std::path::absolute(args[0]).display;
let output = std::path::join(repo, args[2]).display;
std::fs::create_dir_all(output);
let run_directory = package_qualified::build_release_package(
    repo,
    args[1],
    "target/qualification/package-release"
);
let result = rhai::json::parse_file(
    std::path::join(
        run_directory,
        "release-result.json"
    ).display
);
require(!result.dry_run, "release_package_marked_dry_run");
let archive_source =
    std::path::join(run_directory, result.archive).display;
let archive_destination =
    std::path::join(output, result.archive).display;
std::fs::copy(archive_source, archive_destination);
require(
    rhai::crypto::sha256_file(archive_destination) ==
        result.archive_sha256,
    "release_package_copy_hash"
);

let metadata = rhai::json::parse_file(
    std::path::join(repo, "dist/agenterm.json").display
);
let receipt = rhai::json::parse_file(args[1]);
let sbom_source =
    std::path::join(repo, "dist/agenterm-sbom.spdx.json").display;
let sbom_name =
    "agenterm-" + metadata.version + "-sbom.spdx.json";
let sbom_destination =
    std::path::join(output, sbom_name).display;
std::fs::copy(sbom_source, sbom_destination);
require(
    rhai::crypto::sha256_file(sbom_destination) ==
        rhai::crypto::sha256_file(sbom_source),
    "release_sbom_copy_hash"
);

let archive_hash = rhai::crypto::sha256_file(archive_destination);
let archive_name = result.archive;
let provenance = #{
    schema_version: 1,
    product: "AgenTerm",
    version: metadata.version,
    os: "windows",
    arch: "x86_64",
    channel: "release",
    signed: false,
    notarized: false,
    artifact: archive_name,
    sha256: archive_hash,
    source_commit: receipt.provenance.git_head,
    source_tag: if std::env::has("GITHUB_REF_NAME") {
        std::env::get("GITHUB_REF_NAME")
    } else {
        ""
    },
    artifact_manifest_sha256:
        receipt.provenance.artifact_manifest_sha256,
    cargo_lock_sha256: receipt.provenance.cargo_lock_sha256,
    build_log: if std::env::has("GITHUB_SERVER_URL")
            && std::env::has("GITHUB_REPOSITORY")
            && std::env::has("GITHUB_RUN_ID") {
        std::env::get("GITHUB_SERVER_URL") + "/" +
            std::env::get("GITHUB_REPOSITORY") + "/actions/runs/" +
            std::env::get("GITHUB_RUN_ID")
    } else {
        ""
    }
};
let provenance_destination = archive_destination + ".provenance.json";
let checksum_destination = archive_destination + ".sha256";
rhai::runtime::atomic_write(
    provenance_destination,
    rhai::json::stringify_pretty(provenance) + "\n"
);
rhai::runtime::atomic_write(
    checksum_destination,
    archive_hash + "  " + archive_name + "\n"
);
print(
    "RELEASE ASSETS " + archive_destination + " " +
        sbom_destination + " " + provenance_destination + " " +
        checksum_destination
);
