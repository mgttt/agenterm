// Stage one validated AgenTerm executable, parking an in-use Windows image.
//
// Args:
//   SOURCE_DIRECTORY DESTINATION_DIRECTORY ARTIFACT_NAME

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

fn is_artifact_name(name) {
    if name == "agenterm.exe" {
        return true;
    }
    if !name.starts_with("agenterm-") || !name.ends_with(".exe") {
        return false;
    }
    if name.contains("/") || name.contains("\\") {
        return false;
    }
    let role = name;
    role.replace("agenterm-", "");
    role.replace(".exe", "");
    if role == "" {
        return false;
    }
    for character in role {
        if !"abcdefghijklmnopqrstuvwxyz".contains(character.to_string()) {
            return false;
        }
    }
    true
}

require(
    args.len == 3,
    "expected: SOURCE_DIRECTORY DESTINATION_DIRECTORY ARTIFACT_NAME"
);

let source_directory = std::path::absolute(args[0]).display;
let destination_directory = std::path::absolute(args[1]).display;
let name = args[2];
require(is_artifact_name(name), "stage_artifact_name_invalid:" + name);

require(
    std::fs::exists(source_directory)
        && std::fs::metadata(source_directory).is_dir,
    "stage_artifact_source_directory_invalid:" + source_directory
);
let source_directory_metadata = std::fs::symlink_metadata(source_directory);
require(
    !source_directory_metadata.is_symlink
        && !source_directory_metadata.is_reparse_point,
    "stage_artifact_source_directory_indirect:" + source_directory
);

std::fs::create_dir_all(destination_directory);
let destination_directory_metadata =
    std::fs::symlink_metadata(destination_directory);
require(
    destination_directory_metadata.is_dir
        && !destination_directory_metadata.is_symlink
        && !destination_directory_metadata.is_reparse_point,
    "stage_artifact_destination_directory_invalid:" + destination_directory
);

let source = std::path::join(source_directory, name).display;
let destination = std::path::join(destination_directory, name).display;
require(std::fs::exists(source), "stage_artifact_source_missing:" + source);
let source_metadata = std::fs::symlink_metadata(source);
require(
    source_metadata.is_file
        && !source_metadata.is_symlink
        && !source_metadata.is_reparse_point,
    "stage_artifact_source_invalid:" + source
);

try {
    std::fs::copy(source, destination);
    print("Staged artifact: " + destination);
    return;
} catch (initial_error) {
    require(
        std::fs::exists(destination),
        "stage_artifact_copy_failed:" + initial_error.to_string()
    );

    let destination_metadata = std::fs::symlink_metadata(destination);
    require(
        destination_metadata.is_file
            && !destination_metadata.is_symlink
            && !destination_metadata.is_reparse_point,
        "stage_artifact_destination_invalid:" + destination
    );

    let stem = name;
    stem.replace(".exe", "");
    let suffix = std::time::SystemTime::now().unix_millis;
    let parked_name = stem + ".locked-" + suffix + ".exe";
    let parked = std::path::join(destination_directory, parked_name).display;
    std::fs::rename(destination, parked);
    try {
        std::fs::copy(source, destination);
    } catch (replacement_error) {
        try {
            std::fs::rename(parked, destination);
        } catch (rollback_error) {
            throw (
                "stage_artifact_copy_and_rollback_failed:" +
                replacement_error.to_string() + ":" +
                rollback_error.to_string()
            );
        }
        throw (
            "stage_artifact_copy_after_park_failed:" +
            replacement_error.to_string()
        );
    }

    print(
        "Staged new artifact; running copy remains at " + parked
    );
}
