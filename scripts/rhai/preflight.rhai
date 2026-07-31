// Read-only repository release preflight with a durable JSON report.
//
// Args:
//   REPO OUTPUT_PATH [--quiet]

import "scripts/rhai/lib/artifact_manifest" as artifact_manifest;

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

fn checked_output(program, arguments, error_code) {
    let command = std::process::command(program);
    command.args(arguments);
    command.timeout(std::time::Duration::from_secs(10));
    let output = command.output();
    require(output.success, output.error(error_code));
    output.stdout_text()
}

fn git_output(repo, arguments, error_code) {
    let argv = ["-C", repo];
    argv.append(arguments);
    checked_output("git", argv, error_code)
}

fn normalized(value) {
    let result = value;
    result.replace("\r\n", "\n");
    result.replace("\r", "\n");
    result
}

fn one_line(value) {
    let result = normalized(value);
    result.replace("\n", " ");
    while result.contains("  ") {
        result.replace("  ", " ");
    }
    result.trim();
    result
}

fn gate(id, passed, detail) {
    #{ id: id, passed: passed, detail: one_line(detail) }
}

fn is_lower_hex(value, length) {
    if value.len != length {
        return false;
    }
    for character in value {
        if !"0123456789abcdef".contains(character.to_string()) {
            return false;
        }
    }
    true
}

