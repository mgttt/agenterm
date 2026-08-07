// Archived interpreted implementation retained for migration archaeology only.
// Active task: scripts/rh/stage-artifact.rh

// Stage one validated AgenTerm executable, parking an in-use Windows image.
//
// Args:
//   SOURCE_DIRECTORY DESTINATION_DIRECTORY ARTIFACT_NAME

import "scripts/rhai/lib/artifact_files" as artifact_files;

if args.len != 3 {
    throw "expected: SOURCE_DIRECTORY DESTINATION_DIRECTORY ARTIFACT_NAME";
}
let result = artifact_files::stage(args[0], args[1], args[2]);
