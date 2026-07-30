// Qualify, package, rehearse, and explicitly publish one exact candidate.
//
// Args: REPO [validate|rehearse|publish|--rehearse]

import "scripts/rhai/lib/package_qualified" as package_qualified;

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

fn normalized(value) {
    let text = value.to_string();
    text.replace("\r\n", "\n");
    text.replace("\r", "\n");
    text
}

fn trimmed(value) {
    let text = normalized(value);
    text.trim();
    text
}

fn tail(value, maximum) {
    let text = normalized(value);
    if text.len <= maximum {
        return text;
    }
    text.sub_string(text.len - maximum, maximum)
}

fn command_output(program, arguments, repo, timeout_ms) {
    let command = std::process::command(program);
    command.args(arguments);
    command.current_dir(repo);
    command.timeout(std::time::Duration::from_millis(timeout_ms));
    command.output()
}

fn git_output(repo, arguments, code) {
    let argv = ["-C", repo];
    argv.append(arguments);
    let output = command_output("git", argv, repo, 30000);
    require(output.success, code);
    trimmed(output.stdout_text())
}

fn valid_identifier(value) {
    if value == "" {
        return false;
    }
    for character in value {
        let text = character.to_string();
        if !"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz.-"
                .contains(text) {
            return false;
        }
    }
    true
}

fn valid_numeric(value) {
    if value == "" || (value.len > 1 && value.starts_with("0")) {
        return false;
    }
    for character in value {
        if !"0123456789".contains(character.to_string()) {
            return false;
        }
    }
    true
}

fn valid_version(value) {
    let build_parts = value.split("+");
    if build_parts.len > 2
            || (build_parts.len == 2
                && !valid_identifier(build_parts[1])) {
        return false;
    }
    let public_version = build_parts[0];
    let prerelease_parts = public_version.split("-");
    if prerelease_parts.len > 2
            || (prerelease_parts.len == 2
                && !valid_identifier(prerelease_parts[1])) {
        return false;
    }
    let core = prerelease_parts[0].split(".");
    core.len == 3
        && valid_numeric(core[0])
        && valid_numeric(core[1])
        && valid_numeric(core[2])
}

fn package_version(repo) {
    let cargo =
        std::fs::read_to_string(std::path::join(repo, "Cargo.toml").display);
    let in_package = false;
    let version = "";
    for source_line in cargo.split("\n") {
        let line = source_line;
        line.trim();
        if line.starts_with("[") {
            in_package = line == "[package]";
            continue;
        }
        if in_package && line.starts_with("version") {
            let parts = line.split("\"");
            if parts.len >= 2 {
                version = parts[1];
                break;
            }
        }
    }
    require(
        valid_version(version),
        "release_version_invalid:" + version
    );
    version
}

fn assert_clean_main(repo) {
    require(
        git_output(repo, ["branch", "--show-current"], "release_git_branch")
            == "main",
        "release_branch_not_main"
    );
    require(
        git_output(
            repo,
            ["status", "--porcelain", "--untracked-files=normal"],
            "release_git_status"
        ) == "",
        "release_worktree_not_clean"
    );
}

fn assert_workflow(repo) {
    let workflow_path =
        std::path::join(repo, ".github/workflows/release.yml").display;
    let workflow = std::fs::read_to_string(workflow_path);
    require(
        workflow.contains("tags:")
            && (
                workflow.contains("- 'v*'")
                    || workflow.contains(`- "v*"`)
            )
            && workflow.contains("check.cmd --release --include-stress")
            && workflow.contains("expected == \"v0.1.7\""),
        "release_workflow_contract"
    );
    workflow_path
}

