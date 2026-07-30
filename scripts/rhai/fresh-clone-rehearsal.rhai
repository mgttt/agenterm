// Prove the public release rehearsal from an owned, artifact-free clone while
// observing every discoverable descendant process.
//
// Args:
//   [REPO]
//   --self-test
//
// This task is intentionally not part of the qualification manifest: invoking
// it from release qualification would recurse. It is a final, clean-checkout
// delivery proof owned by the caller.

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

fn trimmed(value) {
    let text = value.to_string();
    text.trim();
    text
}

fn tail(value, maximum) {
    let text = value.to_string();
    if text.len <= maximum {
        return text;
    }
    text.sub_string(text.len - maximum, maximum)
}

fn checked_output(program, arguments, cwd, timeout_ms, code) {
    let command = std::process::command(program);
    command.args(arguments);
    command.current_dir(cwd);
    command.env("GCM_INTERACTIVE", "Never");
    command.env("GIT_TERMINAL_PROMPT", "0");
    command.timeout(std::time::Duration::from_millis(timeout_ms));
    command.capture_limit(262144);
    let output = command.output();
    require(
        output.success,
        code + ":exit=" + output.exit_code
    );
    trimmed(output.stdout_text())
}

fn git(repo, arguments, code) {
    let argv = ["-C", repo];
    argv.append(arguments);
    checked_output("git", argv, repo, 120000, code)
}

fn package_version(repo) {
    let cargo =
        std::fs::read_to_string(std::path::join(repo, "Cargo.toml").display);
    let in_package = false;
    for source_line in cargo.split("\n") {
        let line = source_line;
        line.trim();
        if line.starts_with("[") {
            in_package = line == "[package]";
        } else if in_package && line.starts_with("version") {
            let pieces = line.split("\"");
            require(pieces.len >= 2, "fresh_clone_version_parse");
            return pieces[1];
        }
    }
    throw "fresh_clone_version_missing";
}

fn process_key(process) {
    process.id.to_string() + ":" +
        process.parent_id.to_string() + ":" +
        process.executable_name.to_lower()
}

fn powershell_token(process) {
    process.executable_name.to_lower() + ":" + process.id
}

fn terminal_compatibility_executable() {
    "powershell.exe"
}

fn alternate_powershell_executable() {
    "pwsh.exe"
}

fn is_powershell(process) {
    let name = process.executable_name.to_lower();
    name == terminal_compatibility_executable()
        || name == alternate_powershell_executable()
}

fn observe_descendants(root_id, owned_ids, identities) {
    let snapshot = std::process::list();
    let included = #{};
    included[root_id.to_string()] = true;
    for id in owned_ids.keys() {
        included[id] = true;
    }
    for depth in 0..64 {
        let changed = false;
        for process in snapshot {
            let id = process.id.to_string();
            let parent = process.parent_id.to_string();
            if !included.contains(id) && included.contains(parent) {
                included[id] = true;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    let current = [];
    for process in snapshot {
        let id = process.id.to_string();
        if !included.contains(id) {
            continue;
        }
        let identity = process_key(process);
        if identities.contains(id) {
            require(
                identities[id] == identity,
                "fresh_clone_process_id_reused:" + id
            );
        } else {
            identities[id] = identity;
        }
        owned_ids[id] = true;
        current.push(#{
            id: process.id,
            parent_id: process.parent_id,
            executable_name: process.executable_name
        });
    }
    current
}

fn unique_names(processes) {
    let names = [];
    for process in processes {
        let name = process.executable_name.to_lower();
        if !names.contains(name) {
            names.push(name);
        }
    }
    names.sort();
    names
}

fn ordered_processes(processes) {
    let ordered = [];
    for key in processes.keys() {
        let process = processes[key];
        let inserted = false;
        for index in 0..ordered.len {
            if process.id < ordered[index].id {
                ordered.insert(index, process);
                inserted = true;
                break;
            }
        }
        if !inserted {
            ordered.push(process);
        }
    }
    ordered
}

fn joined(values) {
    let text = "";
    for value in values {
        if text != "" {
            text += ",";
        }
        text += value.to_string();
    }
    text
}

fn exact_terminal_payload(observed, typed) {
    let first = observed;
    let second = typed;
    first.sort();
    second.sort();
    require(
        first.len == 1 && second.len == 1,
        "fresh_clone_terminal_payload_not_unique:" +
            first.len + ":" + second.len
    );
    require(
        first[0] == second[0],
        "fresh_clone_untyped_powershell:" +
            joined(first) + ":" + joined(second)
    );
    #{
        policy: "exact_remote_ui_terminal_compatibility_payload",
        observed: first,
        typed: second
    }
}

fn remove_owned_clone(qualification_root, clone) {
    let owner = std::path::absolute(qualification_root).display;
    let target = std::path::absolute(clone).display;
    require(
        std::path::parent(target).display.to_lower() == owner.to_lower()
            && std::path::PathBuf::from(target).file_name.starts_with(
                "fresh-clone-owned-"
            ),
        "fresh_clone_cleanup_outside_owned_root:" + target
    );
    if std::fs::exists(target) {
        std::fs::remove_dir_all(target);
    }
    require(!std::fs::exists(target), "fresh_clone_cleanup_failed:" + target);
}

fn terminate_observed_processes(identities) {
    for attempt in 0..4 {
        let terminated = false;
        for process in std::process::list() {
            let id = process.id.to_string();
            if process.id != std::process::id()
                    && identities.contains(id)
                    && identities[id] == process_key(process) {
                try {
                    std::process::kill(process.id);
                    terminated = true;
                } catch (error) {
                    // A process may exit between the snapshot and kill.
                }
            }
        }
        if !terminated {
            break;
        }
        rhai::task::sleep(std::time::Duration::from_millis(100));
    }
}

fn live_observed_processes(identities) {
    let live = [];
    for process in std::process::list() {
        let id = process.id.to_string();
        if process.id != std::process::id()
                && identities.contains(id)
                && identities[id] == process_key(process) {
            live.push(#{
                id: process.id,
                parent_id: process.parent_id,
                executable_name: process.executable_name
            });
        }
    }
    live
}

