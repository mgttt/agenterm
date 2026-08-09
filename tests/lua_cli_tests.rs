//! Whole-file gate: exercises the lua engine, which defaults off.
#![cfg(feature = "script-lua")]

//! Task CLI tests for the lua script engine, run through the main
//! `agenterm` PE's internal engine dispatch (the standalone `agenterm-lua`
//! binary is retired).

use std::process::Command;

fn lua_command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_agenterm"));
    command.args(["__agenterm-internal-engine", "lua"]);
    command
}

#[test]
fn task_list_lists_entries() {
    let manifest = std::env::current_dir()
        .expect("cwd")
        .join("agenterm.tasks.json");
    let output = lua_command()
        .args(["task", "list", "--manifest", &manifest.to_string_lossy()])
        .output()
        .expect("task list");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("lua-check"),
        "should list lua-check task, got: {stdout}"
    );
    assert!(stdout.contains(".lua"), "should contain .lua entry");
}

#[test]
fn task_run_nonexistent_fails() {
    let manifest = std::env::current_dir()
        .expect("cwd")
        .join("agenterm.tasks.json");
    let output = lua_command()
        .args([
            "task",
            "run",
            "nonexistent-task",
            "--manifest",
            &manifest.to_string_lossy(),
        ])
        .output()
        .expect("task run");
    assert!(!output.status.success());
}

#[test]
fn check_stage_build_lua() {
    let output = lua_command()
        .args(["check", "scripts/lua/stage-build.lua"])
        .output()
        .expect("check");
    assert!(output.status.success());
}

#[test]
fn check_build_identity_lua() {
    let output = lua_command()
        .args(["check", "scripts/lua/build_identity.lua"])
        .output()
        .expect("check");
    assert!(output.status.success());
}

#[test]
fn check_hello_lua() {
    let output = lua_command()
        .args(["check", "scripts/lua/hello.lua"])
        .output()
        .expect("check");
    assert!(output.status.success());
}

#[test]
fn check_timing_summary_lua() {
    let output = lua_command()
        .args(["check", "scripts/lua/timing-summary.lua"])
        .output()
        .expect("check");
    assert!(output.status.success());
}

#[test]
fn run_hello_returns_zero() {
    let output = lua_command()
        .args(["run", "scripts/lua/hello.lua"])
        .output()
        .expect("run hello");
    assert!(output.status.success());
}

#[test]
fn version_output() {
    let output = lua_command().arg("version").output().expect("version");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("agenterm-lua"));
    assert!(output.status.success());
}
