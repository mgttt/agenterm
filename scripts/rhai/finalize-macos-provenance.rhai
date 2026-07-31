// Mark one already accepted, signed macOS archive provenance as notarized.
//
// The caller owns Apple notarytool submission and may invoke this task only
// after the exact archive receives an Accepted result.
//
// Args: ARCHIVE PROVENANCE_JSON

fn require(condition, code) {
    if !condition {
        throw code;
    }
}

require(args.len == 2, "finalize_macos_provenance_arguments");
let archive = std::path::absolute(args[0]).display;
let provenance_path = std::path::absolute(args[1]).display;
require(
    std::fs::exists(archive) && std::fs::metadata(archive).is_file,
    "finalize_macos_provenance_archive"
);
require(
    std::fs::exists(provenance_path)
        && std::fs::metadata(provenance_path).is_file,
    "finalize_macos_provenance_input"
);
let provenance = rhai::json::parse_file(provenance_path);
require(
    provenance.schema_version == 1
        && provenance.product == "AgenTerm"
        && provenance.os == "macos"
        && provenance.channel == "release"
        && provenance.signed
        && !provenance.notarized,
    "finalize_macos_provenance_state"
);
require(
    provenance.sha256 == rhai::crypto::sha256_file(archive),
    "finalize_macos_provenance_archive_hash"
);
provenance.notarized = true;
rhai::runtime::atomic_write(
    provenance_path,
    rhai::json::stringify_pretty(provenance) + "\n"
);
print("NOTARIZED PROVENANCE " + provenance_path);
