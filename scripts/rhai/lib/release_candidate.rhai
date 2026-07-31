// Exact-SHA release Candidate schema v1.
//
// A Candidate is a closed, flat payload directory plus one manifest.  This
// module never builds, tags, publishes, downloads, or trusts a filename glob.

fn require(condition, code) {
    if !condition {
        throw code;
    }
}

fn file_hash(path, code) {
    require(
        std::fs::exists(path) && std::fs::metadata(path).is_file,
        code + ":" + path
    );
    rhai::crypto::sha256_file(path)
}

fn file_size(path, code) {
    require(
        std::fs::exists(path) && std::fs::metadata(path).is_file,
        code + ":" + path
    );
    std::fs::metadata(path).len
}

fn payload_path(root, name) {
    require(
        name != ""
            && std::path::PathBuf::from(name).file_name == name
            && !name.contains("/")
            && !name.contains("\\"),
        "candidate_asset_name:" + name
    );
    std::path::join(root, name).display
}

fn valid_sha256(value) {
    if value.len != 64 {
        return false;
    }
    for character in value {
        if !"0123456789abcdef".contains(character.to_string()) {
            return false;
        }
    }
    true
}

fn valid_source_sha(value) {
    if value.len != 40 {
        return false;
    }
    for character in value {
        if !"0123456789abcdef".contains(character.to_string()) {
            return false;
        }
    }
    true
}

fn valid_version(value) {
    if value == "" || value.contains("/") || value.contains("\\") {
        return false;
    }
    let build_parts = value.split("+");
    if build_parts.len > 2 {
        return false;
    }
    if build_parts.len == 2 {
        if build_parts[1] == "" {
            return false;
        }
        for character in build_parts[1] {
            if !"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz.-"
                    .contains(character.to_string()) {
                return false;
            }
        }
    }
    let public_version = build_parts[0];
    let prerelease_parts = public_version.split("-");
    if prerelease_parts.len > 2 {
        return false;
    }
    if prerelease_parts.len == 2 {
        if prerelease_parts[1] == "" {
            return false;
        }
        for character in prerelease_parts[1] {
            if !"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz.-"
                    .contains(character.to_string()) {
                return false;
            }
        }
    }
    let core = prerelease_parts[0];
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

fn unique_names(values, code) {
    let names = [];
    let seen = #{};
    for value in values {
        require(
            value.name != "" && !seen.contains(value.name),
            code + ":" + value.name
        );
        seen[value.name] = true;
        names.push(value.name);
    }
    names
}

fn file_names(root) {
    require(
        std::fs::exists(root) && std::fs::metadata(root).is_dir,
        "candidate_asset_root:" + root
    );
    let names = [];
    for entry in std::fs::read_dir(root) {
        require(
            entry.is_file,
            "candidate_asset_not_file:" + entry.file_name
        );
        names.push(entry.file_name);
    }
    names
}

fn platform_specs(version) {
    [
        #{
            os: "windows",
            arch: "x86_64",
            release_name:
                "agenterm-" + version + "-windows-x86_64.zip",
            preview_name: ""
        },
        #{
            os: "windows",
            arch: "aarch64",
            release_name:
                "agenterm-" + version + "-windows-aarch64.zip",
            preview_name: ""
        },
        #{
            os: "linux",
            arch: "x86_64",
            release_name:
                "agenterm-" + version + "-linux-x86_64.tar.gz",
            preview_name: ""
        },
        #{
            os: "linux",
            arch: "aarch64",
            release_name:
                "agenterm-" + version + "-linux-aarch64.tar.gz",
            preview_name: ""
        },
        #{
            os: "macos",
            arch: "aarch64",
            release_name:
                "agenterm-" + version + "-macos-aarch64.zip",
            preview_name:
                "agenterm-" + version +
                    "-macos-aarch64-unsigned-preview.zip"
        },
        #{
            os: "macos",
            arch: "x86_64",
            release_name:
                "agenterm-" + version + "-macos-x86_64.zip",
            preview_name:
                "agenterm-" + version +
                    "-macos-x86_64-unsigned-preview.zip"
        }
    ]
}

fn one_platform_asset(manifest, os_name, arch) {
    let found = ();
    for asset in manifest.assets {
        if asset.os == os_name && asset.arch == arch {
            require(
                found == (),
                "candidate_platform_duplicate:" + os_name + "-" + arch
            );
            found = asset;
        }
    }
    require(
        found != (),
        "candidate_platform_missing:" + os_name + "-" + arch
    );
    found
}

