// Fail-fast repository lint without a PowerShell dependency.
//
// Args:
//   REPO [WORKER] [all|static|rust|rhai] [--json] [--self-test]

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

fn joined_lines(values) {
    let text = "";
    for value in values {
        if text != "" {
            text += "\n";
        }
        text += value.to_string();
    }
    text
}

fn run(program, arguments, cwd, timeout_seconds, code) {
    let command = std::process::command(program);
    command.args(arguments);
    command.current_dir(cwd);
    command.timeout(std::time::Duration::from_secs(timeout_seconds));
    command.capture_limit(262144);
    let output = command.output();
    require(
        output.success,
        code + ":\n" + output.stdout_text() + output.stderr_text()
    );
    output.stdout_text()
}

fn tracked(repo, patterns) {
    let arguments = ["-C", repo, "ls-files", "--"];
    arguments.append(patterns);
    let output = run("git", arguments, repo, 10, "lint_git_ls_files");
    let paths = [];
    for source_line in output.split("\n") {
        let relative = source_line;
        relative.trim();
        if relative == "" {
            continue;
        }
        let path = std::path::join(repo, relative).display;
        if std::fs::exists(path) && std::fs::metadata(path).is_file {
            paths.push(path);
        }
    }
    paths
}

fn relative(repo, path) {
    let root = std::path::absolute(repo).display;
    let full = std::path::absolute(path).display;
    if full.to_lower().starts_with(root.to_lower() + "\\") {
        return full.sub_string(root.len + 1);
    }
    if full.to_lower().starts_with(root.to_lower() + "/") {
        return full.sub_string(root.len + 1);
    }
    full
}

fn assert_json(paths, repo) {
    let problems = [];
    for path in paths {
        try {
            rhai::json::parse_file(path);
        } catch (error) {
            problems.push(
                relative(repo, path) + ": " + error.to_string()
            );
        }
    }
    require(
        problems.len == 0,
        "JSON parse errors:\n" + joined_lines(problems)
    );
}

fn assert_text(paths, repo) {
    let problems = [];
    for path in paths {
        let bytes = std::fs::read(path);
        let text = "";
        try {
            text = bytes.to_text();
        } catch (error) {
            problems.push(relative(repo, path) + ": invalid UTF-8");
            continue;
        }
        let nul = rhai::bytes::from_array([0]).to_text();
        if text.contains(nul) {
            problems.push(relative(repo, path) + ": embedded NUL byte");
        }
        if text.starts_with("<<<<<<<")
                || text.starts_with("=======")
                || text.starts_with(">>>>>>>")
                || text.contains("\n<<<<<<<")
                || text.contains("\n=======")
                || text.contains("\n>>>>>>>") {
            problems.push(
                relative(repo, path) +
                    ": unresolved merge-conflict marker"
            );
        }
    }
    require(
        problems.len == 0,
        "Tracked text hygiene errors:\n" + joined_lines(problems)
    );
}

fn non_empty_file(path) {
    std::fs::exists(path)
        && std::fs::metadata(path).is_file
        && std::fs::metadata(path).len > 0
}

fn resolve_rh_cli(repo, worker) {
    if worker != "" {
        let parent = std::path::parent(worker).display;
        let adjacent_win = std::path::join(parent, "agenterm-rh.exe").display;
        if non_empty_file(adjacent_win) {
            return adjacent_win;
        }
        let adjacent = std::path::join(parent, "agenterm-rh").display;
        if non_empty_file(adjacent) {
            return adjacent;
        }
    }
    let dist_win = std::path::join(repo, "dist/agenterm-rh.exe").display;
    if non_empty_file(dist_win) {
        return dist_win;
    }
    let dist = std::path::join(repo, "dist/agenterm-rh").display;
    if non_empty_file(dist) {
        return dist;
    }
    let target_win = std::path::join(repo, "target/debug/agenterm-rh.exe").display;
    if non_empty_file(target_win) {
        return target_win;
    }
    let target = std::path::join(repo, "target/debug/agenterm-rh").display;
    if non_empty_file(target) {
        return target;
    }
    ""
}

fn check_many_program(repo, worker) {
    let rh = resolve_rh_cli(repo, worker);
    if rh != "" {
        return rh;
    }
    worker
}

fn assert_rhai(worker, paths, repo) {
    require(
        std::fs::exists(worker) && std::fs::metadata(worker).is_file,
        "lint_worker_missing:" + worker
    );
    require(paths.len > 0, "lint_rhai_files_empty");
    let files = [];
    for path in paths {
        files.push(relative(repo, path));
    }
    let manifest_path = std::path::join(
        rhai::runtime::temp_dir().display,
        "check-many.json"
    ).display;
    rhai::runtime::atomic_write(
        manifest_path,
        rhai::json::stringify_pretty(#{
            schema_version: 1,
            kind: "agenterm-rhai-check-manifest",
            files: files
        }) + "\n"
    );
    let program = check_many_program(repo, worker);
    let command = std::process::command(program);
    command.args([
        "check-many", "--manifest", manifest_path,
        "--profile", "local",
        "--project-root", repo,
        "--timeout-ms", "10000",
        "--max-operations", "1000000"
    ]);
    command.current_dir(repo);
    command.timeout(std::time::Duration::from_secs(60));
    command.capture_limit(262144);
    let output = command.output();
    require(
        output.success,
        "Rhai lint errors:\n" + output.stdout_text() + output.stderr_text()
    );
}

