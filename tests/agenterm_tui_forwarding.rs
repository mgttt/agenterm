#![cfg(windows)]

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

fn agenterm_executable() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_agenterm"))
}

fn launcher_executable() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_agenterm-com"))
}

fn con_executable() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_agenterm-con"))
}

fn scratch_dir() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("agenterm-tui-{nonce}"))
}

fn snapshot_contains(path: &Path, needle: &str) -> bool {
    fs::read_to_string(path)
        .ok()
        .is_some_and(|text| text.contains(needle))
}

#[test]
fn tui_help_is_available_through_the_gui_executable() {
    let output = Command::new(launcher_executable())
        .args(["tui", "--help"])
        .output()
        .expect("run agenterm.com tui --help");
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("Usage: agenterm tui"));
    assert!(output.stderr.is_empty());
}

#[test]
fn placeholder_tui_enters_screen_accepts_q_and_returns_to_cmd() {
    let dir = scratch_dir();
    fs::create_dir_all(&dir).unwrap();
    let script = dir.join("drive.json");
    let snapshot = dir.join("snapshot.json");
    fs::copy(agenterm_executable(), dir.join("agenterm.exe")).unwrap();
    fs::copy(launcher_executable(), dir.join("agenterm.com")).unwrap();
    fs::write(
        &script,
        serde_json::to_vec_pretty(&serde_json::json!([
            {"wait_ms": 300}, {"text": "agenterm tui\r"}, {"wait_ms": 1200},
            {"text": "q"}, {"wait_ms": 500}, {"text": "echo TUI_RETURNED\r"},
            {"wait_ms": 300}, {"text": "exit\r"}, {"wait_ms": 200}
        ]))
        .unwrap(),
    )
    .unwrap();

    let mut child = Command::new(con_executable())
        .args(["--no-activate", "--emit-snapshot"])
        .arg(&snapshot)
        .arg("--script")
        .arg(&script)
        .arg("--working-dir")
        .arg(&dir)
        .args(["-e", "cmd.exe", "/k"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start real ConPTY TUI journey");

    let deadline = Instant::now() + Duration::from_secs(10);
    let mut saw_tui = false;
    while Instant::now() < deadline {
        saw_tui |= snapshot_contains(&snapshot, "AGENTERM TUI");
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    if child.try_wait().unwrap().is_none() {
        let _ = child.kill();
        panic!("TUI ConPTY journey timed out");
    }
    assert!(saw_tui, "placeholder TUI was never rendered");
    assert!(snapshot_contains(&snapshot, "TUI_RETURNED"));
}