fn rejection_contains(expected, action) {
    let rejected = false;
    try {
        action.call();
    } catch (error) {
        require(
            error.to_string().contains(expected),
            "fresh_clone_selftest_wrong_rejection:" + error
        );
        rejected = true;
    }
    require(rejected, "fresh_clone_selftest_expected_rejection:" + expected);
}

if args.len == 1 && args[0] == "--self-test" {
    let accepted = exact_terminal_payload(
        [terminal_compatibility_executable() + ":101"],
        [terminal_compatibility_executable() + ":101"]
    );
    require(accepted.observed.len == 1, "fresh_clone_selftest_accept");
    rejection_contains(
        "fresh_clone_untyped_powershell",
        || exact_terminal_payload(
            [terminal_compatibility_executable() + ":101"],
            [terminal_compatibility_executable() + ":202"]
        )
    );
    rejection_contains(
        "fresh_clone_terminal_payload_not_unique",
        || exact_terminal_payload([], [])
    );
    rejection_contains(
        "fresh_clone_terminal_payload_not_unique",
        || exact_terminal_payload(
            [
                terminal_compatibility_executable() + ":101",
                alternate_powershell_executable() + ":202"
            ],
            [terminal_compatibility_executable() + ":101"]
        )
    );
    print("PASS: fresh-clone process policy fails closed");
    return;
}

require(args.len <= 1, "expected: [REPO]");
require(
    !std::env::has("AGENTERM_FRESH_CLONE_REHEARSAL_ACTIVE"),
    "fresh_clone_recursive_invocation"
);
let child_deadline_ms = 3300000;

let source =
    std::path::absolute(
        if args.len == 1 {
            args[0]
        } else {
            std::env::current_dir().display
        }
    ).display;
require(
    git(source, ["branch", "--show-current"], "fresh_clone_source_branch")
        == "main",
    "fresh_clone_source_not_main"
);
require(
    git(
        source,
        ["status", "--porcelain", "--untracked-files=all"],
        "fresh_clone_source_status"
    ) == "",
    "fresh_clone_source_dirty"
);
let head = git(source, ["rev-parse", "HEAD"], "fresh_clone_source_head");
let origin =
    git(source, ["remote", "get-url", "origin"], "fresh_clone_origin");
let push_origin = git(
    source,
    ["remote", "get-url", "--push", "origin"],
    "fresh_clone_push_origin"
);
let version = package_version(source);
let tag = "v" + version;

let qualification_root =
    std::path::join(source, "target/qualification").display;
std::fs::create_dir_all(qualification_root);
let nonce =
    std::process::id() + "-" +
    std::time::SystemTime::now().unix_millis;
let clone =
    std::path::join(
        qualification_root,
        "fresh-clone-owned-" + nonce
    ).display;
let evidence_path =
    std::path::join(
        qualification_root,
        "fresh-clone-rehearsal.json"
    ).display;
let stdout_path =
    std::path::join(
        qualification_root,
        "fresh-clone-rehearsal.stdout"
    ).display;
let stderr_path =
    std::path::join(
        qualification_root,
        "fresh-clone-rehearsal.stderr"
    ).display;

