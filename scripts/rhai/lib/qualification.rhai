// Versioned qualification manifest, result, provenance, and receipt logic.

fn require(condition, code) {
    if !condition {
        throw code;
    }
}

fn sorted(values) {
    let result = [];
    for value in values {
        let inserted = false;
        for index in 0..result.len {
            if value < result[index] {
                result.insert(index, value);
                inserted = true;
                break;
            }
        }
        if !inserted {
            result.push(value);
        }
    }
    result
}

fn exact_values(left, right) {
    let first = sorted(left);
    let second = sorted(right);
    if first.len != second.len {
        return false;
    }
    for index in 0..first.len {
        if first[index] != second[index] {
            return false;
        }
    }
    true
}

fn valid_id(value, allow_period) {
    if value == "" || value.starts_with(".") || value.starts_with("-")
            || value.ends_with(".") || value.ends_with("-") {
        return false;
    }
    let previous_separator = false;
    for character in value {
        let text = character.to_string();
        let alphabet =
            if allow_period {
                "abcdefghijklmnopqrstuvwxyz0123456789.-"
            } else {
                "abcdefghijklmnopqrstuvwxyz0123456789-"
            };
        if !alphabet.contains(text)
                || (
                    (text == "." || text == "-")
                        && previous_separator
                ) {
            return false;
        }
        previous_separator = text == "." || text == "-";
    }
    true
}

fn sha256(path) {
    require(
        std::fs::exists(path) && std::fs::metadata(path).is_file,
        "qualification_input_missing:" + path
    );
    rhai::crypto::sha256_file(path)
}

fn read_manifest(path) {
    require(
        std::fs::exists(path) && std::fs::metadata(path).is_file,
        "qualification_manifest_missing:" + path
    );
    let manifest = rhai::json::parse_file(path);
    require(
        manifest.schema_version == 1
            && manifest.receipt_schema_version == 1,
        "qualification_manifest_schema"
    );
    require(
        manifest.required_gates.len > 0,
        "qualification_manifest_empty"
    );
    let gate_ids = #{};
    let evidence_ids = #{};
    for gate in manifest.required_gates {
        require(
            valid_id(gate.id, false) && !gate_ids.contains(gate.id),
            "qualification_gate_id:" + gate.id
        );
        gate_ids[gate.id] = true;
        for id in gate.evidence {
            require(
                valid_id(id, true) && !evidence_ids.contains(id),
                "qualification_evidence_id:" + id
            );
            evidence_ids[id] = true;
        }
    }
    manifest
}

fn new_context(manifest_path, release, stress_included) {
    let path = std::path::absolute(manifest_path).display;
    #{
        manifest: read_manifest(path),
        manifest_path: path,
        manifest_sha256: sha256(path),
        release: release,
        stress_included: stress_included,
        started_at_utc: std::time::SystemTime::now().rfc3339,
        results: #{}
    }
}

fn gate_by_id(manifest, id) {
    let found = ();
    for gate in manifest.required_gates {
        if gate.id == id {
            require(
                found == (),
                "qualification_gate_duplicate:" + id
            );
            found = gate;
        }
    }
    found
}

fn evidence_from_output(output) {
    let evidence = [];
    for item in output {
        let line = item.to_string();
        line.trim();
        if line.starts_with("EVIDENCE ") {
            let id = line.sub_string(9);
            if valid_id(id, true) {
                evidence.push(id);
            }
        }
    }
    evidence
}

fn add_result(context, gate_id, status, duration_ms, output) {
    require(
        status == "passed"
            || status == "failed"
            || status == "skipped",
        "qualification_result_status:" + status
    );
    require(
        !context.results.contains(gate_id),
        "qualification_result_duplicate:" + gate_id
    );
    require(
        gate_by_id(context.manifest, gate_id) != (),
        "qualification_result_undeclared:" + gate_id
    );
    context.results[gate_id] = #{
        id: gate_id,
        status: status,
        duration_ms: duration_ms,
        evidence: evidence_from_output(output)
    };
    context
}

fn assert_results(context) {
    for gate in context.manifest.required_gates {
        require(
            context.results.contains(gate.id),
            "qualification_gate_missing:" + gate.id
        );
        let result = context.results[gate.id];
        require(
            result.status == "passed",
            "qualification_gate_not_passed:" + gate.id + ":" +
                result.status
        );
        let unique = #{};
        for id in result.evidence {
            require(
                !unique.contains(id),
                "qualification_evidence_duplicate:" + gate.id
            );
            unique[id] = true;
        }
        require(
            exact_values(gate.evidence, result.evidence),
            "qualification_evidence_mismatch:" + gate.id
        );
    }
    require(
        context.stress_included,
        "qualification_stress_required"
    );
}

