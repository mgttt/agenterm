#![cfg(windows)]

use std::{path::PathBuf, process::Command};

fn launcher_executable() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_agenterm-com"))
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