fn expected_archive_name(spec, asset_root) {
    let release_exists =
        std::fs::exists(payload_path(asset_root, spec.release_name));
    let preview_exists =
        spec.preview_name != ""
            && std::fs::exists(payload_path(asset_root, spec.preview_name));
    require(
        release_exists != preview_exists,
        "candidate_platform_archive_count:" + spec.os + "-" + spec.arch
    );
    if release_exists { spec.release_name } else { spec.preview_name }
}

fn verify_checksum(path, hash, name) {
    let text = std::fs::read_to_string(path);
    text.trim();
    require(
        text == hash + "  " + name,
        "candidate_checksum_content:" + name
    );
}

fn validate_receipt(
    receipt,
    source_sha,
    cargo_hash,
    artifact_hash,
    gate_hash,
    sbom_hash,
    gate_spec
) {
    require(
        receipt.schema_version == 1
            && receipt.product == "AgenTerm"
            && receipt.release
            && receipt.stress_included,
        "candidate_receipt_profile"
    );
    require(
        receipt.provenance.git_head == source_sha
            && !receipt.provenance.source_dirty,
        "candidate_receipt_source"
    );
    require(
        receipt.provenance.cargo_lock_sha256 == cargo_hash,
        "candidate_receipt_cargo_lock"
    );
    require(
        receipt.provenance.artifact_manifest_sha256 == artifact_hash,
        "candidate_receipt_artifact_manifest"
    );
    require(
        receipt.gate_manifest.sha256 == gate_hash,
        "candidate_receipt_gate_manifest"
    );
    require(
        receipt.provenance.sbom_sha256 == sbom_hash,
        "candidate_receipt_sbom"
    );
    let required = [];
    for gate in gate_spec.required_gates {
        required.push(gate.id);
    }
    let actual = [];
    let seen = #{};
    for gate in receipt.gates {
        require(
            gate.id != "" && !seen.contains(gate.id),
            "candidate_receipt_gate_duplicate:" + gate.id
        );
        seen[gate.id] = true;
        require(
            gate.status == "passed",
            "candidate_receipt_gate_status:" + gate.id
        );
        let expected_gate = ();
        for declared in gate_spec.required_gates {
            if declared.id == gate.id {
                expected_gate = declared;
            }
        }
        require(
            expected_gate != (),
            "candidate_receipt_gate_unknown:" + gate.id
        );
        let evidence_seen = #{};
        for evidence in gate.evidence {
            require(
                !evidence_seen.contains(evidence),
                "candidate_receipt_evidence_duplicate:" + gate.id
            );
            evidence_seen[evidence] = true;
        }
        require(
            exact_values(expected_gate.evidence, gate.evidence),
            "candidate_receipt_evidence_set:" + gate.id
        );
        actual.push(gate.id);
    }
    require(
        exact_values(required, actual),
        "candidate_receipt_gate_set"
    );
}

fn validate_provenance(
    provenance,
    name,
    hash,
    source_sha,
    version,
    expected_tag,
    os_name,
    arch,
    cargo_hash,
    artifact_hash
) {
    require(
        provenance.schema_version == 1
            && provenance.product == "AgenTerm"
            && provenance.version == version
            && provenance.artifact == name
            && provenance.sha256 == hash,
        "candidate_provenance_artifact:" + name
    );
    require(
        provenance.source_commit == source_sha,
        "candidate_provenance_source_commit:" + name
    );
    require(
        provenance.source_tag == expected_tag,
        "candidate_provenance_source_tag:" + name
    );
    require(
        provenance.cargo_lock_sha256 == cargo_hash,
        "candidate_provenance_cargo_lock:" + name
    );
    require(
        provenance.artifact_manifest_sha256 == artifact_hash,
        "candidate_provenance_artifact_manifest:" + name
    );
    require(
        provenance.os == os_name && provenance.arch == arch,
        "candidate_provenance_platform:" + name
    );
    if os_name == "macos" {
        require(
            (
                provenance.channel == "release"
                    && provenance.signed
                    && provenance.notarized
            ) || (
                provenance.channel == "macos-unsigned-preview"
                    && !provenance.signed
                    && !provenance.notarized
            ),
            "candidate_provenance_macos_channel:" + name
        );
    } else {
        require(
            provenance.channel == "release"
                && !provenance.signed
                && !provenance.notarized,
            "candidate_provenance_release_channel:" + name
        );
    }
}