let evidence = #{
    schema_version: 1,
    started_at_utc: std::time::SystemTime::now().rfc3339,
    completed_at_utc: (),
    source_commit: head,
    source_branch: "main",
    source_clean: true,
    delivery_sequence: [],
    origin_urls_redacted: true,
    origin_restored: false,
    origin_redacted_before_cleanup: false,
    initial_artifacts: #{
        target_absent: false,
        dist_absent: false
    },
    stage0: #{
        target_observed_at_ms: (),
        worker_observed_at_ms: (),
        cargo_observed: false,
        rustc_observed: false,
        script_worker_observed: false
    },
    process_observation: #{
        root_id: (),
        scan_interval_ms: 100,
        child_deadline_ms: child_deadline_ms,
        scan_count: 0,
        unique_processes: [],
        timeline: [],
        remaining_before_cleanup: [],
        remaining_after_cleanup: [],
        powershell_policy: ()
    },
    qualification: #{
        expected_gate_count: 34,
        gate_count: 0,
        all_passed: false,
        stress_included: false,
        receipt_source_commit: (),
        receipt_source_dirty: ()
    },
    packaging: #{
        ready_to_publish: false,
        mode: (),
        archive_name: (),
        archive_sha256: ()
    },
    mutation_proof: #{
        local_tags_unchanged: false,
        remote_refs_unchanged: false,
        local_candidate_tag_absent_before: false,
        local_candidate_tag_absent_after: false,
        candidate_tag_absent_before: false,
        candidate_tag_absent_after: false
    },
    clone_clean_after_rehearsal: false,
    clone_removed: false,
    success: false,
    failure: ()
};

