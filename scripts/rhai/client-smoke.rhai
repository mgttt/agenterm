// Probe the public entry points declared for one platform.
//
// Args: REPO OS ARCH BINARY_DIRECTORY [--existence-only]

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

require(
    args.len == 4 || (args.len == 5 && args[4] == "--existence-only"),
    "expected: REPO OS ARCH BINARY_DIRECTORY [--existence-only]"
);
let repo = std::path::absolute(args[0]).display;
let os_name = args[1];
let arch = args[2];
let binary_directory = std::path::absolute(args[3]).display;
let existence_only = args.len == 5;
let manifest = rhai::json::parse_file(
    std::path::join(repo, "scripts/artifacts.json").display
);
let matches = [];
for platform in manifest.platforms {
    if platform.os == os_name && platform.arch == arch {
        matches.push(platform);
    }
}
require(matches.len == 1, "client_smoke_platform_match");
for executable in matches[0].executables {
    let path = std::path::join(binary_directory, executable.name).display;
    require(std::fs::exists(path), "client_smoke_missing:" + executable.name);
    if !existence_only {
        let probes = [];
        if executable.role == "control-center-gui" {
            probes.push(["--help"]);
            probes.push(["--version"]);
            probes.push(["capabilities", "--json"]);
        } else if executable.offline_probe.len > 0 {
            probes.push(executable.offline_probe);
        }
        for probe in probes {
            let command = std::process::command(path);
            command.args(probe);
            command.current_dir(repo);
            command.timeout(std::time::Duration::from_secs(10));
            let output = command.output();
            require(
                output.success,
                "client_smoke_probe:" + executable.name + ":" +
                    output.stderr_text()
            );
            if executable.role == "control-center-gui"
                    && probe.contains("capabilities") {
                let capability = rhai::json::parse(
                    output.stdout_text() + output.stderr_text()
                );
                let expected_webview_backend = "";
                if os_name == "windows" {
                    expected_webview_backend = "webview2";
                } else if os_name == "macos" {
                    expected_webview_backend = "wkwebview";
                } else if os_name == "linux" {
                    expected_webview_backend = "webkitgtk";
                }
                require(
                    expected_webview_backend.len > 0,
                    "client_smoke_webview_platform:" + os_name
                );
                let runtime_presence =
                    capability.webview_host.runtime_presence;
                require(
                    capability.schema_version == 1
                        && capability.executable == "agenterm-cc"
                        && capability.role == "isolated_projection"
                        && capability.owns_terminal_authority == false
                        && capability.process_reuse
                        && capability.no_activate
                        && capability.views.len == 4,
                    "client_smoke_control_center_capabilities"
                );
                require(
                    capability.renderer == "native"
                        && capability.webview_host.backend
                            == expected_webview_backend
                        && capability.webview_host.host_state
                            == "unimplemented"
                        && capability.webview_host.active_renderer == "native"
                        && capability.webview_host.bridge_version == 1
                        && (
                            runtime_presence == "detected"
                                || runtime_presence == "missing"
                                || runtime_presence == "failed"
                        ),
                    "client_smoke_webview_host_contract"
                );
            }
        }
    }
}
print("PASS client entrypoints " + os_name + "-" + arch);