fn static_lint(repo) {
    assert_json(tracked(repo, ["*.json"]), repo);
    assert_text(
        tracked(
            repo,
            [
                "*.rs", "*.toml", "*.rhai", "*.json", "*.md", "*.html",
                "*.yml", "*.yaml", "*.sh", "*.bat", "*.cmd"
            ]
        ),
        repo
    );
}

fn rust_lint(repo) {
    run(
        "cargo",
        ["fmt", "--all", "--", "--check"],
        repo,
        60,
        "lint_rustfmt"
    );
    run(
        "cargo",
        [
            "clippy", "--locked", "--all-targets", "--all-features",
            "--", "-D", "warnings"
        ],
        repo,
        60,
        "lint_clippy"
    );
}

fn default_worker(repo) {
    let windows = std::path::join(
        repo,
        "target/debug/agenterm-rhai.exe"
    ).display;
    if std::fs::exists(windows) {
        return windows;
    }
    std::path::join(repo, "target/debug/agenterm-rhai").display
}

fn self_test(repo, worker) {
    let root = rhai::runtime::temp_dir().display;
    let bad_json = std::path::join(root, "bad.json").display;
    let bad_utf8 = std::path::join(root, "bad.md").display;
    let conflict = std::path::join(root, "conflict.md").display;
    let bad_rhai = std::path::join(root, "bad.rhai").display;
    std::fs::write(bad_json, "{\"missing\":");
    std::fs::write_bytes(
        bad_utf8,
        rhai::bytes::from_array([255, 254, 253])
    );
    std::fs::write(conflict, "<<<<<<< ours\n=======\n>>>>>>> theirs\n");
    std::fs::write(bad_rhai, "let broken = ;");

    let json_rejected = false;
    try {
        assert_json([bad_json], repo);
    } catch (error) {
        json_rejected = error.to_string().contains("JSON parse errors");
    }
    let text_rejected = false;
    try {
        assert_text([bad_utf8, conflict], repo);
    } catch (error) {
        text_rejected =
            error.to_string().contains("Tracked text hygiene errors");
    }
    let rhai_rejected = false;
    try {
        assert_rhai(worker, [bad_rhai], repo);
    } catch (error) {
        rhai_rejected = error.to_string().contains("Rhai lint errors");
    }
    require(
        json_rejected && text_rejected && rhai_rejected,
        "lint_selftest_did_not_reject_malformed_inputs"
    );
}

require(args.len >= 1, "expected: REPO [WORKER] [MODE] [OPTIONS]");
let repo = std::path::absolute(args[0]).display;
let worker = default_worker(repo);
let mode = "all";
let json = false;
let internal_selftest = false;
for index in 1..args.len {
    let value = args[index];
    if value == "--json" {
        json = true;
    } else if value == "--self-test" {
        internal_selftest = true;
    } else if ["all", "static", "rust", "rhai"].contains(value) {
        mode = value;
    } else if value == "--worker" {
        require(index + 1 < args.len, "lint_worker_value_missing");
        worker = std::path::absolute(args[index + 1]).display;
        index += 1;
    } else if std::fs::exists(value) {
        worker = std::path::absolute(value).display;
    } else {
        throw "lint_unknown_argument:" + value;
    }
}

let started = std::time::SystemTime::now().unix_millis;
let phases = [];
let failure = "";
try {
    if internal_selftest {
        let phase_started = std::time::SystemTime::now().unix_millis;
        self_test(repo, worker);
        phases.push(#{
            id: "self-test",
            passed: true,
            duration_ms:
                std::time::SystemTime::now().unix_millis - phase_started,
            message: ""
        });
    } else {
        if mode == "all" || mode == "static" {
            let phase_started = std::time::SystemTime::now().unix_millis;
            static_lint(repo);
            phases.push(#{
                id: "static",
                passed: true,
                duration_ms:
                    std::time::SystemTime::now().unix_millis - phase_started,
                message: ""
            });
        }
        if mode == "all" || mode == "rust" {
            let phase_started = std::time::SystemTime::now().unix_millis;
            rust_lint(repo);
            phases.push(#{
                id: "rust",
                passed: true,
                duration_ms:
                    std::time::SystemTime::now().unix_millis - phase_started,
                message: ""
            });
        }
        if mode == "all" || mode == "rhai" {
            let phase_started = std::time::SystemTime::now().unix_millis;
            assert_rhai(
                worker,
                tracked(repo, ["scripts/rhai/*.rhai"]),
                repo
            );
            phases.push(#{
                id: "rhai",
                passed: true,
                duration_ms:
                    std::time::SystemTime::now().unix_millis - phase_started,
                message: ""
            });
        }
    }
} catch (error) {
    failure = error.to_string();
}

let report = #{
    schema_version: 1,
    ok: failure == "",
    mode: if internal_selftest { "self-test" } else { mode },
    duration_ms: std::time::SystemTime::now().unix_millis - started,
    phases: phases
};
if json {
    print(rhai::json::stringify_pretty(report));
} else if failure == "" {
    print("PASS: repository lint (" + report.duration_ms + " ms)");
}
if failure != "" {
    throw failure;
}
