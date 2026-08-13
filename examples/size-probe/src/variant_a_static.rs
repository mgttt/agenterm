//! Size probe — variant A (static).
//!
//! Milestone 15: measure the order of magnitude of `S` (how much a consumer
//! sheds when its mechanism code moves from static linking to the
//! `libagenterm` dylib). Variant A statically links `agenterm-platform`
//! through its Rust API and performs the same six probes as variant B.
//!
//! The probes deliberately cover only runtime / clipboard / process /
//! parent-console. Window host and PTY — the two biggest mechanisms in the
//! real `agenterm-con` consumer — are NOT covered (see README.md; this makes
//! `S_probe` a lower-bound estimate by design).

use std::process::ExitCode;

use agenterm_platform::clipboard::has_unicode_text;
use agenterm_platform::parent_console::write_stdout;
use agenterm_platform::process::list;
use agenterm_platform::runtime::{default_terminal_shell, user_config_directory};

/// Static linking has no ABI-version concept: a constant placeholder, per
/// the probe brief (variant B prints the real `agt_abi_version()`).
const ABI_VERSION_PLACEHOLDER: u32 = 0;

fn main() -> ExitCode {
    println!("size-probe variant A (static: agenterm-platform rlib)");
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("size-probe variant A: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    // 1. user config dir length (never print its content)
    let config_dir = user_config_directory()
        .map_err(|e| format!("user_config_directory failed: {e}"))?;
    let config_dir_len = config_dir.as_os_str().as_encoded_bytes().len();

    // 2. default shell length (never print its content)
    let shell_len = default_terminal_shell().len();

    // 3. clipboard: does it hold Unicode text? (bool only)
    let clipboard_has_text = has_unicode_text();

    // 4. process list entry count
    let processes = list().map_err(|e| format!("process::list failed: {e}"))?;
    let process_count = processes.len();

    // 5. write one short line to the parent console
    let parent_console = if write_stdout("size-probe[variant A] parent-console write ok") {
        "ok"
    } else {
        "unsupported"
    };

    println!("user_config_dir_len={config_dir_len}");
    println!("default_shell_len={shell_len}");
    println!("clipboard_has_text={clipboard_has_text}");
    println!("process_count={process_count}");
    println!("parent_console_write_stdout={parent_console}");
    println!("abi_version={ABI_VERSION_PLACEHOLDER}(static placeholder)");
    Ok(())
}