fn command_output(program, arguments, cwd, code) {
    let command = std::process::command(program);
    command.args(arguments);
    command.current_dir(cwd);
    command.timeout(std::time::Duration::from_secs(10));
    command.capture_limit(262144);
    let output = command.output();
    require(
        output.success,
        code + ":" + output.stdout_text() + output.stderr_text()
    );
    let text = output.stdout_text();
    text.trim();
    text
}

fn provenance(
    repo,
    metadata_path,
    artifact_manifest_path,
    staged_directory,
    require_clean
) {
    let root = std::path::absolute(repo).display;
    let head = command_output(
        "git",
        ["rev-parse", "HEAD"],
        root,
        "qualification_git_head"
    ).to_lower();
    require(
        head.len == 40,
        "qualification_git_head_invalid"
    );
    let metadata = rhai::json::parse_file(metadata_path);
    require(
        metadata.schema_version == 2,
        "qualification_metadata_schema"
    );
    require(
        metadata.git_commit == head,
        "qualification_metadata_commit"
    );
    require(
        !require_clean || !metadata.git_dirty,
        "qualification_metadata_dirty"
    );
    let cargo_hash = sha256(
        std::path::join(root, "Cargo.lock").display
    );
    require(
        metadata.cargo_lock_sha256 == cargo_hash,
        "qualification_cargo_lock_hash"
    );
    let artifact_hash = sha256(artifact_manifest_path);
    require(
        metadata.artifact_manifest_sha256 == artifact_hash,
        "qualification_artifact_manifest_hash"
    );
    let artifact_spec = rhai::json::parse_file(
        artifact_manifest_path
    );
    require(
        artifact_spec.executables.len > 0,
        "qualification_artifact_manifest_empty"
    );
    let names = #{};
    let artifacts = [];
    for executable in artifact_spec.executables {
        require(
            !names.contains(executable.name),
            "qualification_artifact_duplicate:" + executable.name
        );
        names[executable.name] = true;
        let path = std::path::join(
            staged_directory,
            executable.name
        ).display;
        let hash = sha256(path);
        let metadata_entry = ();
        for entry in metadata.executables {
            if entry.name == executable.name {
                require(
                    metadata_entry == (),
                    "qualification_metadata_artifact_duplicate:" +
                        executable.name
                );
                metadata_entry = entry;
            }
        }
        require(
            metadata_entry != ()
                && metadata_entry.sha256 == hash,
            "qualification_metadata_artifact_hash:" +
                executable.name
        );
        artifacts.push(#{
            name: executable.name,
            sha256: hash
        });
    }
    #{
        git_head: head,
        source_dirty: metadata.git_dirty,
        cargo_lock_sha256: cargo_hash,
        artifact_manifest_sha256: artifact_hash,
        sbom_sha256: sha256(
            std::path::join(
                staged_directory,
                "agenterm-sbom.spdx.json"
            ).display
        ),
        executables: artifacts
    }
}

fn write_receipt(context, repo, output_path) {
    assert_results(context);
    let root = std::path::absolute(repo).display;
    let receipt = #{
        schema_version: context.manifest.receipt_schema_version,
        product: "AgenTerm",
        completed_at_utc: std::time::SystemTime::now().rfc3339,
        started_at_utc: context.started_at_utc,
        release: context.release,
        stress_included: context.stress_included,
        gate_manifest: #{
            path: "scripts/qualification-gates.json",
            sha256: context.manifest_sha256
        },
        provenance: provenance(
            root,
            std::path::join(root, "dist/agenterm.json").display,
            std::path::join(
                root,
                "scripts/artifacts.json"
            ).display,
            std::path::join(root, "dist").display,
            true
        ),
        gates: []
    };
    for gate in context.manifest.required_gates {
        receipt.gates.push(context.results[gate.id]);
    }
    let path = std::path::absolute(output_path).display;
    std::fs::create_dir_all(
        std::path::parent(path).display
    );
    rhai::runtime::atomic_write(
        path,
        rhai::json::stringify_pretty(receipt) + "\n"
    );
    path
}
