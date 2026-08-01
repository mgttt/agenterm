// Keep the PRD modules, shipped evidence contract, and public catalogs aligned.
//
// Args:
//   REPO [CLI_EXE MUX_EXE]

fn require(condition, message) {
    if !condition {
        throw message;
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

fn compare_exact(expected, actual, label) {
    require(
        expected.len == actual.len,
        "prd_alignment_list_count:" + label +
            ":expected=" + expected.len + ":actual=" + actual.len
    );
    for index in 0..expected.len {
        require(
            expected[index] == actual[index],
            "prd_alignment_list_mismatch:" + label +
                ":index=" + index +
                ":expected=" + expected[index] +
                ":actual=" + actual[index]
        );
    }
}

fn valid_id(value) {
    if value == "" || value.starts_with(".") || value.starts_with("-")
            || value.ends_with(".") || value.ends_with("-") {
        return false;
    }
    let separator = false;
    for character in value {
        let text = character.to_string();
        let valid =
            "abcdefghijklmnopqrstuvwxyz0123456789.-".contains(text);
        if !valid || ((text == "." || text == "-") && separator) {
            return false;
        }
        separator = text == "." || text == "-";
    }
    true
}

fn text_or_empty(value) {
    let text = value.to_string();
    if text == "()" {
        ""
    } else {
        text
    }
}

fn command_output(program, arguments, cwd, code) {
    let command = std::process::command(program);
    command.args(arguments);
    command.current_dir(cwd);
    command.timeout(std::time::Duration::from_secs(10));
    let output = command.output();
    require(output.success, output.error(code));
    let text = output.stdout_text();
    text.trim();
    text
}

fn command_json(program, arguments, cwd, code) {
    rhai::json::parse(command_output(program, arguments, cwd, code))
}

fn linked_details(index) {
    let result = [];
    let first = true;
    for part in index.split("(prd/") {
        if first {
            first = false;
            continue;
        }
        let closing = part.split(")");
        require(closing.len >= 2, "prd_alignment_link_unclosed");
        let name = closing[0];
        if name.starts_with("PRD_") && name.ends_with(".md") {
            result.push(name);
        }
    }
    result
}

fn status_document_line_present(prd, declaration, status_marker) {
    let start = 0;
    while start <= prd.len - declaration.len {
        let position = prd.index_of(declaration, start);
        if position < 0 {
            return false;
        }
        let line_start = position;
        while line_start > 0 && prd.get(line_start - 1) != '\n' {
            line_start -= 1;
        }
        let prefix = prd.sub_string(line_start, position - line_start);
        prefix.trim();
        if prefix.starts_with(status_marker) {
            return true;
        }
        start = position + declaration.len;
    }
    false
}

fn status_line_present(document_paths, declaration, status_marker) {
    declaration = declaration.to_lower();
    for path in document_paths {
        let document = std::fs::read_to_string(path).to_lower();
        if status_document_line_present(document, declaration, status_marker) {
            return true;
        }
    }
    false
}

fn product_name_character(character) {
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-"
        .contains(character.to_string())
}

fn product_document_mentions(product, name) {
    let start = 0;
    while start <= product.len - name.len {
        let position = product.index_of(name, start);
        if position < 0 {
            return false;
        }
        let before_ok =
            position == 0 || !product_name_character(product.get(position - 1));
        let after = position + name.len;
        let after_ok =
            after == product.len || !product_name_character(product.get(after));
        if before_ok && after_ok {
            return true;
        }
        start = position + name.len;
    }
    false
}

fn product_mentions(document_paths, name) {
    for path in document_paths {
        let document = std::fs::read_to_string(path).to_lower();
        if product_document_mentions(document, name) {
            return true;
        }
    }
    false
}

fn nonempty_lines(source) {
    let lines = [];
    for source_line in source.split("\n") {
        let line = source_line;
        line.trim();
        if line != "" {
            lines.push(line);
        }
    }
    lines
}

fn resolve_executable(path) {
    let candidate = std::path::absolute(path).display;
    if std::fs::exists(candidate) {
        return candidate;
    }
    let windows_candidate = candidate + ".exe";
    if std::fs::exists(windows_candidate) {
        return windows_candidate;
    }
    candidate
}

require(args.len == 1 || args.len == 3, "expected: REPO [CLI_EXE MUX_EXE]");
let repo = std::path::absolute(args[0]).display;
let cli = if args.len == 3 {
    resolve_executable(args[1])
} else {
    resolve_executable(std::path::join(repo, "dist/agenterm-cli").display)
};
let mux = if args.len == 3 {
    resolve_executable(args[2])
} else {
    resolve_executable(std::path::join(repo, "dist/agenterm-mux").display)
};
let prd_path = std::path::join(repo, "PRD.md").display;
let detail_directory = std::path::join(repo, "prd").display;
let contract_path =
    std::path::join(detail_directory, "alignment-contract.json").display;
for path in [repo, cli, mux, prd_path, detail_directory, contract_path] {
    require(std::fs::exists(path), "prd_alignment_input_missing");
}

let prd_index = std::fs::read_to_string(prd_path);
let linked = linked_details(prd_index);
let actual = [];
for entry in std::fs::read_dir(detail_directory) {
    if entry.is_file && entry.file_name.starts_with("PRD_")
            && entry.file_name.ends_with(".md") {
        actual.push(entry.file_name);
    }
}
compare_exact(sorted(actual), sorted(linked), "prd_modules");

// Keep paths, not lowercase copies of the entire PRD tree. Rhai's string
// budget is deliberately bounded, and retaining every document plus its
// lowercase copy made the audit depend on allocator/platform details as the
// product specification grew. Consumers below stream one document at a time.
let prd_document_paths = [prd_path];
for name in linked {
    prd_document_paths.push(std::path::join(detail_directory, name).display);
}
let contract = rhai::json::parse_file(contract_path);
require(contract.schema_version == 3, "prd_alignment_contract_schema");
let capabilities = contract.capabilities;
require(capabilities.len > 0, "prd_alignment_capabilities_empty");

let capability_ids = #{};
let allowed_kinds = #{
    architecture: true,
    behavior: true,
    decision: true,
    visual: true
};
let allowed_modes = #{
    "black-box": true,
    "black-box-partial": true,
    decision: true,
    "unit-source-partial": true
};
for capability in capabilities {
    require(valid_id(capability.id), "prd_alignment_capability_id");
    require(
        !capability_ids.contains(capability.id),
        "prd_alignment_capability_duplicate:" + capability.id
    );
    capability_ids[capability.id] = true;
    require(
        allowed_kinds.contains(capability.kind),
        "prd_alignment_capability_kind:" + capability.id
    );
    require(
        allowed_modes.contains(capability.evidence_mode),
        "prd_alignment_evidence_mode:" + capability.id
    );
    if capability.kind == "decision" {
        require(
            capability.status == "accepted"
                && capability.evidence_mode == "decision"
                && capability.evidence_ids.len == 0,
            "prd_alignment_decision_contract:" + capability.id
        );
    } else if capability.status == "shipped" {
        require(
            capability.evidence_mode == "black-box"
                && capability.evidence_ids.len > 0,
            "prd_alignment_shipped_contract:" + capability.id
        );
    } else {
        require(
            capability.status == "partial"
                && (
                    capability.evidence_mode == "black-box-partial"
                        || capability.evidence_mode == "unit-source-partial"
                )
                && capability.evidence_ids.len > 0,
            "prd_alignment_partial_contract:" + capability.id
        );
    }
    let status_marker = if capability.status == "partial" {
        "- [~]"
    } else {
        "- [x]"
    };
    require(
        status_line_present(prd_document_paths, capability.prd, status_marker),
        "prd_alignment_status_line:" + capability.id
    );
}