let failure = ();
let child = ();
let owned_ids = #{};
let identities = #{};
let unique = #{};
try {
    checked_output(
        "git",
        [
            "clone", "--no-hardlinks", "--no-checkout", "--",
            source, clone
        ],
        source,
        300000,
        "fresh_clone_create"
    );
    git(
        clone,
        ["checkout", "-B", "main", head],
        "fresh_clone_checkout"
    );
    git(
        clone,
        ["remote", "set-url", "origin", origin],
        "fresh_clone_restore_origin"
    );
    git(
        clone,
        ["remote", "set-url", "--push", "origin", push_origin],
        "fresh_clone_restore_push_origin"
    );
    require(
        git(clone, ["remote", "get-url", "origin"], "fresh_clone_origin_check")
            == origin
            && git(
                clone,
                ["remote", "get-url", "--push", "origin"],
                "fresh_clone_push_origin_check"
            ) == push_origin,
        "fresh_clone_origin_not_restored"
    );
    evidence.origin_restored = true;
    require(
        git(clone, ["rev-parse", "HEAD"], "fresh_clone_head") == head
            && git(
                clone,
                ["branch", "--show-current"],
                "fresh_clone_branch"
            ) == "main"
            && git(
                clone,
                ["status", "--porcelain", "--untracked-files=all"],
                "fresh_clone_status"
            ) == "",
        "fresh_clone_not_exact_clean_main"
    );

    let clone_target = std::path::join(clone, "target").display;
    let clone_dist = std::path::join(clone, "dist").display;
    evidence.initial_artifacts.target_absent =
        !std::fs::exists(clone_target);
    evidence.initial_artifacts.dist_absent =
        !std::fs::exists(clone_dist);
    require(
        evidence.initial_artifacts.target_absent
            && evidence.initial_artifacts.dist_absent,
        "fresh_clone_preexisting_artifacts"
    );
    evidence.delivery_sequence.push("fresh_clone_artifact_free");

    let local_tags_before =
        git(clone, ["tag", "--list"], "fresh_clone_local_tags_before");
    evidence.mutation_proof.local_candidate_tag_absent_before =
        git(
            clone,
            ["tag", "--list", tag],
            "fresh_clone_local_candidate_before"
        ) == "";
    let remote_before = git(
        clone,
        [
            "ls-remote", "--refs", "origin",
            "refs/heads/main", "refs/tags/" + tag
        ],
        "fresh_clone_remote_before"
    );
    evidence.mutation_proof.candidate_tag_absent_before =
        !remote_before.contains("refs/tags/" + tag);

    let release = std::process::command("cmd.exe");
    release.args(["/d", "/c", "release.cmd", "--rehearse"]);
    release.current_dir(clone);
    release.env("AGENTERM_NO_ACTIVATE", "1");
    release.env("AGENTERM_FRESH_CLONE_REHEARSAL_ACTIVE", "1");
    release.env("GCM_INTERACTIVE", "Never");
    release.env("GIT_TERMINAL_PROMPT", "0");
    release.stdout_file(stdout_path);
    release.stderr_file(stderr_path);
    child = release.start();
    evidence.process_observation.root_id = child.id;

    let powershell = [];
    let started = std::time::SystemTime::now().unix_millis;
    let next_timeline = started;
    while child.state == "running"
            && std::time::SystemTime::now().unix_millis - started
                < child_deadline_ms {
        let now = std::time::SystemTime::now().unix_millis;
        let current =
            observe_descendants(child.id, owned_ids, identities);
        evidence.process_observation.scan_count += 1;
        for process in current {
            let identity = process_key(process);
            if !unique.contains(identity) {
                unique[identity] = process;
            }
            if is_powershell(process) {
                let token = powershell_token(process);
                if !powershell.contains(token) {
                    powershell.push(token);
                }
            }
        }
        if evidence.stage0.target_observed_at_ms == ()
                && std::fs::exists(clone_target) {
            evidence.stage0.target_observed_at_ms = now - started;
        }
        let bootstrap_worker =
            std::path::join(
                clone_target,
                "debug/agenterm-script.exe"
            ).display;
        if evidence.stage0.worker_observed_at_ms == ()
                && std::fs::exists(bootstrap_worker) {
            evidence.stage0.worker_observed_at_ms = now - started;
        }
        let names = unique_names(current);
        if names.contains("cargo.exe") {
            evidence.stage0.cargo_observed = true;
        }
        if names.contains("rustc.exe") {
            evidence.stage0.rustc_observed = true;
        }
        if names.contains("agenterm-script.exe") {
            evidence.stage0.script_worker_observed = true;
        }
        if now >= next_timeline {
            evidence.process_observation.timeline.push(#{
                elapsed_ms: now - started,
                descendant_count: current.len,
                executable_names: names
            });
            next_timeline = now + 1000;
        }
        rhai::task::sleep(std::time::Duration::from_millis(100));
    }
    if child.state == "running" {
        child.kill();
        throw "fresh_clone_release_deadline";
    }
    let result =
        child.wait_with_output(std::time::Duration::from_secs(10));
    require(
        result.success,
        "fresh_clone_release_failed:" +
            if std::fs::exists(stderr_path) {
                tail(std::fs::read_to_string(stderr_path), 16384)
            } else {
                ""
            }
    );

    require(
        evidence.process_observation.scan_count > 0
            && evidence.process_observation.unique_processes.len > 2
            && evidence.stage0.target_observed_at_ms != ()
            && evidence.stage0.worker_observed_at_ms != ()
            && evidence.stage0.cargo_observed
            && evidence.stage0.rustc_observed
            && evidence.stage0.script_worker_observed,
        "fresh_clone_stage0_not_observed"
    );
    evidence.delivery_sequence.push("stage0_worker");

    let receipt_path =
        std::path::join(clone_target, "qualification/receipt.json").display;
    let release_path =
        std::path::join(
            clone_target,
            "qualification/release-rehearsal.json"
        ).display;
    require(
        std::fs::exists(receipt_path) && std::fs::exists(release_path),
        "fresh_clone_delivery_evidence_missing"
    );
    let receipt = rhai::json::parse_file(receipt_path);
    let release_report = rhai::json::parse_file(release_path);
    evidence.qualification.gate_count = receipt.gates.len;
    evidence.qualification.stress_included = receipt.stress_included;
    evidence.qualification.receipt_source_commit =
        receipt.provenance.git_head;
    evidence.qualification.receipt_source_dirty =
        receipt.provenance.source_dirty;
    evidence.qualification.all_passed = true;
    for gate in receipt.gates {
        if gate.status != "passed" {
            evidence.qualification.all_passed = false;
        }
    }
    require(
        receipt.gates.len == 34
            && evidence.qualification.all_passed
            && receipt.release
            && receipt.stress_included
            && receipt.provenance.git_head == head
            && !receipt.provenance.source_dirty,
        "fresh_clone_qualification_invalid"
    );
    evidence.delivery_sequence.push("qualification_34_of_34");
    require(
        receipt.process_observation.automation_processes.len == 0,
        "fresh_clone_inner_automation_powershell"
    );
    evidence.process_observation.powershell_policy =
        exact_terminal_payload(
            powershell,
            receipt.process_observation.terminal_compatibility_payloads
        );

    require(
        release_report.mode == "rehearse"
            && release_report.source_commit == head
            && release_report.ready_to_publish,
        "fresh_clone_release_report_invalid"
    );
    require(
        std::fs::exists(release_report.archive),
        "fresh_clone_package_archive_missing"
    );
    evidence.packaging.ready_to_publish = true;
    evidence.packaging.mode = release_report.mode;
    evidence.packaging.archive_name =
        std::path::PathBuf::from(release_report.archive).file_name;
    evidence.packaging.archive_sha256 =
        rhai::crypto::sha256_file(release_report.archive);
    require(
        evidence.packaging.archive_sha256 ==
            release_report.archive_sha256,
        "fresh_clone_package_hash_mismatch"
    );
    evidence.delivery_sequence.push("package_rehearsal");

    let local_tags_after =
        git(clone, ["tag", "--list"], "fresh_clone_local_tags_after");
    evidence.mutation_proof.local_candidate_tag_absent_after =
        git(
            clone,
            ["tag", "--list", tag],
            "fresh_clone_local_candidate_after"
        ) == "";
    let remote_after = git(
        clone,
        [
            "ls-remote", "--refs", "origin",
            "refs/heads/main", "refs/tags/" + tag
        ],
        "fresh_clone_remote_after"
    );
    evidence.mutation_proof.local_tags_unchanged =
        local_tags_before == local_tags_after;
    evidence.mutation_proof.remote_refs_unchanged =
        remote_before == remote_after;
    evidence.mutation_proof.candidate_tag_absent_after =
        !remote_after.contains("refs/tags/" + tag);
    require(
        evidence.mutation_proof.local_tags_unchanged
            && evidence.mutation_proof.remote_refs_unchanged
            && evidence.mutation_proof.local_candidate_tag_absent_before
            && evidence.mutation_proof.local_candidate_tag_absent_after
            && evidence.mutation_proof.candidate_tag_absent_before
            && evidence.mutation_proof.candidate_tag_absent_after,
        "fresh_clone_mutation_detected"
    );
    evidence.clone_clean_after_rehearsal =
        git(
            clone,
            ["status", "--porcelain", "--untracked-files=all"],
            "fresh_clone_final_status"
        ) == "";
    require(
        evidence.clone_clean_after_rehearsal,
        "fresh_clone_worktree_mutated"
    );
    evidence.success = true;
} catch (error) {
    failure = error.to_string();
    evidence.failure = failure;
}

