// Byte-qualified offline Windows package construction.

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

fn sha256(path) {
    require(
        std::fs::exists(path) && std::fs::metadata(path).is_file,
        "package_input_missing:" + path
    );
    rhai::crypto::sha256_file(path)
}

fn sorted(values) {
    let result = values;
    result.sort();
    result
}

fn exact_values(expected, actual) {
    let left = sorted(expected);
    let right = sorted(actual);
    if left.len != right.len {
        return false;
    }
    for index in 0..left.len {
        if left[index] != right[index] {
            return false;
        }
    }
    true
}

fn names(values) {
    let result = [];
    for value in values {
        result.push(value.name);
    }
    result
}

fn one_by_name(values, name, code) {
    let found = ();
    for value in values {
        if value.name == name {
            require(found == (), code + "_duplicate:" + name);
            found = value;
        }
    }
    require(found != (), code + "_missing:" + name);
    found
}

fn valid_version(value) {
    if value == "" {
        return false;
    }
    let suffix_offset = -1;
    for separator in ["+", "-"] {
        let offset = value.index_of(separator);
        if offset >= 0 && (suffix_offset < 0 || offset < suffix_offset) {
            suffix_offset = offset;
        }
    }
    let core =
        if suffix_offset < 0 {
            value
        } else {
            require(
                suffix_offset + 1 < value.len,
                "package_version_suffix_empty"
            );
            let suffix = value.sub_string(suffix_offset + 1);
            for character in suffix {
                require(
                    "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789.-"
                        .contains(character.to_string()),
                    "package_version_suffix_invalid"
                );
            }
            value.sub_string(0, suffix_offset)
        };
    let parts = core.split(".");
    if parts.len != 3 {
        return false;
    }
    for part in parts {
        if part == "" {
            return false;
        }
        for character in part {
            if !"0123456789".contains(character.to_string()) {
                return false;
            }
        }
    }
    true
}

