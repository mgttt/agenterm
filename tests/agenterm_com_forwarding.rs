#![cfg(windows)]

use std::{fs, path::PathBuf, process::Command};

fn launcher() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_agenterm-com"))
}

#[test]
fn launcher_is_a_console_subsystem_pe() {
    let bytes = fs::read(launcher()).unwrap();
    assert_eq!(&bytes[..2], b"MZ");
    let pe_offset = u32::from_le_bytes(bytes[0x3c..0x40].try_into().unwrap()) as usize;
    assert_eq!(&bytes[pe_offset..pe_offset + 4], b"PE\0\0");
    let optional_header = pe_offset + 24;
    let subsystem = u16::from_le_bytes(
        bytes[optional_header + 68..optional_header + 70]
            .try_into()
            .unwrap(),
    );
    assert_eq!(subsystem, 3, "agenterm.com must be Windows CUI");
}

#[test]
fn launcher_forwards_stdout_and_success() {
    let output = Command::new(launcher())
        .args(["cli", "--version"])
        .output()
        .expect("run CLI through CUI launcher");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("agenterm cli {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn launcher_forwards_stderr_and_exit_code() {
    let output = Command::new(launcher())
        .args(["cli", "--deadline-ms", "0", "list-windows"])
        .output()
        .expect("run failing CLI through CUI launcher");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("must be from 1 to 60000"));
}

#[test]
fn launcher_forwards_all_unified_script_runtime_subcommands() {
    for (subcommand, prefix) in [
        ("rh", "agenterm-rh "),
        ("qjs", "agenterm-qjs "),
        ("lua", "agenterm-lua "),
        ("sql", "agenterm-sql "),
    ] {
        let output = Command::new(launcher())
            .args([subcommand, "version"])
            .output()
            .unwrap_or_else(|error| panic!("run {subcommand} through launcher: {error}"));
        assert!(output.status.success(), "{subcommand} failed");
        assert!(
            String::from_utf8_lossy(&output.stdout).starts_with(prefix),
            "unexpected {subcommand} version output"
        );
        assert!(output.stderr.is_empty(), "{subcommand} wrote stderr");
    }
}