let gate_manifest = rhai::json::parse_file(
    std::path::join(repo, "scripts/qualification-gates.json").display
);
let registered_evidence = #{};
let evidence_suites = #{
    "cli-smoke": true,
    "server-smoke": true,
    "native-ipc-smoke": true,
    "native-ipc-compat-smoke": true,
    "remote-ui-smoke": true,
    "remote-ui-upgrade-smoke": true,
    "fleet-smoke": true,
    "script-smoke": true,
    "startup-smoke": true,
    "theme-smoke": true,
    "wake-smoke": true,
    "working-context-smoke": true,
    "workbench-smoke": true
};
for gate in gate_manifest.required_gates {
    if !evidence_suites.contains(gate.id) {
        continue;
    }
    let rhai_name = gate.id + ".rhai";
    let rhai_path = std::path::join(
        std::path::join(repo, "scripts/rhai").display,
        rhai_name
    ).display;
    require(
        std::fs::exists(rhai_path),
        "prd_alignment_evidence_suite_missing:" + gate.id
    );
    let suite_source = std::fs::read_to_string(rhai_path);
    for evidence_id in gate.evidence {
        require(
            valid_id(evidence_id),
            "prd_alignment_evidence_id:" + evidence_id
        );
        require(
            !registered_evidence.contains(evidence_id),
            "prd_alignment_evidence_duplicate:" + evidence_id
        );
        require(
            suite_source.contains("EVIDENCE " + evidence_id)
                || suite_source.contains("Write-Evidence '" + evidence_id + "'")
                || suite_source.contains("'" + evidence_id + "'")
                || suite_source.contains("\"" + evidence_id + "\""),
            "prd_alignment_evidence_not_emitted:" + evidence_id
        );
        registered_evidence[evidence_id] = true;
    }
}

