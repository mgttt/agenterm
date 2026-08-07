// Derive machine-readable releases.json from one sealed Candidate.
//
// Pure index: every field is copied or mapped from the candidate-manifest
// and per-archive .provenance.json companions.  No second release truth.
//
// Usage:
//   agenterm-rhai run scripts/rhai/build-releases-index.rhai -- \
//     CANDIDATE_MANIFEST ASSET_DIRECTORY OUTPUT_JSON
//
// H1 (v0.1.15): GitHub Release asset / workflow handoff; Pages host is
// optional (P5) and not required for this leaf.

fn require(condition, code) {
    if !condition {
        throw code;
    }
}

fn optional_sbom(provenance) {
    try {
        return provenance.sbom_sha256.to_string();
    } catch {
        return "";
    }
}

fn optional_build_log(provenance) {
    try {
        return provenance.build_log.to_string();
    } catch {
        return "";
    }
}

fn checksum_name_of(asset) {
    try {
        let name = asset.checksum.name;
        if type_of(name) == "string" && name != "" {
            return name;
        }
    } catch {
        // fall through
    }
    asset.name + ".sha256"
}

fn checksum_sha_of(asset) {
    try {
        return asset.checksum.sha256.to_string();
    } catch {
        return "";
    }
}

fn distribution_for_channel(channel) {
    if channel == "macos-unsigned-preview" {
        return "preview";
    }
    if channel == "release" {
        return "stable";
    }
    throw "releases_index_unknown_channel:" + channel;
}

fn variant_for_channel(channel) {
    if channel == "macos-unsigned-preview" {
        return "unsigned-preview";
    }
    if channel == "release" {
        return "release";
    }
    throw "releases_index_unknown_channel:" + channel;
}

if args.len == 1 && (args[0] == "--help" || args[0] == "-h") {
    print(
        "usage: build-releases-index CANDIDATE_MANIFEST " +
            "ASSET_DIRECTORY OUTPUT_JSON"
    );
    print(
        "derives schema-v1 releases.json from a sealed candidate " +
            "manifest and payload provenance companions"
    );
    return;
}
require(
    args.len == 3,
    "releases_index_arguments: expected CANDIDATE_MANIFEST " +
        "ASSET_DIRECTORY OUTPUT_JSON"
);

let manifest_path = std::path::absolute(args[0]).display;
let asset_root = std::path::absolute(args[1]).display;
let output_path = std::path::absolute(args[2]).display;
require(
    std::fs::exists(manifest_path) && std::fs::metadata(manifest_path).is_file,
    "releases_index_manifest_missing"
);
require(
    std::fs::exists(asset_root) && std::fs::metadata(asset_root).is_dir,
    "releases_index_assets_missing"
);

let manifest = rhai::json::parse_file(manifest_path);
require(
    manifest.schema_version == 1
        && manifest.product == "AgenTerm"
        && type_of(manifest.assets) == "array"
        && manifest.assets.len > 0
        && manifest.version != ""
        && manifest.expected_tag == "v" + manifest.version
        && manifest.source_sha != "",
    "releases_index_manifest"
);

let artifacts = [];
let has_stable = false;
let has_preview = false;
for asset in manifest.assets {
    require(
        asset.name != ""
            && asset.os != ""
            && asset.arch != ""
            && asset.channel != ""
            && asset.sha256 != ""
            && type_of(asset.provenance) == "map"
            && asset.provenance.name != ""
            && asset.provenance.sha256 != "",
        "releases_index_asset:" + asset.name
    );
    let distribution = distribution_for_channel(asset.channel);
    let variant = variant_for_channel(asset.channel);
    if distribution == "stable" {
        has_stable = true;
    }
    if distribution == "preview" {
        has_preview = true;
    }

    let provenance_path =
        std::path::join(asset_root, asset.provenance.name).display;
    require(
        std::fs::exists(provenance_path)
            && std::fs::metadata(provenance_path).is_file,
        "releases_index_provenance_missing:" + asset.provenance.name
    );
    let provenance_hash = rhai::crypto::sha256_file(provenance_path);
    require(
        provenance_hash == asset.provenance.sha256,
        "releases_index_provenance_hash:" + asset.provenance.name
    );
    let provenance = rhai::json::parse_file(provenance_path);
    require(
        provenance.schema_version == 1
            && provenance.product == "AgenTerm"
            && provenance.version == manifest.version
            && provenance.artifact == asset.name
            && provenance.sha256 == asset.sha256
            && provenance.os == asset.os
            && provenance.arch == asset.arch
            && provenance.channel == asset.channel
            && provenance.signed == asset.signed
            && provenance.notarized == asset.notarized
            && provenance.source_commit == manifest.source_sha
            && provenance.source_tag == manifest.expected_tag,
        "releases_index_provenance_mismatch:" + asset.name
    );

    artifacts.push(#{
        os: asset.os,
        arch: asset.arch,
        variant: variant,
        channel: asset.channel,
        distribution: distribution,
        name: asset.name,
        sha256: asset.sha256,
        size: asset.size,
        signed: asset.signed,
        notarized: asset.notarized,
        provenance: #{
            name: asset.provenance.name,
            sha256: asset.provenance.sha256
        },
        checksum: #{
            name: checksum_name_of(asset),
            sha256: checksum_sha_of(asset)
        },
        source_commit: provenance.source_commit,
        source_tag: provenance.source_tag,
        sbom_sha256: optional_sbom(provenance),
        build_log: optional_build_log(provenance)
    });
}

require(artifacts.len == manifest.assets.len, "releases_index_artifact_count");

let channel_ptr = #{
    version: manifest.version,
    tag: manifest.expected_tag,
    source_commit: manifest.source_sha
};
let channels = #{};
if has_stable {
    channels.stable = channel_ptr;
}
if has_preview {
    channels.preview = channel_ptr;
}
require(
    has_stable || has_preview,
    "releases_index_empty_channels"
);

let release_entry = #{
    version: manifest.version,
    tag: manifest.expected_tag,
    source_commit: manifest.source_sha,
    artifacts: artifacts
};

let index = #{
    schema_version: 1,
    kind: "agenterm-releases-index",
    product: "AgenTerm",
    source: #{
        kind: "agenterm-release-candidate",
        source_sha: manifest.source_sha,
        version: manifest.version,
        tag: manifest.expected_tag,
        candidate_run_id: manifest.run.id,
        candidate_run_attempt: manifest.run.attempt,
        manifest_sha256: rhai::crypto::sha256_file(manifest_path)
    },
    channels: channels,
    releases: [release_entry]
};

rhai::runtime::atomic_write(
    output_path,
    rhai::json::stringify_pretty(index) + "\n"
);
print(
    "RELEASES_INDEX " +
        manifest.expected_tag +
        " artifacts=" +
        artifacts.len +
        " " +
        output_path
);
