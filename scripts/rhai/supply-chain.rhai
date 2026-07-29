// Validate locked Cargo licenses and write a deterministic SPDX 2.3 inventory.
//
// Args:
//   REPO OUTPUT_PATH

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

fn normalized(value) {
    let result = value;
    result.replace("\r\n", "\n");
    result.replace("\r", "\n");
    result
}

fn quoted_value(line) {
    let parts = line.split("\"");
    if parts.len >= 3 {
        parts[1]
    } else {
        ""
    }
}

fn lock_checksums(source) {
    let checksums = #{};
    let name = "";
    let version = "";
    let package_source = "";
    let checksum = "";

    for source_line in normalized(source).split("\n") {
        let line = source_line;
        line.trim();
        if line == "[[package]]" {
            if checksum != "" {
                checksums[name + "\n" + version + "\n" + package_source] =
                    checksum;
            }
            name = "";
            version = "";
            package_source = "";
            checksum = "";
            continue;
        }
        if line.starts_with("name = ") {
            name = quoted_value(line);
        } else if line.starts_with("version = ") {
            version = quoted_value(line);
        } else if line.starts_with("source = ") {
            package_source = quoted_value(line);
        } else if line.starts_with("checksum = ") {
            checksum = quoted_value(line);
        }
    }
    if checksum != "" {
        checksums[name + "\n" + version + "\n" + package_source] = checksum;
    }
    checksums
}

fn allowed_licenses() {
    let allowed = #{};
    for license in [
        "(MIT OR Apache-2.0) AND Unicode-3.0",
        "0BSD OR MIT OR Apache-2.0",
        "Apache-2.0",
        "Apache-2.0 AND ISC",
        "Apache-2.0 AND MIT",
        "Apache-2.0 OR ISC OR MIT",
        "Apache-2.0 OR MIT",
        "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT",
        "BSD-2-Clause",
        "BSD-2-Clause OR Apache-2.0 OR MIT",
        "BSD-3-Clause",
        "BSD-3-Clause OR MIT OR Apache-2.0",
        "CC0-1.0",
        "CDLA-Permissive-2.0",
        "ISC",
        "MIT",
        "MIT OR Apache-2.0",
        "MIT OR Apache-2.0 OR LGPL-2.1-or-later",
        "MIT OR Apache-2.0 OR Zlib",
        "MIT OR Zlib OR Apache-2.0",
        "MIT/Apache-2.0",
        "MPL-2.0+",
        "Unlicense/MIT",
        "Unlicense OR MIT",
        "Zlib OR Apache-2.0 OR MIT"
    ] {
        allowed[license] = true;
    }
    allowed
}

fn package_sort_key(pkg) {
    pkg.name + "\n" + pkg.version + "\n" +
        pkg.source + "\n" + pkg.id
}

fn sorted_packages(packages) {
    let sorted = [];
    for pkg in packages {
        let key = package_sort_key(pkg);
        let inserted = false;
        for index in 0..sorted.len {
            if key < package_sort_key(sorted[index]) {
                sorted.insert(index, pkg);
                inserted = true;
                break;
            }
        }
        if !inserted {
            sorted.push(pkg);
        }
    }
    sorted
}

fn spdx_license(license) {
    if license == "MIT/Apache-2.0" {
        "MIT OR Apache-2.0"
    } else if license == "MPL-2.0+" {
        "MPL-2.0-or-later"
    } else {
        license
    }
}

require(args.len == 2, "expected: REPO OUTPUT_PATH");
let repo = std::path::absolute(args[0]).display;
let output_value = std::path::PathBuf::from(args[1]);
let output = if output_value.is_absolute {
    output_value.display
} else {
    std::path::join(repo, args[1]).display
};
let cargo_lock_path = std::path::join(repo, "Cargo.lock").display;
let checksums =
    lock_checksums(std::fs::read_to_string(cargo_lock_path));
let output_parent = std::path::parent(output).display;
std::fs::create_dir_all(output_parent);
let metadata_path = std::path::join(
    output_parent,
    "cargo-metadata-" + std::time::SystemTime::now().unix_millis + ".json"
).display;
let command = std::process::command("cargo");
command.args([
    "metadata", "--locked", "--format-version", "1",
    "--manifest-path", std::path::join(repo, "Cargo.toml").display
]);
command.timeout(std::time::Duration::from_secs(10));
command.stdout_file(metadata_path);
let metadata_output = command.output();
require(
    metadata_output.success,
    metadata_output.error("supply_chain_cargo_metadata")
);
let metadata = rhai::json::parse_file(metadata_path);
std::fs::remove_file(metadata_path);
require(metadata.contains("resolve"), "supply_chain_resolve_missing");
let workspace_ids = #{};
for id in metadata.workspace_members {
    workspace_ids[id] = true;
}
let resolved_ids = #{};
for node in metadata.resolve.nodes {
    resolved_ids[node.id] = true;
}
let dependencies = [];
let roots = [];
for pkg in metadata.packages {
    if workspace_ids.contains(pkg.id) {
        roots.push(pkg);
    } else if resolved_ids.contains(pkg.id) {
        dependencies.push(pkg);
    }
}
require(dependencies.len > 0, "supply_chain_dependencies_empty");
require(roots.len == 1, "supply_chain_workspace_package_count");
dependencies = sorted_packages(dependencies);

