// Freeze truthful source inputs into a batch-importable build environment.
//
// Args:
//   REPO PROFILE OUTPUT_PATH

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

fn checked_git(repo, arguments, error_code) {
    let command = std::process::command("git");
    command.args(["-C", repo]);
    command.args(arguments);
    command.timeout(std::time::Duration::from_secs(10));
    let output = command.output();
    require(output.success, output.error(error_code));
    output.stdout_text()
}

require(args.len == 3, "expected: REPO PROFILE OUTPUT_PATH");

let repo = std::path::absolute(args[0]).display;
let profile = args[1];
let output_path = std::path::absolute(args[2]).display;
require(
    profile == "dev"
        || profile == "release-fast"
        || profile == "release",
    "build_identity_profile_invalid:" + profile
);

let prefix = checked_git(repo, ["rev-parse", "--show-prefix"], "build-identity-git-root");
prefix.trim();
require(prefix == "", "build_identity_repo_not_exact_git_root:" + repo);

let commit = checked_git(
    repo,
    ["rev-parse", "--verify", "HEAD"],
    "build-identity-git-commit"
);
commit.trim();
commit.to_lower();
require(
    is_lower_hex(commit, 40) || is_lower_hex(commit, 64),
    "build_identity_git_commit_invalid"
);

let status = checked_git(
    repo,
    ["status", "--porcelain=v1", "--untracked-files=normal"],
    "build-identity-git-status"
);
status.trim();
let dirty = status != "";

let cargo_lock = std::path::join(repo, "Cargo.lock").display;
let artifact_manifest =
    std::path::join(repo, "scripts/artifacts.json").display;
require(
    std::fs::exists(cargo_lock),
    "build_identity_cargo_lock_missing"
);
require(
    std::fs::exists(artifact_manifest),
    "build_identity_artifact_manifest_missing"
);
let cargo_lock_sha256 = rhai::crypto::sha256_file(cargo_lock);
let artifact_manifest_sha256 =
    rhai::crypto::sha256_file(artifact_manifest);
require(
    is_lower_hex(cargo_lock_sha256, 64),
    "build_identity_cargo_lock_hash_invalid"
);
require(
    is_lower_hex(artifact_manifest_sha256, 64),
    "build_identity_artifact_manifest_hash_invalid"
);

let content =
    "set \"AGENTERM_BUILD_IDENTITY_VERSION=1\"\n" +
    "set \"AGENTERM_BUILD_GIT_COMMIT=" + commit + "\"\n" +
    "set \"AGENTERM_BUILD_GIT_DIRTY=" + dirty + "\"\n" +
    "set \"AGENTERM_BUILD_CARGO_LOCK_SHA256=" +
        cargo_lock_sha256 + "\"\n" +
    "set \"AGENTERM_BUILD_ARTIFACT_MANIFEST_SHA256=" +
        artifact_manifest_sha256 + "\"\n" +
    "set \"AGENTERM_BUILD_PROFILE=" + profile + "\"\n";
rhai::runtime::atomic_write(output_path, content);
print(
    "Prepared build identity for " + commit +
    " (" + profile + ", dirty=" + dirty + ")"
);
