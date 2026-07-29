// Prove the offline packager accepts only the exact qualified byte set.
//
// Args: REPO

import "scripts/rhai/lib/package_qualified" as package_qualified;

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

fn expect_rejection(expected, action) {
    let rejected = false;
    try {
        action.call();
    } catch (error) {
        let message = error.to_string();
        require(
            message.contains(expected),
            "package_selftest_wrong_rejection:" +
                expected + ":" + message
        );
        rejected = true;
    }
    require(
        rejected,
        "package_selftest_expected_rejection:" + expected
    );
}

fn zeros(count) {
    let value = "";
    for index in 0..count {
        value += "0";
    }
    value
}

fn write_json(path, value) {
    rhai::runtime::atomic_write(
        path,
        rhai::json::stringify_pretty(value) + "\n"
    );
}

fn command_output(program, arguments, cwd, code) {
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

fn exact_values(expected, actual) {
    expected.sort();
    actual.sort();
    if expected.len != actual.len {
        return false;
    }
    for index in 0..expected.len {
        if expected[index] != actual[index] {
            return false;
        }
    }
    true
}

fn only_result(output_root) {
    let results = [];
    for run in std::fs::read_dir(output_root) {
        if run.is_dir {
            let path = std::path::join(
                run.path.display,
                "dry-run-result.json"
            ).display;
            if std::fs::exists(path) {
                results.push(path);
            }
        }
    }
    require(
        results.len == 1,
        "package_selftest_result_count:" + results.len
    );
    results[0]
}

require(args.len == 1, "expected: REPO");
let repo = std::path::absolute(args[0]).display;
let owned_root =
    std::path::join(repo, "target/qualification").display;
std::fs::create_dir_all(owned_root);
let fixture = std::path::join(
    owned_root,
    "package-selftest-" +
        std::time::SystemTime::now().unix_millis + "-" +
        std::process::id()
).display;
std::fs::create_dir_all(fixture);
let failure = "";

try {
    for directory in [
        "dist",
        "scripts",
        "target/qualification"
    ] {
        std::fs::create_dir_all(
            std::path::join(fixture, directory).display
        );
    }
    std::fs::write(
        std::path::join(fixture, ".gitignore").display,
        "/dist/\n/target/\n"
    );
    std::fs::write(
        std::path::join(fixture, "Cargo.lock").display,
        "fixture lock\n"
    );
    for name in [
        "LICENSE-APACHE",
        "LICENSE-MIT",
        "THIRD_PARTY_NOTICES.md"
    ] {
        std::fs::write(
            std::path::join(fixture, name).display,
            "fixture " + name + "\n"
        );
    }
    for name in [
        "artifacts.json",
        "qualification-gates.json"
    ] {
        std::fs::copy(
            std::path::join(
                std::path::join(repo, "scripts").display,
                name
            ).display,
            std::path::join(
                std::path::join(fixture, "scripts").display,
                name
            ).display
        );
    }
    command_output("git", ["init", "--quiet"], fixture, "package_fixture_init");
    command_output("git", ["add", "."], fixture, "package_fixture_add");
    command_output(
        "git",
        [
            "-c", "user.name=qualification-selftest",
            "-c", "user.email=qualification-selftest.invalid",
            "commit", "--quiet", "-m", "fixture"
        ],
        fixture,
        "package_fixture_commit"
    );
    let head = command_output(
        "git",
        ["rev-parse", "HEAD"],
        fixture,
        "package_fixture_head"
    ).to_lower();

    let artifact_path = std::path::join(
        fixture,
        "scripts/artifacts.json"
    ).display;
    let artifact_spec = rhai::json::parse_file(artifact_path);
    let executable_records = [];
    for artifact in artifact_spec.executables {
        let path = std::path::join(
            std::path::join(fixture, "dist").display,
            artifact.name
        ).display;
        std::fs::write(path, "fixture " + artifact.name + "\n");
        executable_records.push(#{
            name: artifact.name,
            role: artifact.role,
            size: std::fs::metadata(path).len,
            sha256: rhai::crypto::sha256_file(path)
        });
    }
    let cargo_path =
        std::path::join(fixture, "Cargo.lock").display;
    let cargo_hash = rhai::crypto::sha256_file(cargo_path);
    let artifact_hash = rhai::crypto::sha256_file(artifact_path);
    let sbom_path = std::path::join(
        fixture,
        "dist/agenterm-sbom.spdx.json"
    ).display;
    std::fs::write(
        sbom_path,
        "fixture deterministic SPDX inventory\n"
    );
    let sbom_hash = rhai::crypto::sha256_file(sbom_path);
    let gate_path = std::path::join(
        fixture,
        "scripts/qualification-gates.json"
    ).display;
    let gate_hash = rhai::crypto::sha256_file(gate_path);
    let gate_spec = rhai::json::parse_file(gate_path);
    let metadata = #{
        schema_version: 2,
        product: "AgenTerm",
        version: "0.1.7",
        git_commit: head,
        git_dirty: false,
        cargo_lock_sha256: cargo_hash,
        artifact_manifest_sha256: artifact_hash,
        executables: executable_records
    };
    write_json(
        std::path::join(fixture, "dist/agenterm.json").display,
        metadata
    );
    let receipt_executables = [];
    for entry in executable_records {
        receipt_executables.push(#{
            name: entry.name,
            sha256: entry.sha256
        });
    }
    let gates = [];
    for gate in gate_spec.required_gates {
        gates.push(#{ id: gate.id, status: "passed" });
    }
    let receipt = #{
        schema_version: 1,
        product: "AgenTerm",
        release: true,
        stress_included: true,
        gate_manifest: #{ sha256: gate_hash },
        provenance: #{
            git_head: head,
            source_dirty: false,
            cargo_lock_sha256: cargo_hash,
            artifact_manifest_sha256: artifact_hash,
            sbom_sha256: sbom_hash,
            executables: receipt_executables
        },
        gates: gates
    };
    let receipt_path = std::path::join(
        fixture,
        "target/qualification/receipt.json"
    ).display;
    write_json(receipt_path, receipt);

    let output_relative =
        "target/qualification/package-dry-run";
    let run_directory = package_qualified::build_package(
        fixture,
        "target/qualification/receipt.json",
        output_relative
    );
    let result_path = only_result(
        std::path::join(fixture, output_relative).display
    );
    let result = rhai::json::parse_file(result_path);
    let archive =
        std::path::join(run_directory, result.archive).display;
    require(
        result.dry_run
            && std::fs::exists(archive)
            && result.archive_sha256 ==
                rhai::crypto::sha256_file(archive),
        "package_selftest_result_incomplete"
    );
    let listing = command_output(
        "tar.exe",
        ["-tf", archive],
        fixture,
        "package_selftest_archive_list"
    );
    let archive_names = [];
    for line in listing.split("\n") {
        let name = line;
        name.trim();
        if name != "" {
            archive_names.push(name);
        }
    }
    let expected_names = [];
    for artifact in artifact_spec.executables {
        expected_names.push(artifact.name);
    }
    expected_names.append([
        "agenterm.json",
        "artifacts.json",
        "qualification-receipt.json",
        "agenterm-sbom.spdx.json",
        "LICENSE-APACHE",
        "LICENSE-MIT",
        "THIRD_PARTY_NOTICES.md",
        "package-manifest.json"
    ]);
    require(
        exact_values(expected_names, archive_names),
        "package_selftest_archive_members"
    );

    receipt.provenance.cargo_lock_sha256 = zeros(64);
    write_json(receipt_path, receipt);
    expect_rejection(
        "package_receipt_cargo_lock_hash",
        || package_qualified::build_package(
            fixture,
            "target/qualification/receipt.json",
            "target/qualification/rejected-cargo"
        )
    );
    receipt.provenance.cargo_lock_sha256 = cargo_hash;
    receipt.provenance.artifact_manifest_sha256 = zeros(64);
    write_json(receipt_path, receipt);
    expect_rejection(
        "package_receipt_artifact_manifest_hash",
        || package_qualified::build_package(
            fixture,
            "target/qualification/receipt.json",
            "target/qualification/rejected-manifest"
        )
    );
    receipt.provenance.artifact_manifest_sha256 = artifact_hash;
    write_json(receipt_path, receipt);

    let first_name = artifact_spec.executables[0].name;
    let first_path = std::path::join(
        std::path::join(fixture, "dist").display,
        first_name
    ).display;
    std::fs::write(
        first_path,
        std::fs::read_to_string(first_path) + "tampered\n"
    );
    expect_rejection(
        "package_executable_hash:" + first_name,
        || package_qualified::build_package(
            fixture,
            "target/qualification/receipt.json",
            "target/qualification/rejected-output"
        )
    );
    receipt.provenance.git_head = zeros(40);
    write_json(receipt_path, receipt);
    expect_rejection(
        "package_receipt_head_mismatch",
        || package_qualified::build_package(
            fixture,
            "target/qualification/receipt.json",
            "target/qualification/rejected-head"
        )
    );

    let source =
        std::fs::read_to_string(
            std::path::join(
                repo,
                "scripts/rhai/lib/package_qualified.rhai"
            ).display
        ) +
        std::fs::read_to_string(
            std::path::join(
                repo,
                "scripts/rhai/package-qualified.rhai"
            ).display
        );
    for forbidden in [
        "build.bat",
        "cargo build",
        "git tag",
        "git push",
        "gh release",
        "release.ps1"
    ] {
        require(
            !source.contains(forbidden),
            "package_selftest_forbidden_action:" + forbidden
        );
    }
} catch (error) {
    failure = error.to_string();
}

let resolved = std::path::absolute(fixture).display;
require(
    std::path::parent(resolved).display.to_lower() ==
        std::path::absolute(owned_root).display.to_lower(),
    "package_selftest_fixture_outside_owned_root"
);
if std::fs::exists(resolved) {
    std::fs::remove_dir_all(resolved);
}
if failure != "" {
    throw failure;
}
print("PASS: qualified package dry-run self-test");
