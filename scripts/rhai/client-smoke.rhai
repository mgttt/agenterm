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
    if !existence_only && executable.offline_probe.len > 0 {
        let command = std::process::command(path);
        command.args(executable.offline_probe);
        command.current_dir(repo);
        command.timeout(std::time::Duration::from_secs(10));
        let output = command.output();
        require(
            output.success,
            "client_smoke_probe:" + executable.name + ":" +
                output.stderr_text()
        );
    }
}
print("PASS client entrypoints " + os_name + "-" + arch);
