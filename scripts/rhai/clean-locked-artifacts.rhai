// Remove stale staged executables left behind by an atomic replacement.
//
// Args:
//   DIRECTORY ARTIFACT_MANIFEST [OBSOLETE_NAME...]

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
    args.len >= 2,
    "expected: DIRECTORY ARTIFACT_MANIFEST [OBSOLETE_NAME...]"
);

let directory = std::path::absolute(args[0]).display;
if !std::fs::exists(directory) {
    return;
}
require(
    std::fs::metadata(directory).is_dir,
    "locked_artifact_directory_not_directory"
);

let manifest_path = std::path::absolute(args[1]).display;
let manifest = rhai::json::parse(std::fs::read_to_string(manifest_path));
require(
    manifest.schema_version == 2,
    "artifact_manifest_schema_unsupported"
);
require(
    type_of(manifest.executables) == "array"
        && manifest.executables.len > 0,
    "artifact_manifest_executables_empty"
);

let names = #{};
for entry in manifest.executables {
    let name = entry.name;
    require(is_artifact_name(name), "artifact_manifest_name_invalid:" + name);
    require(!names.contains(name), "artifact_manifest_name_duplicate:" + name);
    names[name] = true;
}

let removed = 0;
let retained = 0;
for name in names.keys() {
    let stem = name;
    stem.replace(".exe", "");
    let prefix = stem + ".locked-";
    for entry in std::fs::read_dir(directory) {
        if entry.is_file
                && entry.file_name.starts_with(prefix)
                && entry.file_name.ends_with(".exe") {
            try {
                std::fs::remove_file(entry.path.display);
                removed += 1;
            } catch (error) {
                retained += 1;
            }
        }
    }
}

if removed > 0 {
    print(
        "Removed " + removed +
        " stale locked artifact(s) from " + directory
    );
}
if retained > 0 {
    print(
        "Retained " + retained +
        " locked artifact(s) still in use; the next build will retry."
    );
}

if args.len > 2 {
    for index in 2..args.len {
        let name = args[index];
        require(
            name != ""
                && !name.contains("/")
                && !name.contains("\\")
                && name != "."
                && name != "..",
            "obsolete_artifact_name_invalid:" + name
        );
        let obsolete_path = std::path::join(directory, name).display;
        if std::fs::exists(obsolete_path) {
            require(
                std::fs::metadata(obsolete_path).is_file,
                "obsolete_artifact_not_file:" + name
            );
            std::fs::remove_file(obsolete_path);
            print("Removed obsolete artifact " + name + " from " + directory);
        }
    }
}
