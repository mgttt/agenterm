-- build_identity.lua — Freeze truthful source inputs into a batch-importable build environment.
-- Aligned with rh scripts/rh/lib/build_identity.rh semantics.
-- Args: repo, profile, output_path

local build_identity = {}

local function is_lower_hex(value, expected_length)
    if #value ~= expected_length then
        return false
    end
    for i = 1, #value do
        local c = value:sub(i, i)
        if not string.find("0123456789abcdef", c, 1, true) then
            return false
        end
    end
    return true
end

local function require_condition(cond, msg)
    if not cond then
        error(msg)
    end
end

--- Write build identity batch file.
--- @param repo string     Path to git repository root
--- @param profile string  Build profile: dev, release-fast, or release
--- @param output_path string  Output batch file path
function build_identity.write(repo, profile, output_path)
    -- Validate profile
    require_condition(
        profile == "dev" or profile == "release-fast" or profile == "release",
        "build_identity_profile_invalid:" .. profile
    )

    -- Git: verify we are at repo root
    local prefix_result = std.process.stdout_file(
        "git", {"-C", repo, "rev-parse", "--show-prefix"},
        repo .. ".build-identity.stdout.tmp",
        10000
    )
    require_condition(prefix_result.success, "build-identity-git-root")
    local prefix = std.fs.read(repo .. ".build-identity.stdout.tmp")
    prefix = prefix:gsub("^%s+", ""):gsub("%s+$", "")  -- trim
    require_condition(prefix == "", "build_identity_repo_not_exact_git_root:" .. repo)

    -- Git: get HEAD commit
    local commit_result = std.process.stdout_file(
        "git", {"-C", repo, "rev-parse", "--verify", "HEAD"},
        repo .. ".build-identity.stdout.tmp",
        10000
    )
    require_condition(commit_result.success, "build-identity-git-commit")
    local commit = std.fs.read(repo .. ".build-identity.stdout.tmp")
    commit = commit:gsub("^%s+", ""):gsub("%s+$", ""):lower()
    require_condition(
        is_lower_hex(commit, 40) or is_lower_hex(commit, 64),
        "build_identity_git_commit_invalid"
    )

    -- Git: check dirty status
    local status_result = std.process.stdout_file(
        "git", {"-C", repo, "status", "--porcelain=v1", "--untracked-files=normal"},
        repo .. ".build-identity.stdout.tmp",
        10000
    )
    require_condition(status_result.success, "build-identity-git-status")
    local status = std.fs.read(repo .. ".build-identity.stdout.tmp")
    status = status:gsub("^%s+", ""):gsub("%s+$", "")

    local dirty = false
    if status ~= "" then
        dirty = true
    end

    -- Hash Cargo.lock and artifacts manifest
    local cargo_lock = std.path.join(repo, "Cargo.lock")
    local artifact_manifest = std.path.join(repo, "scripts/artifacts.json")
    require_condition(std.fs.exists(cargo_lock), "build_identity_cargo_lock_missing")
    require_condition(std.fs.exists(artifact_manifest), "build_identity_artifact_manifest_missing")

    local cargo_lock_sha256 = std.crypto.sha256_file(cargo_lock)
    local artifact_manifest_sha256 = std.crypto.sha256_file(artifact_manifest)
    require_condition(
        is_lower_hex(cargo_lock_sha256, 64),
        "build_identity_cargo_lock_hash_invalid"
    )
    require_condition(
        is_lower_hex(artifact_manifest_sha256, 64),
        "build_identity_artifact_manifest_hash_invalid"
    )

    -- Build batch file content
    local dirty_label = "false"
    if dirty then
        dirty_label = "true"
    end

    local batch =
        "set \"AGENTERM_BUILD_IDENTITY_VERSION=1\"\n" ..
        "set \"AGENTERM_BUILD_GIT_COMMIT=" .. commit .. "\"\n" ..
        "set \"AGENTERM_BUILD_GIT_DIRTY=" .. dirty_label .. "\"\n" ..
        "set \"AGENTERM_BUILD_CARGO_LOCK_SHA256=" .. cargo_lock_sha256 .. "\"\n" ..
        "set \"AGENTERM_BUILD_ARTIFACT_MANIFEST_SHA256=" .. artifact_manifest_sha256 .. "\"\n" ..
        "set \"AGENTERM_BUILD_PROFILE=" .. profile .. "\"\n"

    -- Atomic write
    std.fs.write(output_path, batch)

    -- Cleanup temp file
    pcall(function() std.fs.write(repo .. ".build-identity.stdout.tmp", "") end)

    print(
        "Prepared build identity for " .. commit ..
        " (" .. profile .. ", dirty=" .. dirty_label .. ")"
    )
    return 0
end

return build_identity
