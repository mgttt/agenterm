//! Task CLI tests for agenterm-lua.

use std::process::Command;

fn lua_binary() -> String {
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    format!("target/{profile}/agenterm-lua.exe")
}

#[test]
fn task_list_lists_entries() {
    let binary = lua_binary();
    let manifest = std::env::current_dir()
        .expect("cwd")
        .join("agenterm.tasks.json");
    let output = Command::new(&binary)
        .args(["task", "list", "--manifest", &manifest.to_string_lossy()])
        .output()
        .expect("task list");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("lua-check"), "should list lua-check task, got: {stdout}");
    assert!(stdout.contains(".lua"), "should contain .lua entry");
}

#[test]
fn task_run_nonexistent_fails() {
    let binary = lua_binary();
    let manifest = std::env::current_dir()
        .expect("cwd")
        .join("agenterm.tasks.json");
    let output = Command::new(&binary)
        .args(["task", "run", "nonexistent-task", "--manifest", &manifest.to_string_lossy()])
        .output()
        .expect("task run");
    assert!(!output.status.success());
}

#[test]
fn check_stage_build_lua() {
    let binary = lua_binary();
    let output = Command::new(&binary)
        .args(["check", "scripts/lua/stage-build.lua"])
        .output()
        .expect("check");
    assert!(output.status.success());
}

#[test]
fn check_build_identity_lua() {
    let binary = lua_binary();
    let output = Command::new(&binary)
        .args(["check", "scripts/lua/build_identity.lua"])
        .output()
        .expect("check");
    assert!(output.status.success());
}

#[test]
fn check_hello_lua() {
    let binary = lua_binary();
    let output = Command::new(&binary)
        .args(["check", "scripts/lua/hello.lua"])
        .output()
        .expect("check");
    assert!(output.status.success());
}

#[test]
fn check_timing_summary_lua() {
    let binary = lua_binary();
    let output = Command::new(&binary)
        .args(["check", "scripts/lua/timing-summary.lua"])
        .output()
        .expect("check");
    assert!(output.status.success());
}

#[test]
fn run_hello_returns_zero() {
    let binary = lua_binary();
    let output = Command::new(&binary)
        .args(["run", "scripts/lua/hello.lua"])
        .output()
        .expect("run hello");
    assert!(output.status.success());
}

#[test]
fn version_output() {
    let binary = lua_binary();
    let output = Command::new(&binary)
        .arg("version")
        .output()
        .expect("version");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("agenterm-lua"));
    assert!(output.status.success());
}