fn validate_manifest(source_root_value, manifest, asset_root_value, now_ms) {
    let source_root = std::path::absolute(source_root_value).display;
    let asset_root = std::path::absolute(asset_root_value).display;
    require(
        manifest.schema_version == 1 && manifest.product == "AgenTerm",
        "candidate_schema"
    );
    require(valid_source_sha(manifest.source_sha), "candidate_source_sha");
    require(valid_version(manifest.version), "candidate_version");
    require(
        manifest.expected_tag == "v" + manifest.version
            && manifest.expected_tag != "v0.1.7",
        "candidate_expected_tag"
    );
    require(
        manifest.workflow.path == ".github/workflows/candidate.yml"
            && valid_sha256(manifest.workflow.sha256),
        "candidate_workflow_identity"
    );
    require(
        manifest.run.id > 0
            && manifest.run.attempt > 0
            && manifest.run.event == "workflow_dispatch",
        "candidate_run_identity"
    );
    require(
        manifest.created_at_unix_ms > 0
            && manifest.expires_at_unix_ms > manifest.created_at_unix_ms
            && now_ms >= manifest.created_at_unix_ms
            && now_ms <= manifest.expires_at_unix_ms,
        "candidate_expired"
    );

    let workflow_path =
        std::path::join(source_root, manifest.workflow.path).display;
    require(
        file_hash(workflow_path, "candidate_workflow_missing")
            == manifest.workflow.sha256,
        "candidate_workflow_hash"
    );
    let cargo_path = std::path::join(source_root, "Cargo.lock").display;
    let artifact_path =
        std::path::join(source_root, "scripts/artifacts.json").display;
    let gate_path =
        std::path::join(
            source_root,
            "scripts/qualification-gates.json"
        ).display;
    let cargo_hash = file_hash(cargo_path, "candidate_cargo_lock_missing");
    let artifact_hash =
        file_hash(artifact_path, "candidate_artifact_manifest_missing");
    let gate_hash = file_hash(gate_path, "candidate_gate_manifest_missing");
    require(
        manifest.inputs.cargo_lock_sha256 == cargo_hash
            && manifest.inputs.artifact_manifest_sha256 == artifact_hash
            && manifest.inputs.gate_manifest_sha256 == gate_hash,
        "candidate_source_input_hash"
    );

    let sbom_name =
        "agenterm-" + manifest.version + "-sbom.spdx.json";
    require(
        manifest.sbom.name == sbom_name,
        "candidate_sbom_name"
    );
    let sbom_path = payload_path(asset_root, sbom_name);
    let sbom_hash = file_hash(sbom_path, "candidate_sbom_missing");
    require(
        manifest.sbom.sha256 == sbom_hash
            && manifest.sbom.size == file_size(
                sbom_path,
                "candidate_sbom_missing"
            ),
        "candidate_sbom_identity"
    );

    let receipt_name = "qualification-receipt.json";
    require(
        manifest.qualification.name == receipt_name,
        "candidate_receipt_name"
    );
    let receipt_path = payload_path(asset_root, receipt_name);
    let receipt_hash =
        file_hash(receipt_path, "candidate_receipt_missing");
    require(
        manifest.qualification.sha256 == receipt_hash
            && manifest.qualification.size == file_size(
                receipt_path,
                "candidate_receipt_missing"
            ),
        "candidate_receipt_identity"
    );
    validate_receipt(
        rhai::json::parse_file(receipt_path),
        manifest.source_sha,
        cargo_hash,
        artifact_hash,
        gate_hash,
        sbom_hash,
        rhai::json::parse_file(gate_path)
    );

    require(manifest.assets.len == 6, "candidate_asset_count");
    let expected_files = [receipt_name, sbom_name];
    let mac_channel = "";
    for spec in platform_specs(manifest.version) {
        let asset = one_platform_asset(manifest, spec.os, spec.arch);
        require(
            asset.name == spec.release_name
                || (
                    spec.preview_name != ""
                        && asset.name == spec.preview_name
                ),
            "candidate_archive_name:" + spec.os + "-" + spec.arch
        );
        let archive_path = payload_path(asset_root, asset.name);
        let archive_hash =
            file_hash(archive_path, "candidate_archive_missing");
        require(
            asset.sha256 == archive_hash
                && asset.size == file_size(
                    archive_path,
                    "candidate_archive_missing"
                ),
            "candidate_archive_identity:" + asset.name
        );
        let checksum_name = asset.name + ".sha256";
        let provenance_name = asset.name + ".provenance.json";
        let checksum_path = payload_path(asset_root, checksum_name);
        let provenance_path = payload_path(asset_root, provenance_name);
        require(
            asset.checksum.name == checksum_name
                && asset.checksum.sha256
                    == file_hash(
                        checksum_path,
                        "candidate_checksum_missing"
                    )
                && asset.checksum.size == file_size(
                    checksum_path,
                    "candidate_checksum_missing"
                ),
            "candidate_checksum_identity:" + asset.name
        );
        verify_checksum(checksum_path, archive_hash, asset.name);
        require(
            asset.provenance.name == provenance_name
                && asset.provenance.sha256
                    == file_hash(
                        provenance_path,
                        "candidate_provenance_missing"
                    )
                && asset.provenance.size == file_size(
                    provenance_path,
                    "candidate_provenance_missing"
                ),
            "candidate_provenance_identity:" + asset.name
        );
        let provenance = rhai::json::parse_file(provenance_path);
        validate_provenance(
            provenance,
            asset.name,
            archive_hash,
            manifest.source_sha,
            manifest.version,
            manifest.expected_tag,
            spec.os,
            spec.arch,
            cargo_hash,
            artifact_hash
        );
        require(
            asset.channel == provenance.channel
                && asset.signed == provenance.signed
                && asset.notarized == provenance.notarized,
            "candidate_asset_channel:" + asset.name
        );
        if spec.os == "macos" {
            if mac_channel == "" {
                mac_channel = asset.channel;
            } else {
                require(
                    mac_channel == asset.channel,
                    "candidate_macos_mixed_channels"
                );
            }
        }
        expected_files.push(asset.name);
        expected_files.push(checksum_name);
        expected_files.push(provenance_name);
    }

    let readmes = [
        "MACOS_UNSIGNED_PREVIEW_README.md",
        "MACOS_UNSIGNED_PREVIEW_README.zh-Hant.md"
    ];
    if mac_channel == "macos-unsigned-preview" {
        for name in readmes {
            let path = payload_path(asset_root, name);
            let hash = file_hash(path, "candidate_preview_readme_missing");
            let entry = ();
            for candidate in manifest.unsigned_macos_readmes {
                if candidate.name == name {
                    require(
                        entry == (),
                        "candidate_preview_readme_duplicate:" + name
                    );
                    entry = candidate;
                }
            }
            require(
                entry != ()
                    && entry.sha256 == hash
                    && entry.size == file_size(
                        path,
                        "candidate_preview_readme_missing"
                    ),
                "candidate_preview_readme_identity:" + name
            );
            expected_files.push(name);
        }
        require(
            manifest.unsigned_macos_readmes.len == 2,
            "candidate_preview_readme_count"
        );
    } else {
        require(
            manifest.unsigned_macos_readmes.len == 0,
            "candidate_preview_readme_unexpected"
        );
    }
    require(
        exact_values(expected_files, file_names(asset_root)),
        "candidate_payload_file_set"
    );
    manifest
}

