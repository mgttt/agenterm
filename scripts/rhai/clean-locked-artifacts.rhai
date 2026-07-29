// Remove stale staged executables left behind by an atomic replacement.
//
// Args:
//   DIRECTORY ARTIFACT_MANIFEST [OBSOLETE_NAME...]

import "scripts/rhai/lib/artifact_files" as artifact_files;
import "scripts/rhai/lib/artifact_manifest" as artifact_manifest;

if args.len < 2 {
    throw "expected: DIRECTORY ARTIFACT_MANIFEST [OBSOLETE_NAME...]";
}

let obsolete_names = [];
if args.len > 2 {
    for index in 2..args.len {
        obsolete_names.push(args[index]);
    }
}
let manifest = artifact_manifest::load(args[1]);
let result =
    artifact_files::clean_locked(args[0], manifest, obsolete_names);
