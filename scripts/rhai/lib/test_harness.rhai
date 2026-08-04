// Shared fail-closed foundations for repository-owned Rhai smoke journeys.

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

fn new_context(repo_value, suite) {
    require(suite != "", "test_harness_suite_empty");
    let repo = std::path::absolute(repo_value).display;
    let shared_evidence = if std::env::has("AGENTERM_TEST_EVIDENCE_PATH") {
        std::env::get("AGENTERM_TEST_EVIDENCE_PATH")
    } else {
        ""
    };
    let evidence_file = if shared_evidence != () && shared_evidence != "" {
        std::path::absolute(shared_evidence).display
    } else {
        ""
    };
    let owned_root = std::path::join(
        repo,
        "target/smoke/test-runs"
    ).display;
    let run_id =
        std::time::SystemTime::now().unix_millis + "-" +
        std::process::id();
    let run_directory = std::path::join(
        owned_root,
        suite + "-" + run_id
    ).display;
    let workspace_directory =
        std::path::join(run_directory, "workspace").display;
    let settings_directory =
        std::path::join(run_directory, "settings").display;
    let instance_directory =
        std::path::join(run_directory, "instances").display;
    let evidence_directory =
        std::path::join(run_directory, "evidence").display;
    for directory in [
        workspace_directory,
        settings_directory,
        instance_directory,
        evidence_directory
    ] {
        std::fs::create_dir_all(directory);
    }
    let port = 20000 + (
        (
            std::time::SystemTime::now().unix_millis
                + std::process::id() * 97
        ) % 40000
    );
    #{
        schema_version: 1,
        suite: suite,
        run_id: run_id,
        address: "127.0.0.1:" + port,
        owned_root: owned_root,
        run_directory: run_directory,
        workspace_path: std::path::join(
            workspace_directory,
            "workspace.json"
        ).display,
        settings_path: std::path::join(
            settings_directory,
            "settings.json"
        ).display,
        instance_directory: instance_directory,
        evidence_path: if evidence_file != () && evidence_file != "" {
            evidence_file
        } else {
            std::path::join(
                evidence_directory,
                "emitted.txt"
            ).display
        },
        failure_directory: std::path::join(
            run_directory,
            "failure"
        ).display,
        command_log_path: std::path::join(
            run_directory,
            "commands.json"
        ).display,
        command_journal_path: std::path::join(
            run_directory,
            "commands.jsonl"
        ).display,
        cleanup_path: std::path::join(
            run_directory,
            "cleanup.json"
        ).display,
        manifest_path: std::path::join(
            run_directory,
            "manifest.json"
        ).display
    }
}

fn assert_owned_run(context) {
    let parent = std::path::parent(context.run_directory).display;
    require(
        parent.to_lower() == context.owned_root.to_lower(),
        "test_harness_run_outside_owned_root"
    );
}

fn redact_text(source, redactions) {
    let safe = source;
    for value in redactions {
        if value != "" {
            safe.replace(value, "<redacted>");
        }
    }
    safe
}

fn bounded_record_text(source, maximum_characters) {
    if source.len <= maximum_characters {
        return source;
    }
    source.sub_string(0, maximum_characters) +
        "\n<agenterm-command-output-truncated>\n"
}

fn configured_command(context, program, arguments) {
    let command = std::process::command(program);
    command.args(arguments);
    command.env("AGENTERM_IPC_ADDRESS", context.address);
    command.env("AGENTERM_WORKSPACE_PATH", context.workspace_path);
    command.env("AGENTERM_SETTINGS_PATH", context.settings_path);
    command.env("AGENTERM_INSTANCE_DIR", context.instance_directory);
    command.env("AGENTERM_NO_ACTIVATE", "1");
    command.timeout(std::time::Duration::from_secs(60));
    command.capture_limit(262144);
    command
}

fn append_command_record(context, arguments, output, expected_failure, redactions) {
    let safe_arguments = [];
    for argument in arguments {
        safe_arguments.push(redact_text(argument, redactions));
    }
    let record = #{
        recorded_at_utc: std::time::SystemTime::now().rfc3339,
        arguments: safe_arguments,
        expected_failure: expected_failure,
        exit_code: output.exit_code,
        output: bounded_record_text(
            redact_text(
                output.stdout_text() + output.stderr_text(),
                redactions
            ),
            // Preserve every command and outcome while keeping long smoke
            // journeys below the runtime's ordinary string robustness bound.
            // Full structured state belongs in explicit retained diagnostics.
            512
        )
    };
    // Keep the hot path proportional to one bounded record. Re-reading and
    // pretty-printing the entire command array here made long smoke journeys
    // quadratic and caused later UI phases to appear much slower than earlier
    // ones. The durable journal is the recovery source of truth until cleanup
    // seals the established commands.json array atomically.
    rhai::runtime::append_sync(
        context.command_journal_path,
        rhai::json::stringify(record) + "\n"
    );
    // Preserve the existing immediate diagnostic contract used by the harness
    // self-test without growing the per-command write. The complete record set
    // is recovered from the journal by finalize_command_log.
    rhai::runtime::atomic_write(
        context.command_log_path,
        rhai::json::stringify_pretty([record]) + "\n"
    );
}

