// Produce the staged build provenance manifest from frozen or live inputs.
//
// Args:
//   REPO OUTPUT_MANIFEST ARTIFACT_MANIFEST STAGED_DIRECTORY PROFILE

import "scripts/rhai/lib/artifact_manifest" as artifact_manifest;

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

fn is_lower_hex(value, expected_length) {
    if value.len != expected_length {
        return false;
    }
    for character in value {
        if !"0123456789abcdef".contains(character.to_string()) {
            return false;
        }
    }
    true
}

fn is_git_commit(value) {
    is_lower_hex(value, 40) || is_lower_hex(value, 64)
}

fn checked_output(program, arguments, timeout_seconds, error_code) {
    let command = std::process::command(program);
    command.args(arguments);
    command.timeout(std::time::Duration::from_secs(timeout_seconds));
    let output = command.output();
    require(output.success, output.error(error_code));
    output.stdout_text()
}

fn git_output(repo, arguments, error_code) {
    let argv = ["-C", repo];
    argv.append(arguments);
    checked_output("git", argv, 10, error_code)
}

fn cargo_package_version(repo) {
    let cargo_toml =
        std::fs::read_to_string(std::path::join(repo, "Cargo.toml").display);
    let in_package = false;
    for source_line in cargo_toml.split("\n") {
        let line = source_line;
        line.trim();
        if line == "[package]" {
            in_package = true;
            continue;
        }
        if line.starts_with("[") {
            in_package = false;
        }
        if in_package && line.starts_with("version") {
            let parts = line.split("\"");
            require(
                parts.len >= 3 && parts[1] != "",
                "build_metadata_package_version_invalid"
            );
            return parts[1];
        }
    }
    throw "build_metadata_package_version_missing";
}

fn tracked_diff_exit(repo, cached) {
    let command = std::process::command("git");
    command.args(["-C", repo, "diff"]);
    if cached {
        command.arg("--cached");
    }
    command.arg("--quiet");
    command.timeout(std::time::Duration::from_secs(10));
    command.output().exit_code
}

require(
    args.len == 5,
    "expected: REPO OUTPUT_MANIFEST ARTIFACT_MANIFEST STAGED_DIRECTORY PROFILE"
);

let repo = std::path::absolute(args[0]).display;
let output_manifest = std::path::absolute(args[1]).display;
let artifact_manifest_path = std::path::absolute(args[2]).display;
let staged_directory = std::path::absolute(args[3]).display;
let profile = args[4];
require(
    profile == "dev"
        || profile == "release-fast"
        || profile == "release",
    "build_metadata_profile_invalid:" + profile
);

let prefix = git_output(
    repo,
    ["rev-parse", "--show-prefix"],
    "build-metadata-git-root"
);
prefix.trim();
require(prefix == "", "build_metadata_repo_not_exact_git_root:" + repo);

let staged_metadata = std::fs::symlink_metadata(staged_directory);
require(
    staged_metadata.is_dir
        && !staged_metadata.is_symlink
        && !staged_metadata.is_reparse_point,
    "build_metadata_staged_directory_invalid:" + staged_directory
);

let artifacts = artifact_manifest::load(artifact_manifest_path);
let cargo_lock = std::path::join(repo, "Cargo.lock").display;
let cargo_lock_sha256 = rhai::crypto::sha256_file(cargo_lock);
let artifact_manifest_sha256 =
    rhai::crypto::sha256_file(artifact_manifest_path);

let identity_available =
    std::env::has("AGENTERM_BUILD_IDENTITY_VERSION")
        && std::env::get("AGENTERM_BUILD_IDENTITY_VERSION") == "1";
