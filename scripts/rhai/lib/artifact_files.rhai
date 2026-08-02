// Shared filesystem operations for validated AgenTerm build artifacts.

import "scripts/rhai/lib/artifact_manifest" as artifact_manifest;

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

fn clean_locked(directory_value, manifest, obsolete_names) {
    let directory = std::path::absolute(directory_value).display;
    if !std::fs::exists(directory) {
        return #{ removed: 0, retained: 0, obsolete_removed: 0 };
    }
    require(
        std::fs::metadata(directory).is_dir,
        "locked_artifact_directory_not_directory"
    );

    let names = #{};
    for entry in manifest.executables {
        let name = entry.name;
        require(
            artifact_manifest::is_artifact_name(name),
            "artifact_manifest_name_invalid:" + name
        );
        require(
            !names.contains(name),
            "artifact_manifest_name_duplicate:" + name
        );
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

    let obsolete_removed = 0;
    for name in obsolete_names {
        let name = name;
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
            obsolete_removed += 1;
            print("Removed obsolete artifact " + name + " from " + directory);
        }
    }

    #{
        removed: removed,
        retained: retained,
        obsolete_removed: obsolete_removed
    }
}

fn stage(source_directory_value, destination_directory_value, name) {
    let source_directory =
        std::path::absolute(source_directory_value).display;
    let destination_directory =
        std::path::absolute(destination_directory_value).display;
    require(
        artifact_manifest::is_artifact_name(name),
        "stage_artifact_name_invalid:" + name
    );

    require(
        std::fs::exists(source_directory)
            && std::fs::metadata(source_directory).is_dir,
        "stage_artifact_source_directory_invalid:" + source_directory
    );
    let source_directory_metadata =
        std::fs::symlink_metadata(source_directory);
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
        "stage_artifact_destination_directory_invalid:" +
            destination_directory
    );

    let source = std::path::join(source_directory, name).display;
    let destination =
        std::path::join(destination_directory, name).display;
    require(
        std::fs::exists(source),
        "stage_artifact_source_missing:" + source
    );
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
        return #{ destination: destination, parked: "" };
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
        let parked =
            std::path::join(destination_directory, parked_name).display;
        std::fs::rename(destination, parked);
        try {
            std::fs::copy(source, destination);
        } catch (replacement_error) {
            try {
                std::fs::rename(parked, destination);
            } catch (rollback_error) {
                let failure =
                    "stage_artifact_copy_and_rollback_failed:" +
                    replacement_error.to_string() + ":" +
                    rollback_error.to_string();
                throw failure;
            }
            let failure =
                "stage_artifact_copy_after_park_failed:" +
                replacement_error.to_string();
            throw failure;
        }

        print("Staged new artifact; running copy remains at " + parked);
        #{ destination: destination, parked: parked }
    }
}

fn stage_as(
    source_directory_value,
    destination_directory_value,
    source_name,
    destination_name
) {
    let source_directory =
        std::path::absolute(source_directory_value).display;
    let destination_directory =
        std::path::absolute(destination_directory_value).display;
    require(
        artifact_manifest::is_artifact_name(destination_name),
        "stage_artifact_as_destination_invalid:" + destination_name
    );

    require(
        source_name != "",
        "stage_artifact_as_source_empty"
    );

    let source = std::path::join(source_directory, source_name).display;
    let destination =
        std::path::join(destination_directory, destination_name).display;
    require(
        std::fs::exists(source),
        "stage_artifact_as_source_missing:" + source
    );
    let source_metadata = std::fs::symlink_metadata(source);
    require(
        source_metadata.is_file
            && !source_metadata.is_symlink
            && !source_metadata.is_reparse_point,
        "stage_artifact_as_source_invalid:" + source
    );

    try {
        std::fs::copy(source, destination);
        print("Staged artifact: " + destination);
        return #{ destination: destination, parked: "" };
    } catch (initial_error) {
        require(
            std::fs::exists(destination),
            "stage_artifact_as_copy_failed:" + initial_error.to_string()
        );

        let destination_metadata = std::fs::symlink_metadata(destination);
        require(
            destination_metadata.is_file
                && !destination_metadata.is_symlink
                && !destination_metadata.is_reparse_point,
            "stage_artifact_as_destination_invalid:" + destination
        );

        let stem = destination_name;
        stem.replace(".exe", "");
        let suffix = std::time::SystemTime::now().unix_millis;
        let parked_name = stem + ".locked-" + suffix + ".exe";
        let parked =
            std::path::join(destination_directory, parked_name).display;
        std::fs::rename(destination, parked);
        try {
            std::fs::copy(source, destination);
        } catch (replacement_error) {
            try {
                std::fs::rename(parked, destination);
            } catch (rollback_error) {
                let failure =
                    "stage_artifact_as_copy_and_rollback_failed:" +
                    replacement_error.to_string() + ":" +
                    rollback_error.to_string();
                throw failure;
            }
            let failure =
                "stage_artifact_as_copy_after_park_failed:" +
                replacement_error.to_string();
            throw failure;
        }

        print("Staged new artifact; running copy remains at " + parked);
        #{ destination: destination, parked: parked }
    }
}