fn run_quality(repo) {
    let output_root =
        std::path::join(repo, "target/qualification").display;
    std::fs::create_dir_all(output_root);
    let stdout_path =
        std::path::join(output_root, "release-check.stdout").display;
    let stderr_path =
        std::path::join(output_root, "release-check.stderr").display;
    require(
        std::env::has("AGENTERM_BOOTSTRAP_WORKER"),
        "release_bootstrap_worker_missing"
    );
    let worker =
        std::path::absolute(std::env::get("AGENTERM_BOOTSTRAP_WORKER")).display;
    let command = std::process::command(worker);
    command.args([
        "task", "run", "check",
        "--manifest", std::path::join(repo, "agenterm.tasks.json").display,
        "--timeout-ms", "3600000",
        "--max-operations", "100000000",
        "--max-collection-items", "100000",
        "--max-string-bytes", "8388608",
        "--max-output-bytes", "1048576",
        "--", "--release", "--include-stress"
    ]);
    command.current_dir(repo);
    command.timeout(std::time::Duration::from_secs(3600));
    command.stdout_file(stdout_path);
    command.stderr_file(stderr_path);
    let output = command.output();
    if !output.success {
        let diagnostic =
            if std::fs::exists(stderr_path) {
                std::fs::read_to_string(stderr_path)
            } else {
                ""
            } +
            if std::fs::exists(stdout_path) {
                std::fs::read_to_string(stdout_path)
            } else {
                ""
            };
        throw "release_quality_gate:" + tail(diagnostic, 16384);
    }
    if std::fs::exists(stdout_path) {
        print(tail(std::fs::read_to_string(stdout_path), 4096));
    }
}

fn git_mutation(repo, arguments) {
    let argv = ["-C", repo];
    argv.append(arguments);
    command_output("git", argv, repo, 120000)
}

require(args.len >= 1 && args.len <= 2, "expected: REPO [--rehearse]");
let repo = std::path::absolute(args[0]).display;
let mode = if args.len == 1 {
    "publish"
} else if args[1] == "--rehearse" {
    "rehearse"
} else {
    args[1]
};
require(
    mode == "validate" || mode == "rehearse" || mode == "publish",
    "release_mode:" + mode
);

assert_clean_main(repo);
let version = package_version(repo);
require(
    version != "0.1.7",
    "release_internal_version_0.1.7"
);
let tag = "v" + version;
require(
    git_output(repo, ["tag", "--list", tag], "release_git_tags") == "",
    "release_tag_exists:" + tag
);
let workflow_path = assert_workflow(repo);
let head = git_output(repo, ["rev-parse", "HEAD"], "release_git_head");
if mode == "validate" {
    print(
        "VALID RELEASE PLAN " +
            rhai::json::stringify(#{
                version: version,
                tag: tag,
                source_commit: head,
                workflow_sha256:
                    rhai::crypto::sha256_file(workflow_path),
                mutation: false
            })
    );
    return;
}

print("QUALIFY AgenTerm " + version + " from main@" + head);
run_quality(repo);
assert_clean_main(repo);
require(
    git_output(repo, ["rev-parse", "HEAD"], "release_git_head_after_check")
        == head,
    "release_head_changed_during_quality"
);

let receipt_path =
    std::path::join(repo, "target/qualification/receipt.json").display;
let package_root =
    std::path::join(repo, "target/qualification/package-rehearsal").display;
let package_directory =
    package_qualified::build_release_package(
        repo,
        receipt_path,
        package_root
    );
let package_manifest =
    std::path::join(package_directory, "package-manifest.json").display;
let package_result =
    std::path::join(package_directory, "release-result.json").display;
let result = rhai::json::parse_file(package_result);
let report = #{
    schema_version: 1,
    mode: mode,
    version: version,
    tag: tag,
    source_commit: head,
    workflow_sha256: rhai::crypto::sha256_file(workflow_path),
    qualification_receipt: receipt_path,
    qualification_receipt_sha256:
        rhai::crypto::sha256_file(receipt_path),
    package_directory: package_directory,
    package_manifest_sha256:
        rhai::crypto::sha256_file(package_manifest),
    archive: result.archive,
    archive_sha256: result.archive_sha256,
    ready_to_publish: true
};
let report_path =
    std::path::join(repo, "target/qualification/release-rehearsal.json").display;
rhai::runtime::atomic_write(
    report_path,
    rhai::json::stringify_pretty(report) + "\n"
);
print("RELEASE REHEARSAL " + report_path);

if mode == "rehearse" {
    print("PASS: candidate is qualified and packaged; no tag or push occurred");
    return;
}

let tagged = git_mutation(
    repo,
    ["tag", "-a", tag, "-m", "AgenTerm " + version]
);
require(tagged.success, "release_tag_create_failed");
let pushed = git_mutation(
    repo,
    ["push", "--atomic", "origin", "main", tag]
);
if !pushed.success {
    git_mutation(repo, ["tag", "-d", tag]);
    throw "release_atomic_push_failed";
}
print("PUBLISHED " + tag + "; GitHub Actions owns Release creation");