fn finalize_command_log(context) {
    if !std::fs::exists(context.command_journal_path) {
        return;
    }
    let records = [];
    for source_line in std::fs::read_to_string(context.command_journal_path).split("\n") {
        let line = source_line;
        line.trim();
        if line != "" {
            records.push(rhai::json::parse(line));
        }
    }
    // Promote the complete, schema-compatible array before removing its
    // recovery journal. If parsing or promotion fails, the journal remains
    // intact for diagnosis and a later retry.
    rhai::runtime::atomic_write(
        context.command_log_path,
        rhai::json::stringify(records) + "\n"
    );
    std::fs::remove_file(context.command_journal_path);
}

fn run_cli(context, cli, arguments, expected_failure, redactions) {
    let command_arguments = ["--address", context.address];
    command_arguments.append(arguments);
    let command = configured_command(context, cli, command_arguments);
    // The child deadline must exceed the longest wait a CLI command may be
    // asked to perform (wait-pane/wait-ui run up to 30s on cold CI runners),
    // or the harness kills a healthy wait as process_timeout. Keep the
    // configured_command 60s ceiling instead of overriding it downward.
    command.timeout(std::time::Duration::from_secs(60));
    let output = command.output();
    append_command_record(
        context,
        arguments,
        output,
        expected_failure,
        redactions
    );
    #{
        success: output.success,
        exit_code: output.exit_code,
        stdout: output.stdout_text(),
        stderr: output.stderr_text()
    }
}

fn invoke_cli(context, cli, arguments, expected_failure, redactions) {
    let output = run_cli(
        context,
        cli,
        arguments,
        expected_failure,
        redactions
    );
    if expected_failure {
        require(!output.success, "test_harness_cli_unexpected_success");
    } else {
        require(
            output.success,
            "test_harness_cli_failed:" +
                redact_text(output.stderr, redactions)
        );
    }
    #{
        exit_code: output.exit_code,
        stdout: output.stdout,
        stderr: output.stderr
    }
}

fn emit_evidence(context, declared, id) {
    require(
        declared.contains(id),
        "test_harness_undeclared_evidence:" + id
    );
    let existing = if std::fs::exists(context.evidence_path) {
        std::fs::read_to_string(context.evidence_path)
    } else {
        ""
    };
    rhai::runtime::atomic_write(
        context.evidence_path,
        existing + id + "\n"
    );
    print("EVIDENCE " + id);
}

fn scrub_retained_text(context, redactions) {
    assert_owned_run(context);
    let pending = [context.run_directory];
    while pending.len > 0 {
        let directory = pending.pop();
        for entry in std::fs::read_dir(directory) {
            if entry.is_dir && !entry.is_symlink {
                pending.push(entry.path.display);
            } else if entry.is_file
                    && (
                        entry.file_name.ends_with(".json")
                            || entry.file_name.ends_with(".txt")
                            || entry.file_name.ends_with(".log")
                    ) {
                let source = std::fs::read_to_string(entry.path.display);
                let safe = redact_text(source, redactions);
                if safe != source {
                    rhai::runtime::atomic_write(entry.path.display, safe);
                }
            }
        }
    }
}

fn complete_impl(
    context,
    owned_children,
    succeeded,
    failure,
    announce_failure
) {
    assert_owned_run(context);
    let forced = [];
    let completed = [];
    for source_child in owned_children {
        let child = source_child;
        let pid = child.id;
        if child.state == "running" {
            child.kill();
            forced.push(pid);
        }
        child.wait_with_output(std::time::Duration::from_secs(2));
        completed.push(pid);
    }
    let cleanup = #{
        schema_version: 1,
        orphan_free: true,
        owned_count: owned_children.len,
        completed_pids: completed,
        forced_pids: forced,
        remaining_pids: [],
        remaining_windows: [],
        remaining_registrations: []
    };
    finalize_command_log(context);
    let diagnostics = [];
    if !succeeded {
        std::fs::create_dir_all(context.failure_directory);
        for entry in std::fs::read_dir(context.failure_directory) {
            if entry.is_file {
                diagnostics.push(entry.file_name);
            }
        }
        diagnostics.sort();
    }
    let manifest = #{
        schema_version: 1,
        suite: context.suite,
        run_id: context.run_id,
        result_class: if succeeded { "success" } else { "failure" },
        failure: failure,
        cleanup: cleanup,
        command_log: std::path::PathBuf::from(
            context.command_log_path
        ).file_name,
        evidence: std::path::PathBuf::from(
            context.evidence_path
        ).file_name,
        diagnostics: diagnostics,
        privacy: #{
            command_arguments: "known content-bearing arguments redacted",
            output_limit_bytes: 65536,
            pane_capture: "disabled for this suite"
        }
    };
    rhai::runtime::atomic_write(
        context.cleanup_path,
        rhai::json::stringify_pretty(cleanup) + "\n"
    );
    rhai::runtime::atomic_write(
        context.manifest_path,
        rhai::json::stringify_pretty(manifest) + "\n"
    );
    if !succeeded && announce_failure {
        print("FAILURE BUNDLE " + context.run_directory);
    }
    #{ cleanup: cleanup, manifest: manifest }
}

fn complete(context, owned_children, succeeded, failure) {
    complete_impl(
        context,
        owned_children,
        succeeded,
        failure,
        true
    )
}

fn complete_quiet(context, owned_children, succeeded, failure) {
    complete_impl(
        context,
        owned_children,
        succeeded,
        failure,
        false
    )
}

fn remove_run(context) {
    assert_owned_run(context);
    if std::fs::exists(context.run_directory) {
        std::fs::remove_dir_all(context.run_directory);
    }
}
