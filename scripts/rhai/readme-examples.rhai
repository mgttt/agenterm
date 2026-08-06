// Verify that README artifact roles and public command examples agree with
// offline machine-readable product catalogs.

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

fn normalize_whitespace(value) {
    let normalized = value;
    normalized.replace("\r", " ");
    normalized.replace("\n", " ");
    normalized.replace("\t", " ");
    while normalized.contains("  ") {
        normalized.replace("  ", " ");
    }
    normalized.trim();
    normalized
}

let repo = std::env::current_dir().display;
let artifact_root = if args.len > 0 {
    std::path::absolute(args[0]).display
} else {
    std::path::join(repo, "dist").display
};
let skip_probes = args.len > 1 && args[1] == "--skip-executable-probes";
let manifest = rhai::json::parse(std::fs::read_to_string(
    std::path::join(repo, "scripts/artifacts.json").display
));
require(manifest.schema_version == 2, "readme_artifact_manifest_schema");
let readme = std::fs::read_to_string(std::path::join(repo, "README.md").display);
let normalized_readme = normalize_whitespace(readme);

for artifact in manifest.executables {
    require(
        readme.contains(artifact.name),
        "README does not name manifest artifact '" + artifact.name + "'"
    );
    let role = normalize_whitespace(artifact.documentation_role);
    require(
        normalized_readme.contains(role),
        "README does not describe '" + artifact.name + "' as '" + role + "'"
    );
}

for example in [".\\build.bat", ".\\check.cmd", ".\\dist\\agenterm.exe"] {
    require(
        readme.contains(example),
        "README is missing required executable example '" + example + "'"
    );
}

if !skip_probes {
    for artifact in manifest.executables {
        if artifact.offline_probe.len == 0 {
            continue;
        }
        let executable = std::path::join(artifact_root, artifact.name).display;
        require(
            std::fs::exists(executable),
            "README probe artifact is missing: " + executable
        );
        let command = std::process::command(executable);
        command.args(artifact.offline_probe);
        command.env("AGENTERM_NO_ACTIVATE", "1");
        command.timeout(std::time::Duration::from_secs(10));
        let output = command.output();
        require(
            output.success,
            "Offline README probe failed for '" + artifact.name + "': " +
                output.error("readme-offline-probe")
        );
    }

    let cli = std::process::command(
        std::path::join(artifact_root, "agenterm-cli.exe").display
    );
    cli.arg("list-commands");
    cli.env("AGENTERM_NO_ACTIVATE", "1");
    cli.timeout(std::time::Duration::from_secs(10));
    let cli_output = cli.output();
    require(
        cli_output.success,
        cli_output.error("readme-cli-command-catalog")
    );
    let cli_names = #{};
    for source_line in cli_output.stdout_text().split("\n") {
        let line = source_line;
        line.trim();
        if line == "" {
            continue;
        }
        let fields = line.split(" ");
        require(fields.len > 0, "readme-cli-command-catalog-line");
        cli_names[fields[0]] = true;
    }

    for source_line in readme.split("\n") {
        let line = source_line;
        line.trim();
        if !line.starts_with("& $r ") {
            continue;
        }
        let fields = [];
        for source_field in line.split(" ") {
            let field = source_field;
            field.trim();
            if field != "" {
                fields.push(field);
            }
        }
        require(fields.len >= 3, "README contains an invalid agenterm-cli example");
        let command_index = if fields[2] == "--address" { 4 } else { 2 };
        require(
            fields.len > command_index,
            "README contains an incomplete agenterm-cli example"
        );
        let command_name = fields[command_index];
        command_name.replace("\"", "");
        require(
            cli_names.contains(command_name),
            "README references unknown agenterm-cli command '" + command_name + "'"
        );
    }

    let mux = std::process::command(
        std::path::join(artifact_root, "agenterm-cli.exe").display
    );
    mux.args(["mux", "compatibility", "--json"]);
    mux.env("AGENTERM_NO_ACTIVATE", "1");
    mux.timeout(std::time::Duration::from_secs(10));
    let mux_output = mux.output();
    require(
        mux_output.success,
        mux_output.error("readme-mux-compatibility")
    );
    let compatibility = rhai::json::parse(mux_output.stdout_text());
    require(
        compatibility.schema_version > 0,
        "README mux compatibility example returned an invalid catalog"
    );
}

print(
    "PASS: README names " + manifest.executables.len +
    " manifest artifacts and its command examples match the offline catalogs."
);
