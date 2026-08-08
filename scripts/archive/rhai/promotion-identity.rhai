// Build the exact, recoverable GitHub Release draft identity from one sealed
// Candidate manifest. GitHub API calls remain thin workflow adapters.
//
// Args: CANDIDATE_MANIFEST CANDIDATE_RUN_ID SOURCE_SHA OUTPUT_JSON

fn require(condition, code) {
    if !condition {
        throw code;
    }
}

fn lowercase_hex(value, length) {
    if type_of(value) != "string" || value.len != length {
        return false;
    }
    for character in value.chars() {
        if !"0123456789abcdef".contains(character) {
            return false;
        }
    }
    true
}

fn decimal(value) {
    if type_of(value) != "string" || value == "" {
        return false;
    }
    for character in value.chars() {
        if !"0123456789".contains(character) {
            return false;
        }
    }
    true
}

require(args.len == 4, "promotion_identity_arguments");
let manifest_path = std::path::absolute(args[0]).display;
let candidate_run_id = args[1];
let source_sha = args[2];
let output = std::path::absolute(args[3]).display;
require(
    std::fs::exists(manifest_path)
        && std::fs::metadata(manifest_path).is_file,
    "promotion_identity_manifest_missing"
);
require(
    decimal(candidate_run_id),
    "promotion_identity_run_id"
);
require(
    lowercase_hex(source_sha, 40),
    "promotion_identity_source_sha"
);

let manifest = rhai::json::parse_file(manifest_path);
require(
    manifest.schema_version == 1
        && manifest.product == "AgenTerm"
        && manifest.source_sha == source_sha
        && manifest.run.id.to_string() == candidate_run_id
        && manifest.version != ""
        && manifest.expected_tag == "v" + manifest.version
        && type_of(manifest.assets) == "array",
    "promotion_identity_manifest"
);
let channels = [];
for asset in manifest.assets {
    if asset.os == "macos" && !channels.contains(asset.channel) {
        channels.push(asset.channel);
    }
}
require(channels.len == 1, "promotion_identity_macos_channels");
let mac_channel = channels[0];
require(
    ["release", "macos-unsigned-preview"].contains(mac_channel),
    "promotion_identity_macos_channel"
);

let manifest_sha256 = rhai::crypto::sha256_file(manifest_path);
let marker =
    "<!-- agenterm-promotion:v1 candidate_run_id=" + candidate_run_id +
    " source_sha=" + source_sha +
    " manifest_sha256=" + manifest_sha256 +
    " mac_channel=" + mac_channel + " -->";
let body =
    marker + "\n\n" +
    "## AgenTerm " + manifest.expected_tag + "\n\n" +
    "Promoted from verified Candidate run `" + candidate_run_id +
    "` for source\n`" + source_sha +
    "`. Downloadable assets are bound to the Candidate\nmanifest `" +
    manifest_sha256 + "`.";
if mac_channel == "macos-unsigned-preview" {
    body +=
        "\n\n## macOS unsigned preview — not the stable macOS channel\n\n" +
        "Assets whose names end in `-macos-*-unsigned-preview.zip` are not\n" +
        "signed with an Apple Developer ID and are not notarized by Apple.\n" +
        "Verify the adjacent SHA-256 and provenance files, then read the\n" +
        "downloadable `MACOS_UNSIGNED_PREVIEW_README.md` before opening.\n" +
        "Do not disable Gatekeeper globally or recursively remove quarantine.";
}

let identity = #{
    schema_version: 1,
    kind: "agenterm-promotion-identity",
    marker: marker,
    body: body,
    body_sha256:
        rhai::crypto::sha256(rhai::bytes::from_text(body)),
    candidate_run_id: candidate_run_id,
    source_sha: source_sha,
    version: manifest.version,
    tag: manifest.expected_tag,
    manifest_sha256: manifest_sha256,
    mac_channel: mac_channel
};
rhai::runtime::atomic_write(
    output,
    rhai::json::stringify_pretty(identity) + "\n"
);
identity