fn is_digits(value) {
    if value == "" {
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
    let core = value;
    if core.contains("+") {
        let parts = core.split("+");
        if parts.len != 2 || parts[1] == "" {
            return false;
        }
        core = parts[0];
    }
    if core.contains("-") {
        let parts = core.split("-");
        if parts.len < 2 {
            return false;
        }
        let suffix = "";
        for index in 1..parts.len {
            if suffix != "" {
                suffix += "-";
            }
            suffix += parts[index];
        }
        if suffix == "" {
            return false;
        }
        core = parts[0];
    }
    let numbers = core.split(".");
    if numbers.len != 3 {
        return false;
    }
    for number in numbers {
        if !is_digits(number) {
            return false;
        }
    }
    for character in value {
        let text = character.to_string();
        if !"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ.+-"
                .contains(text) {
            return false;
        }
    }
    true
}

fn valid_rust_version(value) {
    let numbers = value.split(".");
    numbers.len == 2 && is_digits(numbers[0]) && is_digits(numbers[1])
}

fn valid_channel(value) {
    let numbers = value.split(".");
    numbers.len == 3
        && is_digits(numbers[0])
        && is_digits(numbers[1])
        && is_digits(numbers[2])
}

fn section_string(source, section, key) {
    let active = false;
    for source_line in normalized(source).split("\n") {
        let line = source_line;
        line.trim();
        if line == "[" + section + "]" {
            active = true;
            continue;
        }
        if line.starts_with("[") {
            active = false;
        }
        if active
                && (line.starts_with(key + " ")
                    || line.starts_with(key + "=")) {
            let parts = line.split("\"");
            if parts.len >= 3 {
                return parts[1];
            }
        }
    }
    ""
}

fn cargo_lock_facts(source, expected_version) {
    let lock_version = "";
    let package_count = 0;
    let checksum_count = 0;
    let invalid_checksum_count = 0;
    let in_package = false;
    let package_name = "";
    let package_version = "";
    let root_version = "";

    for source_line in normalized(source).split("\n") {
        let line = source_line;
        line.trim();
        if line == "[[package]]" {
            if in_package && package_name == "agenterm" {
                root_version = package_version;
            }
            in_package = true;
            package_name = "";
            package_version = "";
            package_count += 1;
            continue;
        }
        if !in_package && line.starts_with("version") {
            let fields = line.split("=");
            if fields.len == 2 {
                lock_version = fields[1];
                lock_version.trim();
            }
            continue;
        }
        if in_package && line.starts_with("name") {
            let parts = line.split("\"");
            if parts.len >= 3 {
                package_name = parts[1];
            }
            continue;
        }
        if in_package && line.starts_with("version") {
            let parts = line.split("\"");
            if parts.len >= 3 {
                package_version = parts[1];
            }
            continue;
        }
        if in_package && line.starts_with("checksum") {
            let parts = line.split("\"");
            checksum_count += 1;
            if parts.len < 3 || !is_lower_hex(parts[1], 64) {
                invalid_checksum_count += 1;
            }
        }
    }
    if in_package && package_name == "agenterm" {
        root_version = package_version;
    }

    require(lock_version == "4", "Cargo.lock is not lockfile version 4.");
    require(
        root_version == expected_version,
        "Cargo.lock AgenTerm version does not match Cargo.toml."
    );
    require(
        checksum_count > 0,
        "Cargo.lock contains no registry checksums."
    );
    require(
        invalid_checksum_count == 0,
        "Cargo.lock contains " + invalid_checksum_count +
            " invalid checksum hash(es)."
    );
    #{ packages: package_count, checksums: checksum_count }
}

fn redact_remote(url) {
    if (url.starts_with("http://") || url.starts_with("https://"))
            && url.contains("@") {
        let scheme_parts = url.split("://");
        if scheme_parts.len == 2 {
            let authority_parts = scheme_parts[1].split("@");
            if authority_parts.len >= 2 {
                return scheme_parts[0] + "://<redacted>@" +
                    authority_parts[authority_parts.len - 1];
            }
        }
    }
    url
}

fn valid_gate_id(value) {
    if value.len < 2 {
        return false;
    }
    for character in value {
        if !"abcdefghijklmnopqrstuvwxyz0123456789-"
                .contains(character.to_string()) {
            return false;
        }
    }
    !value.starts_with("-")
}

require(
    args.len == 2 || (args.len == 3 && args[2] == "--quiet"),
    "expected: REPO OUTPUT_PATH [--quiet]"
);
let started = std::time::SystemTime::now().unix_millis;
let repo = std::path::absolute(args[0]).display;
let output_value = std::path::PathBuf::from(args[1]);
let output = if output_value.is_absolute {
    output_value.display
} else {
    std::path::join(repo, args[1]).display
};
let quiet = args.len == 3;
let gates = [];
let branch = "";
let head = "";
let version = "";
let rust_version = "";
let remotes = [];

try {
    branch = git_output(
        repo,
        ["branch", "--show-current"],
        "preflight-git-branch"
    );
    branch.trim();
    require(branch == "main", "Expected branch main, found '" + branch + "'.");
    gates.push(gate("branch-main", true, "main"));
} catch (error) {
    gates.push(gate("branch-main", false, error.to_string()));
}

try {
    head = git_output(repo, ["rev-parse", "HEAD"], "preflight-git-head");
    head.trim();
    head.to_lower();
    require(
        is_lower_hex(head, 40),
        "HEAD is not a full 40-character commit hash: " + head
    );
    gates.push(gate("full-head", true, head));
} catch (error) {
    gates.push(gate("full-head", false, error.to_string()));
}

try {
    let status = git_output(
        repo,
        ["status", "--porcelain", "--untracked-files=normal"],
        "preflight-git-status"
    );
    status.trim();
    require(
        status == "",
        "Working tree is dirty (" +
            (if status == "" { 0 } else { status.split("\n").len }) +
            " path(s))."
    );
    gates.push(gate("clean-tree", true, "clean"));
} catch (error) {
    gates.push(gate("clean-tree", false, error.to_string()));
}

try {
    let cargo = std::fs::read_to_string(
        std::path::join(repo, "Cargo.toml").display
    );
    let name = section_string(cargo, "package", "name");
    version = section_string(cargo, "package", "version");
    rust_version = section_string(cargo, "package", "rust-version");
    require(
        name == "agenterm"
            && valid_version(version)
            && valid_rust_version(rust_version),
        "Cargo package name, version, or rust-version is invalid."
    );
    gates.push(gate(
        "cargo-package",
        true,
        "version=" + version + " rust=" + rust_version
    ));
} catch (error) {
    gates.push(gate("cargo-package", false, error.to_string()));
}

try {
    let lock = std::fs::read_to_string(
        std::path::join(repo, "Cargo.lock").display
    );
    let facts = cargo_lock_facts(lock, version);
    gates.push(gate(
        "cargo-lock",
        true,
        "packages=" + facts.packages + " checksums=" + facts.checksums
    ));
} catch (error) {
    gates.push(gate("cargo-lock", false, error.to_string()));
}

try {
    let toolchain = std::fs::read_to_string(
        std::path::join(repo, "rust-toolchain.toml").display
    );
    let channel = section_string(toolchain, "toolchain", "channel");
    require(
        valid_channel(channel)
            && normalized(toolchain).contains("profile = \"minimal\"")
            && normalized(toolchain).contains("\"clippy\"")
            && normalized(toolchain).contains("\"rustfmt\"")
            && !normalized(toolchain).contains("targets ="),
        "rust-toolchain.toml is incomplete or invalid."
    );
    let channel_numbers = channel.split(".");
    require(
        channel_numbers[0] + "." + channel_numbers[1] == rust_version,
        "Cargo rust-version and pinned toolchain channel disagree."
    );
    gates.push(gate(
        "toolchain",
        true,
        "channel=" + channel + " targets=host-only"
    ));
} catch (error) {
    gates.push(gate("toolchain", false, error.to_string()));
}

try {
    if version != "0.1.7" {
        gates.push(gate(
            "internal-version-policy",
            true,
            "not-applicable version=" + version
        ));
    } else {
        let tags = git_output(
            repo,
            ["tag", "--list", "v0.1.7"],
            "preflight-git-tags"
        );
        tags.trim();
        require(
            tags == "",
            "Internal-only v0.1.7 must have no local v0.1.7 tag."
        );
        let release = std::fs::read_to_string(
            std::path::join(repo, "scripts/rhai/release.rhai").display
        );
        let workflow = std::fs::read_to_string(
            std::path::join(repo, ".github/workflows/candidate.yml").display
        );
        require(
            release.contains(`version != "0.1.7"`)
                && (workflow.contains("expected -eq 'v0.1.7'")
                    || workflow.contains("expected == \"v0.1.7\"")),
            "Internal v0.1.7 publication rejection policy is missing."
        );
        gates.push(gate(
            "internal-version-policy",
            true,
            "internal-only untagged"
        ));
    }
} catch (error) {
    gates.push(gate(
        "internal-version-policy",
        false,
        error.to_string()
    ));
}

try {
    let source = git_output(
        repo,
        ["config", "--local", "--get-regexp",
         "^remote\\..*\\.(url|pushurl)$"],
        "preflight-git-remotes"
    );
    let origin_count = 0;
    for source_line in source.split("\n") {
        let line = source_line;
        line.trim();
        if line == "" {
            continue;
        }
        while line.contains("  ") {
            line.replace("  ", " ");
        }
        let fields = line.split(" ");
        require(fields.len >= 2, "Could not parse a local Git remote entry.");
        let key = fields[0];
        let key_parts = key.split(".");
        require(
            key_parts.len == 3
                && key_parts[0] == "remote"
                && (key_parts[2] == "url" || key_parts[2] == "pushurl"),
            "Could not parse a local Git remote entry."
        );
        let url = line;
        url.replace(key + " ", "");
        if key_parts[1] == "origin" {
            origin_count += 1;
        }
        remotes.push(#{
            name: key_parts[1],
            kind: key_parts[2],
            url: redact_remote(url),
            embedded_credentials:
                (url.starts_with("http://")
                    || url.starts_with("https://")) && url.contains("@")
        });
    }
    require(origin_count > 0, "Local Git configuration has no origin remote.");
    gates.push(gate(
        "remote-config",
        true,
        "entries=" + remotes.len
    ));
} catch (error) {
    gates.push(gate("remote-config", false, error.to_string()));
}

