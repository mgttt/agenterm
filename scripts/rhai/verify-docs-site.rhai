// Case-exact static-site resource check for case-insensitive dev hosts.

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

require(args.len == 1, "expected: DOCS_ROOT");
let docs = std::path::absolute(args[0]).display;
let html = std::fs::read_to_string(std::path::join(docs, "index.html").display);
let image_name = "agenterm-workspace.png";
let expected_reference = "src=\"assets/" + image_name + "\"";

require(
    html.contains(expected_reference),
    "docs index must use the case-exact workspace image reference"
);
require(
    html.contains("docs/agenterm-script-runtime.md"),
    "docs index must use the case-exact script specification path"
);
require(
    html.contains("© 2026 PARTNERNET SOFTWARE PTY LTD"),
    "docs index must publish the product copyright owner"
);

let found = false;
for entry in std::fs::read_dir(std::path::join(docs, "assets").display) {
    if entry.is_file && entry.file_name == image_name {
        found = true;
        break;
    }
}
require(found, "docs assets must contain the case-exact workspace image");

print("PASS: docs site image reference and asset casing agree");
