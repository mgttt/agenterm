#![cfg(windows)]

use std::{
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

fn agenterm_executable() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_agenterm"))
}

#[test]
fn explicit_gui_cli_forwards_stdout_to_a_waiting_parent() {
    let output = Command::new(agenterm_executable())
        .args(["cli", "--version"])
        .output()
        .expect("run explicit agenterm.exe CLI forwarding");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("agenterm cli {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn explicit_gui_cli_preserves_diagnostics_and_exit_code() {
    let output = Command::new(agenterm_executable())
        .args(["cli", "--deadline-ms", "0", "list-windows"])
        .output()
        .expect("run invalid agenterm.exe CLI command");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("agenterm cli --deadline-ms must be from 1 to 60000")
    );
}

#[test]
fn explicit_gui_cli_preserves_bidirectional_mcp_stdio() {
    let mut child = Command::new(agenterm_executable())
        .args(["cli", "mcp", "serve", "--stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn MCP through agenterm.exe cli");
    let mut stdin = child.stdin.take().expect("MCP stdin");
    stdin
        .write_all(
            concat!(
                "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-06-18\",\"capabilities\":{},\"clientInfo\":{\"name\":\"forwarding-test\",\"version\":\"1\"}}}\n",
                "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n",
                "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\",\"params\":{}}\n",
            )
            .as_bytes(),
        )
        .expect("write MCP requests");
    drop(stdin);

    let output = child.wait_with_output().expect("wait for MCP");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "MCP stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.lines().any(|line| line.contains("\"id\":1")));
    assert!(stdout.lines().any(|line| line.contains("\"id\":2")));
    assert!(output.stderr.is_empty());
}

#[test]
fn cmd_script_waits_for_explicit_agenterm_exe_and_preserves_pipeline_output() {
    let executable = agenterm_executable();
    let directory = executable.parent().expect("agenterm.exe directory");
    let output = Command::new("cmd.exe")
        .args(["/d", "/c", "agenterm.exe cli --version | findstr agenterm"])
        .current_dir(directory)
        .output()
        .expect("run agenterm.exe cli through cmd script");

    assert!(
        output.status.success(),
        "cmd stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("agenterm cli"));
}

#[test]
fn powershell_waits_for_explicit_agenterm_exe() {
    let executable = agenterm_executable();
    let directory = executable.parent().expect("agenterm.exe directory");
    let output = Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "& .\\agenterm.exe cli --version",
        ])
        .current_dir(directory)
        .output()
        .expect("run agenterm.exe cli through PowerShell");

    assert!(
        output.status.success(),
        "PowerShell stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("agenterm cli"));
}