let expected_evidence = [];
for capability in capabilities {
    if capability.kind != "decision" {
        for evidence_id in capability.evidence_ids {
            expected_evidence.push(evidence_id);
        }
    }
}
compare_exact(
    sorted(expected_evidence),
    sorted(registered_evidence.keys()),
    "capability_evidence"
);

let catalog_lines = nonempty_lines(
    command_output(cli, ["list-commands"], repo, "prd_alignment_cli_catalog")
);
let public_names = #{};
for line in catalog_lines {
    let pieces = line.split(" (");
    require(pieces.len <= 2, "prd_alignment_catalog_line:" + line);
    let name = pieces[0];
    require(valid_id(name), "prd_alignment_command_name:" + name);
    public_names[name] = true;
    if pieces.len == 2 {
        let aliases = pieces[1];
        require(aliases.ends_with(")"), "prd_alignment_catalog_aliases");
        aliases = aliases.sub_string(0, aliases.len - 1);
        for source_alias in aliases.split(",") {
            let alias = source_alias;
            alias.trim();
            require(valid_id(alias), "prd_alignment_command_alias:" + alias);
            public_names[alias] = true;
        }
    }
}
for name in public_names.keys() {
    require(
        product_mentions(prd_document_paths, name),
        "prd_alignment_public_command_missing:" + name
    );
}
for root_name in contract.planned_command_roots {
    require(
        !public_names.contains(root_name),
        "prd_alignment_planned_command_public:" + root_name
    );
}

let protocol = command_json(
    cli,
    ["protocol-info"],
    repo,
    "prd_alignment_protocol"
);
require(
    protocol.command_catalog.schema_version == 1,
    "prd_alignment_command_catalog_schema"
);
let discovered_catalog = [];
for command in protocol.command_catalog.commands {
    let line = command.id;
    if command.aliases.len > 0 {
        let aliases = "";
        for index in 0..command.aliases.len {
            if index > 0 {
                aliases += ", ";
            }
            aliases += command.aliases[index];
        }
        line += " (" + aliases + ")";
    }
    discovered_catalog.push(line);
}
compare_exact(catalog_lines, discovered_catalog, "protocol_command_catalog");

let protocol_capabilities = #{};
for capability in capabilities {
    let feature = text_or_empty(capability.protocol_feature);
    if feature != "" {
        require(
            !protocol_capabilities.contains(feature),
            "prd_alignment_protocol_feature_duplicate:" + feature
        );
        protocol_capabilities[feature] = capability.id;
    }
}
let runtime_features = sorted(protocol.features.keys());
compare_exact(
    sorted(protocol_capabilities.keys()),
    runtime_features,
    "protocol_features"
);
for feature in runtime_features {
    require(
        protocol.features[feature] == true,
        "prd_alignment_protocol_feature_false:" + feature
    );
}
for extension in protocol.extensions {
    require(
        public_names.contains(extension),
        "prd_alignment_extension_missing:" + extension
    );
}
for command in protocol.compatibility.tmux_rmux {
    require(
        public_names.contains(command),
        "prd_alignment_compatibility_missing:" + command
    );
}
for command in protocol.compatibility.partial {
    require(
        public_names.contains(command),
        "prd_alignment_compatibility_missing:" + command
    );
}

let mux_compatibility = command_json(
    mux,
    ["compatibility", "--json"],
    repo,
    "prd_alignment_mux_compatibility"
);
let mux_lines = nonempty_lines(
    command_output(mux, ["list-commands"], repo, "prd_alignment_mux_catalog")
);
let supported_mux = [];
let unsupported_mux = [];
for line in mux_lines {
    if line.contains(" (unsupported:") {
        unsupported_mux.push(line.split(" ")[0]);
    } else {
        supported_mux.push(line);
    }
}
compare_exact(
    mux_compatibility.supported,
    supported_mux,
    "mux_supported"
);
let expected_unsupported = [];
for command in mux_compatibility.explicitly_unsupported {
    expected_unsupported.push(command.name);
}
compare_exact(expected_unsupported, unsupported_mux, "mux_unsupported");
for name in supported_mux + unsupported_mux {
    require(
        product_mentions(prd_document_paths, name),
        "prd_alignment_mux_command_missing:" + name
    );
}

print(
    "PASS: PRD aligns with " + catalog_lines.len +
    " catalog entries, " + public_names.keys().len + " public names, " +
    runtime_features.len + " protocol features, " + mux_lines.len +
    " mux commands, " + capability_ids.keys().len + " capability IDs, and " +
    registered_evidence.keys().len + " executable evidence IDs"
);
