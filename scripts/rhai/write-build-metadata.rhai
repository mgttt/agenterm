// Produce the staged build provenance manifest from frozen or live inputs.
//
// Args:
//   REPO OUTPUT_MANIFEST ARTIFACT_MANIFEST STAGED_DIRECTORY PROFILE [OS ARCH]

import "scripts/rhai/lib/build_metadata" as build_metadata;

if args.len != 5 && args.len != 7 {
    throw "expected: REPO OUTPUT_MANIFEST ARTIFACT_MANIFEST STAGED_DIRECTORY PROFILE [OS ARCH]";
}
let manifest = if args.len == 5 {
    build_metadata::write(args[0], args[1], args[2], args[3], args[4])
} else {
    build_metadata::write_platform(
        args[0], args[1], args[2], args[3], args[4], args[5], args[6]
    )
};
