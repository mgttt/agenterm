// Package one manifest-defined cross-platform client release.
//
// Args:
//   REPO VERSION OS ARCH BINARY_DIRECTORY [--unsigned-preview|--signed]
//
// Platform credential import, signing, notarization and system-library
// installation remain host adapters. Payload selection and archive policy live
// here and are driven only by scripts/artifacts.json.

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

fn run(program, arguments, cwd, code) {
    let command = std::process::command(program);
    command.args(arguments);
    command.current_dir(cwd);
    command.timeout(std::time::Duration::from_secs(300));
    let output = command.output();
    require(output.success, output.error(code));
}

fn one_platform(manifest, os_name, arch) {
    let matches = [];
    for platform in manifest.platforms {
        if platform.os == os_name && platform.arch == arch {
            matches.push(platform);
        }
    }
    require(
        matches.len == 1,
        "package_client_platform_match:" + os_name + "-" + arch
    );
    matches[0]
}

fn copy_required(source, destination, code) {
    require(std::fs::exists(source), code + ":" + source);
    std::fs::copy(source, destination);
}

fn environment_or_empty(name) {
    if std::env::has(name) {
        std::env::get(name)
    } else {
        ""
    }
}

require(args.len == 5 || args.len == 6, "package_client_arguments");
let repo = std::path::absolute(args[0]).display;
let version = args[1];
let os_name = args[2];
let arch = args[3];
let binary_directory = if std::fs::exists(args[4]) {
    args[4]
} else {
    std::path::join(repo, args[4]).display
};
let mode = if args.len == 6 { args[5] } else { "" };
require(
    mode == "" || mode == "--unsigned-preview" || mode == "--signed",
    "package_client_mode:" + mode
);
let unsigned_preview = mode == "--unsigned-preview";
let signed = mode == "--signed";
let host_os = if std::env::has("AGENTERM_HOST_OS") {
    std::env::get("AGENTERM_HOST_OS")
} else if std::fs::exists("/System/Library/CoreServices") {
    "macos"
} else if std::fs::exists("/proc") {
    "linux"
} else {
    "windows"
};
let host_arch = environment_or_empty("AGENTERM_HOST_ARCH");
require(
    !version.contains("/") && !version.contains("\\") && version != "",
    "package_client_version"
);
require(
    std::fs::exists(binary_directory),
    "package_client_binary_directory:" + binary_directory
);

let artifact_manifest_path =
    std::path::join(repo, "scripts/artifacts.json").display;
let artifact_manifest = rhai::json::parse_file(artifact_manifest_path);
require(artifact_manifest.schema_version == 2, "package_client_manifest_schema");
let platform = one_platform(artifact_manifest, os_name, arch);
let package_format = platform["package"];
require(
    package_format == "zip" || package_format == "tar.gz",
    "package_client_format:" + package_format
);
require(
    !unsigned_preview || os_name == "macos",
    "package_client_preview_platform"
);
require(!signed || os_name == "macos", "package_client_signed_platform");

let dist = if std::env::has("AGENTERM_PACKAGE_DIST") {
    std::path::absolute(std::env::get("AGENTERM_PACKAGE_DIST")).display
} else {
    std::path::join(repo, "dist").display
};
std::fs::create_dir_all(dist);
let scratch = rhai::runtime::temp_dir().display;
let staging = std::path::join(scratch, "payload").display;
std::fs::create_dir_all(staging);
let failure = "";
let archive = "";

