// Archived interpreted implementation retained for migration archaeology only.
// Active task: scripts/rh/verify-docs-site.rh

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

require(args.len == 1, "expected: DOCS_ROOT");
let docs = std::path::absolute(args[0]).display;
let html = std::fs::read_to_string(std::path::join(docs, "index.html").display);
let mac_preview = std::fs::read_to_string(
    std::path::join(docs, "macos-unsigned-preview.md").display
);
let mac_preview_zh = std::fs::read_to_string(
    std::path::join(docs, "macos-unsigned-preview.zh-Hant.md").display
);
let image_name = "agenterm-workspace.png";
let expected_reference = "src=\"assets/" + image_name + "\"";

require(
    html.contains(expected_reference),
    "docs index must use the case-exact workspace image reference"
);
require(
    html.contains("docs/agenterm-rhai-runtime.md"),
    "docs index must use the case-exact script specification path"
);
require(
    html.contains("© 2026 PARTNERNET SOFTWARE PTY LTD"),
    "docs index must publish the product copyright owner"
);
require(
    html.contains("hreflang=\"en\"")
        && html.contains("hreflang=\"zh-Hant\"")
        && html.contains("data-locale-choice=\"zh-Hant\""),
    "docs index must publish English and Traditional Chinese discovery"
);
require(
    html.contains("本機 Agent 艦隊工作 OS")
        && html.contains("Windows 已交付")
        && html.contains("macos-unsigned-preview.md"),
    "docs index must include reviewed Traditional Chinese product copy and preview guidance"
);
require(
    mac_preview.contains("../prd/PRD_02_17_delivery_quality.md")
        && mac_preview.contains("macos-unsigned-preview.zh-Hant.md"),
    "English macOS preview guidance must link its owning PRD and Traditional Chinese peer"
);
require(
    mac_preview_zh.contains("../prd/PRD_02_17_delivery_quality.md")
        && mac_preview_zh.contains("macos-unsigned-preview.md")
        && mac_preview_zh.contains("請勿停用整個系統的保護"),
    "Traditional Chinese macOS preview guidance must preserve ownership, peer, and safety boundaries"
);

let found = false;
for entry in std::fs::read_dir(std::path::join(docs, "assets").display) {
    if entry.is_file && entry.file_name == image_name {
        found = true;
        break;
    }
}
require(found, "docs assets must contain the case-exact workspace image");

print("PASS: bilingual docs site, preview safety guidance, and asset casing agree");