try {
    let manifest_path =
        std::path::join(repo, "scripts/artifacts.json").display;
    let manifest = artifact_manifest::load(manifest_path);
    let expected = [
        "agenterm.exe",
        "agenterm-cc.exe",
        "agenterm-server.exe",
        "agenterm-cli.exe",
        "agenterm-mux.exe",
        "agenterm-script.exe",
        "agenterm-mcp.exe"
    ];
    let names = #{};
    for entry in manifest.executables {
        names[entry.name] = true;
    }
    require(
        names.keys().len == expected.len,
        "Artifact manifest schema or executable set is invalid."
    );
    for name in expected {
        require(
            names.contains(name),
            "Artifact manifest schema or executable set is invalid."
        );
    }
    gates.push(gate(
        "artifact-manifest",
        true,
        "executables=" + names.keys().len
    ));
} catch (error) {
    gates.push(gate("artifact-manifest", false, error.to_string()));
}

try {
    let manifest = rhai::json::parse(std::fs::read_to_string(
        std::path::join(repo, "scripts/qualification-gates.json").display
    ));
    require(
        manifest.schema_version == 1
            && manifest.receipt_schema_version == 1
            && type_of(manifest.required_gates) == "array"
            && manifest.required_gates.len > 0,
        "Qualification gate manifest schema or gate IDs are invalid."
    );
    let ids = #{};
    for entry in manifest.required_gates {
        let id = entry.id;
        require(
            valid_gate_id(id) && !ids.contains(id),
            "Qualification gate manifest schema or gate IDs are invalid."
        );
        ids[id] = true;
    }
    gates.push(gate(
        "gate-manifest",
        true,
        "required_gates=" + ids.keys().len
    ));
} catch (error) {
    gates.push(gate("gate-manifest", false, error.to_string()));
}

let failed = 0;
for item in gates {
    if !item.passed {
        failed += 1;
    }
}
let report = #{
    schema_version: 1,
    kind: "agenterm-read-only-preflight",
    passed: failed == 0,
    duration_ms:
        std::time::SystemTime::now().unix_millis - started,
    repo: repo,
    branch: branch,
    head: head,
    version: version,
    remotes: remotes,
    gates: gates
};
std::fs::create_dir_all(std::path::parent(output).display);
rhai::runtime::atomic_write(
    output,
    rhai::json::stringify_pretty(report) + "\n"
);

if !quiet {
    print(
        "PREFLIGHT passed=" + report.passed +
        " duration_ms=" + report.duration_ms +
        " branch=" + branch + " head=" + head + " version=" + version
    );
    for item in gates {
        print(
            "  " + (if item.passed { "PASS" } else { "FAIL" }) +
            " " + item.id + ": " + item.detail
        );
    }
    print("PREFLIGHT JSON " + output);
}
require(
    report.passed,
    "AgenTerm read-only preflight failed; report:" + output
);