let allowed = allowed_licenses();
for pkg in dependencies {
    let license = pkg.license.to_string();
    let package_source = pkg.source.to_string();
    require(
        license != "" && license != "()",
        "supply_chain_license_missing:" + pkg.name + ":" + pkg.version
    );
    require(
        allowed.contains(license),
        "supply_chain_license_unreviewed:" + pkg.name + ":" +
            pkg.version + ":" + license
    );
    require(
        package_source != "" && package_source != "()",
        "supply_chain_source_missing:" + pkg.name + ":" + pkg.version
    );
    let key =
        pkg.name + "\n" + pkg.version + "\n" + package_source;
    if package_source.starts_with("registry+") {
        require(
            checksums.contains(key),
            "supply_chain_checksum_missing:" + pkg.name + ":" +
                pkg.version
        );
    }
}

let direct_names = #{};
for dependency in roots[0].dependencies {
    direct_names[dependency.name] = true;
}
let notice_names = #{};
for source_line in std::fs::read_to_string(
        std::path::join(repo, "THIRD_PARTY_NOTICES.md").display
    ).split("\n") {
    let line = source_line;
    line.trim();
    if line.starts_with("| `") {
        let fields = line.split("`");
        if fields.len >= 3 && fields[1] != "" {
            notice_names[fields[1]] = true;
        }
    }
}
require(
    direct_names.keys().len == notice_names.keys().len,
    "supply_chain_notice_count"
);
for name in direct_names.keys() {
    require(
        notice_names.contains(name),
        "supply_chain_notice_missing:" + name
    );
}

let cargo_lock_hash = rhai::crypto::sha256_file(cargo_lock_path);
let spdx_packages = [];
let relationships = [];
let described = [];
for pkg in dependencies {
    let package_source = pkg.source.to_string();
    let key =
        pkg.name + "\n" + pkg.version + "\n" + package_source;
    let id_hash = rhai::crypto::sha256(
        rhai::bytes::from_text(pkg.id)
    );
    let spdx_id = "SPDXRef-Package-" + id_hash.sub_string(0, 16);
    let record = #{
        SPDXID: spdx_id,
        name: pkg.name,
        versionInfo: pkg.version,
        downloadLocation: "NOASSERTION",
        filesAnalyzed: false,
        licenseConcluded: spdx_license(pkg.license.to_string()),
        licenseDeclared: spdx_license(pkg.license.to_string()),
        copyrightText: "NOASSERTION",
        comment: "Cargo source: " + package_source,
        externalRefs: [
            #{
                referenceCategory: "PACKAGE-MANAGER",
                referenceType: "purl",
                referenceLocator:
                    "pkg:cargo/" + pkg.name + "@" + pkg.version
            }
        ]
    };
    if checksums.contains(key) {
        record.checksums = [
            #{
                algorithm: "SHA256",
                checksumValue: checksums[key]
            }
        ];
    }
    spdx_packages.push(record);
    described.push(spdx_id);
    relationships.push(#{
        spdxElementId: "SPDXRef-DOCUMENT",
        relationshipType: "DESCRIBES",
        relatedSpdxElement: spdx_id
    });
}

let document = #{
    spdxVersion: "SPDX-2.3",
    dataLicense: "CC0-1.0",
    SPDXID: "SPDXRef-DOCUMENT",
    name: "AgenTerm Cargo dependencies " + cargo_lock_hash,
    documentNamespace:
        "https://agenterm.local/spdx/cargo-lock/" + cargo_lock_hash,
    creationInfo: #{
        created: "1970-01-01T00:00:00Z",
        creators: ["Tool: agenterm-script task run supply-chain"],
        comment:
            "Generation time is fixed so identical Cargo.lock inputs produce identical bytes."
    },
    documentDescribes: described,
    packages: spdx_packages,
    relationships: relationships
};
rhai::runtime::atomic_write(
    output,
    rhai::json::stringify_pretty(document) + "\n"
);
print(
    "PASS: wrote deterministic SPDX inventory for " + dependencies.len +
    " resolved packages to " + output
);
