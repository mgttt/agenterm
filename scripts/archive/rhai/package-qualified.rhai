// Create one byte-qualified offline Windows package dry run.
//
// Args: REPO RECEIPT_PATH OUTPUT_DIRECTORY

import "scripts/rhai/lib/package_qualified" as package_qualified;

if args.len != 3 {
    throw "expected: REPO RECEIPT_PATH OUTPUT_DIRECTORY";
}
let run_directory = package_qualified::build_package(
    args[0],
    args[1],
    args[2]
);
print("PACKAGE DRY-RUN " + run_directory);
