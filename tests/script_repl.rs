use std::{
    io::Write,
    process::{Command, Stdio},
};

fn run_repl(input: &str, arguments: &[&str]) -> std::process::Output {
    run_repl_with(env!("CARGO_BIN_EXE_agenterm-rhai"), input, arguments)
}

fn run_repl_with(executable: &str, input: &str, arguments: &[&str]) -> std::process::Output {
    let mut command = Command::new(executable);
    command
        .arg("repl")
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("spawn agenterm-rhai repl");
    child
        .stdin
        .take()
        .expect("REPL stdin")
        .write_all(input.as_bytes())
        .expect("write REPL input");
    child.wait_with_output().expect("wait for REPL")
}

fn generation_probe_input() -> String {
    let mut input = String::from("let first_generation = 40; std::process::id()\n");
    for _ in 2..=32 {
        input.push_str("std::process::id()\n");
    }
    input.push_str("std::process::id()\n:vars\n:quit\n");
    input
}

fn assert_bounded_persistent_generation(output: std::process::Output) {
    assert!(output.status.success(), "{output:?}");
    let records = String::from_utf8(output.stdout)
        .expect("UTF-8 stdout")
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("NDJSON record"))
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 35, "{records:#?}");

    let first_pid = records[0]["value"]["value"]
        .as_u64()
        .expect("first worker PID");
    for record in &records[..32] {
        assert_eq!(record["value"]["value"].as_u64(), Some(first_pid));
    }

    let receipt = &records[32];
    assert_eq!(receipt["kind"], "fresh_session");
    assert_eq!(receipt["reason"], "generation_limit");
    assert_eq!(receipt["old_worker_pid"].as_u64(), Some(first_pid));
    assert_eq!(receipt["old_generation"], 1);
    assert_eq!(receipt["new_generation"], 2);
    assert_eq!(receipt["language_state_fresh"], true);
    assert_eq!(receipt["history_fresh"], true);
    assert_eq!(receipt["side_effects_replayed"], false);

    let replacement_pid = records[33]["value"]["value"]
        .as_u64()
        .expect("replacement worker PID");
    assert_ne!(replacement_pid, first_pid);
    assert_eq!(receipt["new_worker_pid"].as_u64(), Some(replacement_pid));
    assert_eq!(records[34]["command"], "vars");
    assert!(
        records[34]["value"]
            .as_array()
            .is_some_and(|variables| variables.iter().all(|entry| {
                entry.as_array().is_none_or(|fields| {
                    fields.first() != Some(&serde_json::json!("first_generation"))
                })
            }))
    );
}

#[cfg(windows)]
#[test]
fn native_cli_compatibility_route_reuses_the_same_session_contract() {
    let mut arguments = vec!["script", "repl"];
    arguments.push("--json");
    let mut command = Command::new(env!("CARGO_BIN_EXE_agenterm-cli"));
    command
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("spawn agenterm-cli script repl");
    child
        .stdin
        .take()
        .expect("CLI REPL stdin")
        .write_all(b"let x = 40;\nx + 2\n:quit\n")
        .expect("write CLI REPL input");
    let output = child.wait_with_output().expect("wait for CLI REPL");
    assert!(output.status.success(), "{output:?}");
    let records = String::from_utf8(output.stdout)
        .expect("UTF-8 stdout")
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("NDJSON record"))
        .collect::<Vec<_>>();
    assert_eq!(records[1]["value"]["value"], 42);
}

#[cfg(windows)]
#[test]
fn native_cli_repl_exposes_bounded_persistent_worker_generations() {
    let mut command = Command::new(env!("CARGO_BIN_EXE_agenterm-cli"));
    command
        .args(["script", "repl", "--json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("spawn agenterm-cli script repl");
    child
        .stdin
        .take()
        .expect("CLI REPL stdin")
        .write_all(generation_probe_input().as_bytes())
        .expect("write CLI REPL generation probe");
    assert_bounded_persistent_generation(child.wait_with_output().expect("wait for CLI REPL"));
}

#[cfg(windows)]
#[test]
fn native_cli_compatibility_route_requires_the_adjacent_script_sidecar() {
    let unique = format!(
        "agenterm-cli-repl-sidecar-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock")
            .as_nanos()
    );
    let root = std::env::temp_dir().join(unique);
    std::fs::create_dir_all(&root).expect("create isolated CLI directory");
    let isolated_cli = root.join("agenterm-cli.exe");
    std::fs::copy(env!("CARGO_BIN_EXE_agenterm-cli"), &isolated_cli)
        .expect("copy isolated agenterm-cli");

    let output = Command::new(&isolated_cli)
        .args(["script", "repl", "--json"])
        .output()
        .expect("run isolated agenterm-cli");
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("host_worker_missing"),
        "{output:?}"
    );

    std::fs::remove_file(&isolated_cli).expect("remove isolated agenterm-cli");
    std::fs::remove_dir(&root).expect("remove isolated CLI directory");
}

#[test]
fn piped_json_repl_persists_variables_functions_and_multiline_cells() {
    let output = run_repl(
        "let x = 40;\nfn add(n) {\n    n + 2\n}\nadd(x)\n:vars\n:functions\n:quit\n",
        &["--json"],
    );
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 stdout");
    assert!(!stdout.contains("rhai>"));
    let records = stdout
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("NDJSON record"))
        .collect::<Vec<_>>();
    assert_eq!(records[2]["value"]["value"], 42);
    assert_eq!(records[3]["kind"], "meta");
    assert_eq!(records[3]["command"], "vars");
    assert!(records[3]["value"].as_array().is_some_and(|variables| {
        variables
            .iter()
            .any(|entry| entry.as_array().is_some_and(|entry| entry[0] == "x"))
    }));
    assert_eq!(records[4]["command"], "functions");
    assert!(
        records[4]["value"]
            .as_array()
            .is_some_and(|functions| functions.iter().any(|entry| entry == "add(n)"))
    );
}

#[test]
fn public_repl_exposes_bounded_persistent_worker_generations() {
    assert_bounded_persistent_generation(run_repl(&generation_probe_input(), &["--json"]));
}

#[test]
fn repl_recovers_after_runtime_failure_and_reset_removes_state() {
    let output = run_repl(
        "let x = 1;\nx = 9; throw \"rollback\";\nx\n:reset\nx\n40 + 2\n:quit\n",
        &["--json"],
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 stdout");
    let records = stdout
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("NDJSON record"))
        .collect::<Vec<_>>();
    assert_eq!(records[1]["ok"], false);
    assert_eq!(records[1]["state_committed"], false);
    assert_eq!(records[2]["value"]["value"], 1);
    assert_eq!(records[3]["command"], "reset");
    assert_eq!(records[4]["ok"], false);
    assert_eq!(records[5]["value"]["value"], 42);
}

#[test]
fn fail_fast_stops_after_first_bad_cell() {
    let output = run_repl("throw \"stop\";\n40 + 2\n", &["--json", "--fail-fast"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 stdout");
    assert_eq!(stdout.lines().count(), 1);
}

#[test]
fn eof_reports_an_incomplete_cell_without_a_prompt() {
    let output = run_repl("fn unfinished() {\n", &["--json"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 stdout");
    assert!(!stdout.contains("rhai>"));
    let result: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("typed incomplete result");
    assert_eq!(result["failure"]["code"], "script_incomplete");
}