evidence.process_observation.remaining_before_cleanup =
    live_observed_processes(identities);
if failure == ()
        && evidence.process_observation.remaining_before_cleanup.len > 0 {
    failure = "fresh_clone_owned_processes_remained";
    evidence.failure = failure;
    evidence.success = false;
}

try {
    if child != () && child.state == "running" {
        let final_processes =
            observe_descendants(child.id, owned_ids, identities);
        evidence.process_observation.scan_count += 1;
        for process in final_processes {
            let identity = process_key(process);
            if !unique.contains(identity) {
                unique[identity] = process;
            }
        }
        child.kill();
        child.wait_with_output(std::time::Duration::from_secs(5));
    }
    terminate_observed_processes(identities);
    evidence.process_observation.remaining_after_cleanup =
        live_observed_processes(identities);
    require(
        evidence.process_observation.remaining_after_cleanup.len == 0,
        "fresh_clone_owned_process_cleanup_failed"
    );
    if std::fs::exists(std::path::join(clone, ".git").display) {
        try {
            git(
                clone,
                [
                    "remote", "set-url", "origin",
                    "redacted://fresh-clone-removed"
                ],
                "fresh_clone_redact_origin"
            );
            git(
                clone,
                [
                    "remote", "set-url", "--push", "origin",
                    "redacted://fresh-clone-removed"
                ],
                "fresh_clone_redact_push_origin"
            );
            evidence.origin_redacted_before_cleanup = true;
        } catch (redaction_error) {
            // Removal below is still authoritative. A retained clone must
            // never be accepted when its real origin could remain in config.
        }
    }
    remove_owned_clone(qualification_root, clone);
    evidence.clone_removed = true;
    require(
        evidence.origin_redacted_before_cleanup
            || !std::fs::exists(clone),
        "fresh_clone_origin_retained"
    );
} catch (cleanup_error) {
    evidence.clone_removed = false;
    if failure == () {
        failure = cleanup_error.to_string();
        evidence.failure = failure;
    } else {
        evidence.failure += "\n" + cleanup_error;
    }
    evidence.success = false;
}

evidence.process_observation.unique_processes =
    ordered_processes(unique);
evidence.completed_at_utc = std::time::SystemTime::now().rfc3339;
rhai::runtime::atomic_write(
    evidence_path,
    rhai::json::stringify_pretty(evidence) + "\n"
);
if failure != () {
    throw failure;
}
print("PASS: fresh clone qualified, packaged, observed, and removed");
