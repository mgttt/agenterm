// Build and validate one exact-SHA Candidate manifest from a closed payload.
// Archived after cutover to scripts/rh/candidate-aggregate.rh.
//
// Named task (historical):
//   agenterm-rhai task run candidate-aggregate -- \
//     SOURCE_ROOT METADATA_JSON ASSET_DIRECTORY OUTPUT_MANIFEST
//
// METADATA_JSON schema v1 fields:
//   source_sha, version, expected_tag, workflow_path, workflow_sha256,
//   run_id, run_attempt, run_event, created_at_unix_ms, expires_at_unix_ms.

import "scripts/rhai/lib/release_candidate" as candidate;

if args.len == 1 && (args[0] == "--help" || args[0] == "-h") {
    print(
        "usage: candidate-aggregate SOURCE_ROOT METADATA_JSON " +
            "ASSET_DIRECTORY OUTPUT_MANIFEST"
    );
    print(
        "creates schema-v1 exact-SHA Candidate metadata; validates the " +
            "closed six-platform payload before writing"
    );
    return;
}
if args.len != 4 {
    throw "candidate_aggregate_arguments: expected SOURCE_ROOT " +
        "METADATA_JSON ASSET_DIRECTORY OUTPUT_MANIFEST";
}
let manifest = candidate::build_manifest(args[0], args[1], args[2], args[3]);
print(
    "CANDIDATE " + manifest.source_sha + " " + manifest.expected_tag +
        " " + std::path::absolute(args[3]).display
);
