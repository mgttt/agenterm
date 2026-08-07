// Validate the canonical Windows executable contract used by local staging.
//
// Args:
//   ARTIFACT_MANIFEST

import "scripts/rhai/lib/artifact_manifest" as artifact_manifest;

if args.len != 1 {
    throw "expected: ARTIFACT_MANIFEST";
}
let manifest = artifact_manifest::load(args[0]);
print(
    "PASS: artifact manifest schema 2 defines " +
    manifest.executables.len + " validated Windows executables"
);