fn checked_command_text(program, arguments, cwd, code) {
    let command = std::process::command(program);
    command.args(arguments);
    command.current_dir(cwd);
    command.timeout(std::time::Duration::from_secs(30));
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

fn create_zip(staging, archive, members) {
    let arguments = ["-a", "-c", "-f", archive, "-C", staging];
    arguments.append(members);
    checked_command_text(
        "tar.exe",
        arguments,
        staging,
        "package_zip_create"
    );
}

fn build_package_mode(
    repo_value,
    receipt_value,
    output_value,
    dry_run
) {
    let repo = std::path::absolute(repo_value).display;
    let receipt_path =
        std::path::join(repo, receipt_value).display;
    let output_root =
        std::path::join(repo, output_value).display;
    let receipt = rhai::json::parse_file(receipt_path);
    require(
        receipt.schema_version == 1
            && receipt.product == "AgenTerm"
            && receipt.release
            && receipt.stress_included,
        "package_receipt_not_release_stress"
    );
    require(
        !receipt.provenance.source_dirty,
        "package_receipt_source_dirty"
    );
    for gate in receipt.gates {
        require(
            gate.status == "passed",
            "package_receipt_gate_not_passed:" + gate.id
        );
    }

    let head = checked_command_text(
        "git",
        ["rev-parse", "HEAD"],
        repo,
        "package_git_head"
    ).to_lower();
    require(
        head.len == 40 && receipt.provenance.git_head == head,
        "package_receipt_head_mismatch"
    );

    let cargo_lock_path =
        std::path::join(repo, "Cargo.lock").display;
    let cargo_hash = sha256(cargo_lock_path);
    require(
        receipt.provenance.cargo_lock_sha256 == cargo_hash,
        "package_receipt_cargo_lock_hash"
    );
    let artifact_manifest_path =
        std::path::join(repo, "scripts/artifacts.json").display;
    let artifact_hash = sha256(artifact_manifest_path);
    require(
        receipt.provenance.artifact_manifest_sha256 == artifact_hash,
        "package_receipt_artifact_manifest_hash"
    );
    let sbom_path = std::path::join(
        repo,
        "dist/agenterm-sbom.spdx.json"
    ).display;
    let sbom_hash = sha256(sbom_path);
    require(
        receipt.provenance.sbom_sha256 == sbom_hash,
        "package_receipt_sbom_hash"
    );
    let gate_manifest_path = std::path::join(
        repo,
        "scripts/qualification-gates.json"
    ).display;
    let gate_hash = sha256(gate_manifest_path);
    require(
        receipt.gate_manifest.sha256 == gate_hash,
        "package_receipt_gate_manifest_hash"
    );
    let gate_manifest = rhai::json::parse_file(gate_manifest_path);
    require(
        exact_values(
            names(gate_manifest.required_gates),
            names(receipt.gates)
        ),
        "package_receipt_gate_set"
    );

    let artifact_spec =
        rhai::json::parse_file(artifact_manifest_path);
    let artifact_names = names(artifact_spec.executables);
    require(
        artifact_names.len > 0,
        "package_artifact_manifest_empty"
    );
    let unique = #{};
    for name in artifact_names {
        require(
            !unique.contains(name),
            "package_artifact_duplicate:" + name
        );
        unique[name] = true;
    }
    require(
        exact_values(
            artifact_names,
            names(receipt.provenance.executables)
        ),
        "package_receipt_artifact_set"
    );

    let metadata_path =
        std::path::join(repo, "dist/agenterm.json").display;
    let metadata = rhai::json::parse_file(metadata_path);
    require(
        metadata.schema_version == 2
            && metadata.git_commit == head
            && !metadata.git_dirty
            && metadata.cargo_lock_sha256 == cargo_hash
            && metadata.artifact_manifest_sha256 == artifact_hash,
        "package_metadata_source_mismatch"
    );
    require(
        exact_values(artifact_names, names(metadata.executables)),
        "package_metadata_artifact_set"
    );

    let payload = [];
    let sources = #{};
    for name in artifact_names {
        let source =
            std::path::join(
                std::path::join(repo, "dist").display,
                name
            ).display;
        let hash = sha256(source);
        let receipt_entry = one_by_name(
            receipt.provenance.executables,
            name,
            "package_receipt_artifact"
        );
        let metadata_entry = one_by_name(
            metadata.executables,
            name,
            "package_metadata_artifact"
        );
        require(
            receipt_entry.sha256 == hash
                && metadata_entry.sha256 == hash,
            "package_executable_hash:" + name
        );
        payload.push(#{ name: name, sha256: hash });
        sources[name] = source;
    }

    for item in [
        #{
            name: "agenterm.json",
            path: metadata_path
        },
        #{
            name: "artifacts.json",
            path: artifact_manifest_path
        },
        #{
            name: "qualification-receipt.json",
            path: receipt_path
        },
        #{
            name: "agenterm-sbom.spdx.json",
            path: sbom_path
        },
        #{
            name: "LICENSE-APACHE",
            path: std::path::join(repo, "LICENSE-APACHE").display
        },
        #{
            name: "LICENSE-MIT",
            path: std::path::join(repo, "LICENSE-MIT").display
        },
        #{
            name: "THIRD_PARTY_NOTICES.md",
            path: std::path::join(
                repo,
                "THIRD_PARTY_NOTICES.md"
            ).display
        }
    ] {
        payload.push(#{ name: item.name, sha256: sha256(item.path) });
        sources[item.name] = item.path;
    }

    let version = metadata.version;
    require(valid_version(version), "package_version_invalid:" + version);
    let run_id =
        std::time::SystemTime::now().unix_millis + "-" +
        std::process::id();
    let run_directory =
        std::path::join(output_root, run_id).display;
    let staging =
        std::path::join(run_directory, "staging").display;
    std::fs::create_dir_all(staging);
    let failure = "";

    try {
        let members = [];
        for entry in payload {
            let destination =
                std::path::join(staging, entry.name).display;
            std::fs::copy(sources[entry.name], destination);
            require(
                sha256(destination) == entry.sha256,
                "package_payload_changed:" + entry.name
            );
            members.push(entry.name);
        }
        let package_manifest = #{
            schema_version: 1,
            dry_run: dry_run,
            product: "AgenTerm",
            version: version,
            qualified_head: head,
            qualification_receipt_sha256: sha256(receipt_path),
            cargo_lock_sha256: cargo_hash,
            artifact_manifest_sha256: artifact_hash,
            sbom_sha256: sbom_hash,
            payload: payload
        };
        let staged_manifest = std::path::join(
            staging,
            "package-manifest.json"
        ).display;
        rhai::runtime::atomic_write(
            staged_manifest,
            rhai::json::stringify_pretty(package_manifest) + "\n"
        );
        members.push("package-manifest.json");

        let archive_name =
            "agenterm-" + version + "-windows-x86_64" +
            if dry_run { "-dry-run" } else { "" } +
            ".zip";
        let archive =
            std::path::join(run_directory, archive_name).display;
        create_zip(staging, archive, members);
        let external_manifest = std::path::join(
            run_directory,
            "package-manifest.json"
        ).display;
        std::fs::copy(staged_manifest, external_manifest);
        let result = #{
            schema_version: 1,
            dry_run: dry_run,
            archive: archive_name,
            archive_sha256: sha256(archive),
            package_manifest: "package-manifest.json",
            qualified_head: head
        };
        rhai::runtime::atomic_write(
            std::path::join(
                run_directory,
                if dry_run {
                    "dry-run-result.json"
                } else {
                    "release-result.json"
                }
            ).display,
            rhai::json::stringify_pretty(result) + "\n"
        );
    } catch (error) {
        failure = error.to_string();
    }
    if std::fs::exists(staging) {
        std::fs::remove_dir_all(staging);
    }
    if failure != "" {
        throw failure;
    }
    run_directory
}

fn build_package(repo_value, receipt_value, output_value) {
    build_package_mode(
        repo_value,
        receipt_value,
        output_value,
        true
    )
}

fn build_release_package(repo_value, receipt_value, output_value) {
    build_package_mode(
        repo_value,
        receipt_value,
        output_value,
        false
    )
}