fn build_manifest(
    source_root_value,
    metadata_path_value,
    asset_root_value,
    output_path_value
) {
    let source_root = std::path::absolute(source_root_value).display;
    let metadata_path = std::path::absolute(metadata_path_value).display;
    let asset_root = std::path::absolute(asset_root_value).display;
    let output_path = std::path::absolute(output_path_value).display;
    let metadata = rhai::json::parse_file(metadata_path);
    require(metadata.schema_version == 1, "candidate_metadata_schema");
    require(valid_source_sha(metadata.source_sha), "candidate_source_sha");
    require(valid_version(metadata.version), "candidate_version");
    require(
        metadata.expected_tag == "v" + metadata.version,
        "candidate_expected_tag"
    );

    let workflow_path =
        std::path::join(source_root, metadata.workflow_path).display;
    let workflow_hash =
        file_hash(workflow_path, "candidate_workflow_missing");
    require(
        valid_sha256(metadata.workflow_sha256)
            && metadata.workflow_sha256 == workflow_hash,
        "candidate_metadata_workflow_hash"
    );
    let cargo_path = std::path::join(source_root, "Cargo.lock").display;
    let artifact_path =
        std::path::join(source_root, "scripts/artifacts.json").display;
    let gate_path =
        std::path::join(
            source_root,
            "scripts/qualification-gates.json"
        ).display;
    let sbom_name =
        "agenterm-" + metadata.version + "-sbom.spdx.json";
    let receipt_name = "qualification-receipt.json";
    let sbom_path = payload_path(asset_root, sbom_name);
    let receipt_path = payload_path(asset_root, receipt_name);
    let cargo_hash = file_hash(cargo_path, "candidate_cargo_lock_missing");
    let artifact_hash =
        file_hash(artifact_path, "candidate_artifact_manifest_missing");

    let assets = [];
    let mac_channel = "";
    for spec in platform_specs(metadata.version) {
        let name = expected_archive_name(spec, asset_root);
        let archive_path = payload_path(asset_root, name);
        let hash = file_hash(archive_path, "candidate_archive_missing");
        let checksum_name = name + ".sha256";
        let provenance_name = name + ".provenance.json";
        let checksum_path = payload_path(asset_root, checksum_name);
        let provenance_path = payload_path(asset_root, provenance_name);
        file_hash(checksum_path, "candidate_checksum_missing");
        file_hash(provenance_path, "candidate_provenance_missing");
        verify_checksum(checksum_path, hash, name);
        let provenance = rhai::json::parse_file(provenance_path);
        validate_provenance(
            provenance,
            name,
            hash,
            metadata.source_sha,
            metadata.version,
            metadata.expected_tag,
            spec.os,
            spec.arch,
            cargo_hash,
            artifact_hash
        );
        if spec.os == "macos" {
            if mac_channel == "" {
                mac_channel = provenance.channel;
            } else {
                require(
                    mac_channel == provenance.channel,
                    "candidate_macos_mixed_channels"
                );
            }
        }
        assets.push(#{
            name: name,
            os: spec.os,
            arch: spec.arch,
            channel: provenance.channel,
            signed: provenance.signed,
            notarized: provenance.notarized,
            size: file_size(archive_path, "candidate_archive_missing"),
            sha256: hash,
            checksum: #{
                name: checksum_name,
                size: file_size(
                    checksum_path,
                    "candidate_checksum_missing"
                ),
                sha256: file_hash(
                    checksum_path,
                    "candidate_checksum_missing"
                )
            },
            provenance: #{
                name: provenance_name,
                size: file_size(
                    provenance_path,
                    "candidate_provenance_missing"
                ),
                sha256: file_hash(
                    provenance_path,
                    "candidate_provenance_missing"
                )
            }
        });
    }

    let readme_entries = [];
    if mac_channel == "macos-unsigned-preview" {
        for name in [
            "MACOS_UNSIGNED_PREVIEW_README.md",
            "MACOS_UNSIGNED_PREVIEW_README.zh-Hant.md"
        ] {
            let path = payload_path(asset_root, name);
            readme_entries.push(#{
                name: name,
                size: file_size(
                    path,
                    "candidate_preview_readme_missing"
                ),
                sha256: file_hash(
                    path,
                    "candidate_preview_readme_missing"
                )
            });
        }
    }

    let manifest = #{
        schema_version: 1,
        product: "AgenTerm",
        source_sha: metadata.source_sha,
        version: metadata.version,
        expected_tag: metadata.expected_tag,
        workflow: #{
            path: metadata.workflow_path,
            sha256: workflow_hash
        },
        run: #{
            id: metadata.run_id,
            attempt: metadata.run_attempt,
            event: metadata.run_event
        },
        created_at_unix_ms: metadata.created_at_unix_ms,
        expires_at_unix_ms: metadata.expires_at_unix_ms,
        inputs: #{
            cargo_lock_sha256: cargo_hash,
            artifact_manifest_sha256: artifact_hash,
            gate_manifest_sha256:
                file_hash(gate_path, "candidate_gate_manifest_missing")
        },
        qualification: #{
            name: receipt_name,
            size: file_size(receipt_path, "candidate_receipt_missing"),
            sha256: file_hash(receipt_path, "candidate_receipt_missing")
        },
        sbom: #{
            name: sbom_name,
            size: file_size(sbom_path, "candidate_sbom_missing"),
            sha256: file_hash(sbom_path, "candidate_sbom_missing")
        },
        assets: assets,
        unsigned_macos_readmes: readme_entries
    };
    validate_manifest(
        source_root,
        manifest,
        asset_root,
        std::time::SystemTime::now().unix_millis
    );
    std::fs::create_dir_all(std::path::parent(output_path).display);
    rhai::runtime::atomic_write(
        output_path,
        rhai::json::stringify_pretty(manifest) + "\n"
    );
    manifest
}

fn verify_manifest(
    source_root_value,
    manifest_path_value,
    asset_root_value,
    now_ms
) {
    validate_manifest(
        source_root_value,
        rhai::json::parse_file(
            std::path::absolute(manifest_path_value).display
        ),
        asset_root_value,
        now_ms
    )
}
