// Freeze truthful source inputs into a batch-importable build environment.
//
// Args:
//   REPO PROFILE OUTPUT_PATH

import "scripts/rhai/lib/build_identity" as build_identity;

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

require(args.len == 3, "expected: REPO PROFILE OUTPUT_PATH");
let output_path = std::path::absolute(args[2]).display;
let identity = build_identity::facts(args[0], args[1]);
rhai::runtime::atomic_write(
    output_path,
    build_identity::batch_environment(identity)
);
print(
    "Prepared build identity for " + identity.commit +
    " (" + identity.profile + ", dirty=" + identity.dirty + ")"
);
