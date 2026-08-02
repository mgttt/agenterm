// Stage the validated executable set and publish its provenance atomically.
//
// Args:
//   REPO SOURCE_DIRECTORY DESTINATION_DIRECTORY PROFILE

import "scripts/rhai/lib/artifact_files" as artifact_files;
import "scripts/rhai/lib/artifact_manifest" as artifact_manifest;
import "scripts/rhai/lib/build_metadata" as build_metadata;

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

fn checked_git_prefix(repo) {
    let command = std::process::command("git");
    command.args(["-C", repo, "rev-parse", "--show-prefix"]);
    command.timeout(std::time::Duration::from_secs(10));
    let output = command.output();
    require(output.success, output.error("stage-build-git-root"));
    let prefix = output.stdout_text();
    prefix.trim();
    prefix
}

fn source_name_for_artifact(artifact_name) {
    if artifact_name == "agenterm-rhai.exe" {
        return "agenterm-rhai.exe";
    }
    if artifact_name == "agenterm-rhai" {
        return "agenterm-rhai";
    }
    artifact_name
}

require(
    args.len == 4,
    "expected: REPO SOURCE_DIRECTORY DESTINATION_DIRECTORY PROFILE"
);

let repo = std::path::absolute(args[0]).display;
let source_directory = std::path::absolute(args[1]).display;
let destination_directory = std::path::absolute(args[2]).display;
let profile = args[3];
require(
    checked_git_prefix(repo) == "",
    "stage_build_repo_not_exact_git_root:" + repo
);
require(
    profile == "dev"
        || profile == "release-fast"
        || profile == "release",
    "stage_build_profile_invalid:" + profile
);

std::fs::create_dir_all(destination_directory);
let artifact_manifest_path =
    std::path::join(repo, "scripts/artifacts.json").display;
let manifest = artifact_manifest::load(artifact_manifest_path);
let obsolete = [
    "agentermctl.exe",
    "agenterm-rhai.exe",
    "agenterm-rhai"
];

artifact_files::clean_locked(
    destination_directory,
    manifest,
    obsolete
);
for artifact in manifest.executables {
    let source_name = source_name_for_artifact(artifact.name);
    if source_name == artifact.name {
        artifact_files::stage(
            source_directory,
            destination_directory,
            source_name
        );
    } else {
        artifact_files::stage_as(
            source_directory,
            destination_directory,
            source_name,
            artifact.name
        );
    }
}

let output_manifest =
    std::path::join(destination_directory, "agenterm.json").display;
build_metadata::write(
    repo,
    output_manifest,
    artifact_manifest_path,
    destination_directory,
    profile
);
artifact_files::clean_locked(
    destination_directory,
    manifest,
    obsolete
);
print("PASS: staged build artifacts and metadata in " + destination_directory);
