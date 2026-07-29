// Produce the staged build provenance manifest from frozen or live inputs.
//
// Args:
//   REPO OUTPUT_MANIFEST ARTIFACT_MANIFEST STAGED_DIRECTORY PROFILE

import "scripts/rhai/lib/build_metadata" as build_metadata;

if args.len != 5 {
    throw "expected: REPO OUTPUT_MANIFEST ARTIFACT_MANIFEST STAGED_DIRECTORY PROFILE";
}
let manifest =
    build_metadata::write(args[0], args[1], args[2], args[3], args[4]);
