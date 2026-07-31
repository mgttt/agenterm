// Verify an exact-SHA Candidate manifest and every payload byte.
//
// Usage:
//   agenterm-script run scripts/rhai/candidate-verify.rhai \
//     -- SOURCE_ROOT MANIFEST ASSET_DIRECTORY [NOW_UNIX_MS]
//
// NOW_UNIX_MS exists for deterministic fixture tests.  Production callers
// should omit it and use the runtime clock.

import "scripts/rhai/lib/release_candidate" as candidate;

if args.len == 1 && (args[0] == "--help" || args[0] == "-h") {
    print(
        "usage: candidate-verify SOURCE_ROOT MANIFEST ASSET_DIRECTORY " +
            "[NOW_UNIX_MS]"
    );
    print(
        "fails closed on identity, expiry, receipt, platform-set, " +
            "companion, provenance, hash, size, or payload-set mismatch"
    );
    return;
}
if args.len != 3 && args.len != 4 {
    throw "candidate_verify_arguments: expected SOURCE_ROOT MANIFEST " +
        "ASSET_DIRECTORY [NOW_UNIX_MS]";
}
let now_ms =
    if args.len == 4 {
        args[3].parse_int()
    } else {
        std::time::SystemTime::now().unix_millis
    };
let manifest = candidate::verify_manifest(args[0], args[1], args[2], now_ms);
print(
    "VALID CANDIDATE " + manifest.source_sha + " " +
        manifest.expected_tag + " run=" + manifest.run.id
);
