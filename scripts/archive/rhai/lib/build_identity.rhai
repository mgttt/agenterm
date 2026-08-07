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

fn facts(repo, profile) {
    let repo = std::path::absolute(repo).display;
    require(
        profile == "dev"
            || profile == "release-fast"
            || profile == "release",
        "build_identity_profile_invalid:" + profile
    );

    let prefix = checked_git(
        repo,
        ["rev-parse", "--show-prefix"],
        "build-identity-git-root"
    );
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

    let cargo_lock = std::path::join(repo, "Cargo.lock").display;
    let artifact_manifest =
        std::path::join(repo, "scripts/artifacts.json").display;
    require(std::fs::exists(cargo_lock), "build_identity_cargo_lock_missing");
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

    #{
        schema_version: 1,
        commit: commit,
        dirty: status != "",
        cargo_lock_sha256: cargo_lock_sha256,
        artifact_manifest_sha256: artifact_manifest_sha256,
        profile: profile
    }
}

fn environment(identity) {
    #{
        AGENTERM_BUILD_IDENTITY_VERSION: identity.schema_version.to_string(),
        AGENTERM_BUILD_GIT_COMMIT: identity.commit,
        AGENTERM_BUILD_GIT_DIRTY: identity.dirty.to_string(),
        AGENTERM_BUILD_CARGO_LOCK_SHA256: identity.cargo_lock_sha256,
        AGENTERM_BUILD_ARTIFACT_MANIFEST_SHA256:
            identity.artifact_manifest_sha256,
        AGENTERM_BUILD_PROFILE: identity.profile
    }
}

fn batch_environment(identity) {
    let environment = environment(identity);
    "set \"AGENTERM_BUILD_IDENTITY_VERSION=" +
        environment.AGENTERM_BUILD_IDENTITY_VERSION + "\"\n" +
    "set \"AGENTERM_BUILD_GIT_COMMIT=" +
        environment.AGENTERM_BUILD_GIT_COMMIT + "\"\n" +
    "set \"AGENTERM_BUILD_GIT_DIRTY=" +
        environment.AGENTERM_BUILD_GIT_DIRTY + "\"\n" +
    "set \"AGENTERM_BUILD_CARGO_LOCK_SHA256=" +
        environment.AGENTERM_BUILD_CARGO_LOCK_SHA256 + "\"\n" +
    "set \"AGENTERM_BUILD_ARTIFACT_MANIFEST_SHA256=" +
        environment.AGENTERM_BUILD_ARTIFACT_MANIFEST_SHA256 + "\"\n" +
    "set \"AGENTERM_BUILD_PROFILE=" +
        environment.AGENTERM_BUILD_PROFILE + "\"\n"
}