try {
    let executable_names = [];
    for executable in platform.executables {
        let name = executable.name;
        executable_names.push(name);
        let source = std::path::join(binary_directory, name).display;
        if os_name == "linux" &&
                (executable.role == "gui" ||
                 executable.role == "control-center-gui") {
            copy_required(
                source,
                std::path::join(staging, "." + name + ".bin").display,
                "package_client_binary_missing"
            );
            copy_required(
                std::path::join(
                    repo,
                    "scripts/linux/agenterm-gui-wrapper.sh"
                ).display,
                std::path::join(staging, name).display,
                "package_client_linux_wrapper_missing"
            );
        } else {
            copy_required(
                source,
                std::path::join(staging, name).display,
                "package_client_binary_missing"
            );
        }
    }
    if os_name != "windows" && host_os != "windows" {
        let chmod_args = ["+x"];
        for name in executable_names {
            chmod_args.push(std::path::join(staging, name).display);
        }
        if os_name == "linux" {
            for executable in platform.executables {
                if executable.role == "gui" ||
                        executable.role == "control-center-gui" {
                    chmod_args.push(
                        std::path::join(
                            staging,
                            "." + executable.name + ".bin"
                        ).display
                    );
                }
            }
        }
        run("chmod", chmod_args, repo, "package_client_chmod");
    }
    if os_name == "linux" && host_os == "linux" {
        let adapter =
            std::path::join(repo, "scripts/stage-linux-gui-libs.sh").display;
        if std::fs::exists(adapter) {
            run(
                adapter,
                [staging, arch],
                repo,
                "package_client_linux_libraries"
            );
        }
    }

    for name in [
        "LICENSE-APACHE",
        "LICENSE-MIT",
        "THIRD_PARTY_NOTICES.md"
    ] {
        copy_required(
            std::path::join(repo, name).display,
            std::path::join(staging, name).display,
            "package_client_notice_missing"
        );
    }
    copy_required(
        artifact_manifest_path,
        std::path::join(staging, "artifacts.json").display,
        "package_client_manifest_missing"
    );
    let sbom_source = if std::env::has("AGENTERM_PACKAGE_SBOM") {
        std::path::absolute(std::env::get("AGENTERM_PACKAGE_SBOM")).display
    } else {
        std::path::join(repo, "dist/agenterm-sbom.spdx.json").display
    };
    if os_name == "macos" {
        copy_required(
            sbom_source,
            std::path::join(
                staging,
                "agenterm-sbom.spdx.json"
            ).display,
            "package_client_sbom_missing"
        );
    }
    if unsigned_preview {
        copy_required(
            std::path::join(repo, "docs/macos-unsigned-preview.md").display,
            std::path::join(
                staging,
                "MACOS_UNSIGNED_PREVIEW_README.md"
            ).display,
            "package_client_preview_readme"
        );
        copy_required(
            std::path::join(
                repo,
                "docs/macos-unsigned-preview.zh-Hant.md"
            ).display,
            std::path::join(
                staging,
                "MACOS_UNSIGNED_PREVIEW_README.zh-Hant.md"
            ).display,
            "package_client_preview_readme_zh"
        );
        std::fs::copy(
            std::path::join(repo, "docs/macos-unsigned-preview.md").display,
            std::path::join(
                dist,
                "MACOS_UNSIGNED_PREVIEW_README.md"
            ).display
        );
        std::fs::copy(
            std::path::join(
                repo,
                "docs/macos-unsigned-preview.zh-Hant.md"
            ).display,
            std::path::join(
                dist,
                "MACOS_UNSIGNED_PREVIEW_README.zh-Hant.md"
            ).display
        );
    }

    let suffix = if unsigned_preview { "-unsigned-preview" } else { "" };
    let archive_name =
        "agenterm-" + version + "-" + os_name + "-" + arch + suffix +
        if package_format == "zip" { ".zip" } else { ".tar.gz" };
    archive = std::path::join(dist, archive_name).display;
    if std::fs::exists(archive) {
        std::fs::remove_file(archive);
    }
    if package_format == "zip" {
        if host_os == "linux" {
            run(
                "zip",
                ["-q", "-r", archive, "."],
                staging,
                "package_client_zip"
            );
        } else {
            run(
                "tar",
                ["-a", "-c", "-f", archive, "-C", staging, "."],
                repo,
                "package_client_zip"
            );
        }
    } else {
        run(
            "tar",
            ["-czf", archive, "-C", staging, "."],
            repo,
            "package_client_tar"
        );
    }
    let archive_hash = rhai::crypto::sha256_file(archive);
    let package_manifest = #{
        schema_version: 1,
        product: "AgenTerm",
        version: version,
        os: os_name,
        arch: arch,
        channel: if unsigned_preview {
            "macos-unsigned-preview"
        } else {
            "release"
        },
        signed: signed,
        notarized: false,
        artifact: archive_name,
        sha256: archive_hash,
        source_commit: if std::env::has("GITHUB_SHA") {
            std::env::get("GITHUB_SHA")
        } else {
            ""
        },
        source_tag: if std::env::has("GITHUB_REF_NAME") {
            std::env::get("GITHUB_REF_NAME")
        } else {
            ""
        },
        artifact_manifest_sha256:
            rhai::crypto::sha256_file(artifact_manifest_path),
        cargo_lock_sha256: rhai::crypto::sha256_file(
            std::path::join(repo, "Cargo.lock").display
        ),
        sbom_sha256: if os_name == "macos" {
            rhai::crypto::sha256_file(sbom_source)
        } else {
            ""
        },
        execution_evidence: if host_os == os_name && host_arch == arch {
            "native_execution_eligible"
        } else {
            "cross_build_existence_only"
        },
        build_log:
            environment_or_empty("GITHUB_SERVER_URL") + "/" +
            environment_or_empty("GITHUB_REPOSITORY") + "/actions/runs/" +
            environment_or_empty("GITHUB_RUN_ID")
    };
    rhai::runtime::atomic_write(
        archive + ".provenance.json",
        rhai::json::stringify_pretty(package_manifest) + "\n"
    );
    rhai::runtime::atomic_write(
        archive + ".sha256",
        archive_hash + "  " + archive_name + "\n"
    );
} catch (error) {
    failure = error.to_string();
}
if std::fs::exists(scratch) {
    std::fs::remove_dir_all(scratch);
}
if failure != "" {
    throw failure;
}
print("PACKAGED " + archive);