let commit = "";
let dirty = false;
if identity_available {
    let frozen_profile = std::env::get("AGENTERM_BUILD_PROFILE");
    require(
        frozen_profile == profile,
        "build_metadata_frozen_profile_mismatch:" +
            frozen_profile + ":" + profile
    );

    commit = std::env::get("AGENTERM_BUILD_GIT_COMMIT");
    commit.to_lower();
    require(
        is_git_commit(commit),
        "build_metadata_frozen_commit_invalid"
    );
    let current_commit = git_output(
        repo,
        ["rev-parse", "--verify", "HEAD"],
        "build-metadata-git-head"
    );
    current_commit.trim();
    current_commit.to_lower();
    require(
        current_commit == commit,
        "build_metadata_git_head_changed"
    );

    let worktree_diff = tracked_diff_exit(repo, false);
    let index_diff = tracked_diff_exit(repo, true);
    require(
        (worktree_diff == 0 || worktree_diff == 1)
            && (index_diff == 0 || index_diff == 1),
        "build_metadata_tracked_state_unavailable"
    );

    let dirty_text = std::env::get("AGENTERM_BUILD_GIT_DIRTY");
    require(
        dirty_text == "true" || dirty_text == "false",
        "build_metadata_frozen_dirty_invalid:" + dirty_text
    );
    dirty = dirty_text == "true";
    require(
        dirty || (worktree_diff == 0 && index_diff == 0),
        "build_metadata_clean_identity_changed"
    );

    require(
        std::env::get("AGENTERM_BUILD_CARGO_LOCK_SHA256")
                == cargo_lock_sha256
            && std::env::get(
                "AGENTERM_BUILD_ARTIFACT_MANIFEST_SHA256"
            ) == artifact_manifest_sha256,
        "build_metadata_frozen_input_hash_changed"
    );
} else {
    commit = git_output(
        repo,
        ["rev-parse", "HEAD"],
        "build-metadata-git-commit"
    );
    commit.trim();
    commit.to_lower();
    require(is_git_commit(commit), "build_metadata_git_commit_invalid");

    let status = git_output(
        repo,
        ["status", "--porcelain"],
        "build-metadata-git-status"
    );
    status.trim();
    dirty = status != "";
}

let rust_metadata = checked_output(
    "rustc",
    ["-vV"],
    10,
    "build-metadata-rustc"
);
let rust_lines = rust_metadata.split("\n");
require(
    rust_lines.len > 0 && rust_lines[0].trim() != "",
    "build_metadata_rust_version_missing"
);
let rust_version = rust_lines[0];
rust_version.trim();
let rust_target = "";
for source_line in rust_lines {
    let line = source_line;
    line.trim();
    if line.starts_with("host: ") {
        rust_target = line;
        rust_target.replace("host: ", "");
        rust_target.trim();
        break;
    }
}
require(rust_target != "", "build_metadata_rust_target_missing");

let executable_metadata = [];
for artifact in artifacts.executables {
    let path =
        std::path::join(staged_directory, artifact.name).display;
    let metadata = std::fs::symlink_metadata(path);
    require(
        metadata.is_file
            && !metadata.is_symlink
            && !metadata.is_reparse_point,
        "build_metadata_executable_invalid:" + artifact.name
    );
    executable_metadata.push(#{
        name: artifact.name,
        role: artifact.role,
        size: metadata.len,
        sha256: rhai::crypto::sha256_file(path)
    });
}

let build_time = std::time::SystemTime::now().rfc3339;
let build_time_parts = build_time.split(".");
require(
    build_time_parts.len == 2,
    "build_metadata_time_render_invalid"
);

let manifest = #{
    schema_version: 2,
    product: "AgenTerm",
    version: cargo_package_version(repo),
    build_time_utc: build_time_parts[0] + "Z",
    git_commit: commit,
    git_dirty: dirty,
    profile: profile,
    rust_target: rust_target,
    rust_version: rust_version,
    cargo_lock_sha256: cargo_lock_sha256,
    artifact_manifest_sha256: artifact_manifest_sha256,
    features: [
        "codex-launcher",
        "hierarchical-tabs",
        "mux-frontend",
        "persistent-workspace",
        "safe-scripting",
        "tab-environment"
    ],
    executables: executable_metadata
};
rhai::runtime::atomic_write(
    output_manifest,
    rhai::json::stringify_pretty(manifest) + "\n"
);
print(
    "Wrote build metadata for " + commit +
    " (" + profile + ", dirty=" + dirty + ")"
);
