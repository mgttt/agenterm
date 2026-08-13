//! Size probe — variant A (static).
//!
//! Milestone 15 + 23: measure the order of magnitude of `S` (how much a
//! consumer sheds when its mechanism code moves from static linking to the
//! `libagenterm` dylib). Variant A statically links `agenterm-platform`
//! through its Rust API and performs the same probes as variant B.
//!
//! Milestone 15 covered runtime / clipboard / process / parent-console only.
//! Milestone 23 adds window_host (`run_pixel_window`, opened-then-exit) and
//! PTY (spawn the shortest-lived child and hand it to the detached reaper) —
//! the two biggest mechanisms in the real `agenterm-con` consumer — so the
//! measured `S_probe` approaches the true value (see README.md).

use std::process::ExitCode;
use std::time::Instant;

use agenterm_platform::clipboard::has_unicode_text;
use agenterm_platform::parent_console::write_stdout;
use agenterm_platform::process::list;
use agenterm_platform::pty::{shutdown_session_detached, ChildCommand, TerminalSize};
use agenterm_platform::runtime::{default_terminal_shell, user_config_directory};
use agenterm_platform::window_host::{
    run_pixel_window, LogicalSize, PixelWindow, PixelWindowApplication, PixelWindowDirective,
    PixelWindowError, PixelWindowEvent, PixelWindowOptions, XrgbPixelFrame,
};

/// Static linking has no ABI-version concept: a constant placeholder, per
/// the probe brief (variant B prints the real `agt_abi_version()`).
const ABI_VERSION_PLACEHOLDER: u32 = 0;

/// Window application that exits as soon as the native window reports
/// `opened()`, so `run_pixel_window` returns promptly on a real desktop and
/// fails fast (Unsupported/Failed) on headless hosts.
struct ExitOnOpenApplication;

impl PixelWindowApplication for ExitOnOpenApplication {
    fn opened(
        &mut self,
        _window: &PixelWindow,
    ) -> Result<PixelWindowDirective, PixelWindowError> {
        Ok(PixelWindowDirective::Exit)
    }

    fn event(
        &mut self,
        _window: &PixelWindow,
        _event: PixelWindowEvent,
    ) -> Result<PixelWindowDirective, PixelWindowError> {
        Ok(PixelWindowDirective::Exit)
    }

    fn render(
        &mut self,
        _window: &PixelWindow,
        _frame: &mut XrgbPixelFrame<'_>,
    ) -> Result<PixelWindowDirective, PixelWindowError> {
        Ok(PixelWindowDirective::Exit)
    }

    fn about_to_wait(
        &mut self,
        _window: &PixelWindow,
        _now: Instant,
    ) -> Result<PixelWindowDirective, PixelWindowError> {
        Ok(PixelWindowDirective::Wait)
    }
}

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
    let config_dir =
        user_config_directory().map_err(|e| format!("user_config_directory failed: {e}"))?;
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

    // 6. window: open-then-exit probe (milestone 23). Linking the code into
    //    the artifact is the point; Unsupported/Failed is acceptable on
    //    headless hosts and macOS, so a failure never fails the probe.
    let window_open = match run_pixel_window(
        PixelWindowOptions::new("size-probe", LogicalSize::new(320.0, 200.0))
            .with_no_activate(true),
        Box::new(ExitOnOpenApplication),
    ) {
        Ok(()) => "ok",
        Err(PixelWindowError::Unsupported { .. }) => "unsupported",
        Err(PixelWindowError::Failed { code, .. }) => {
            println!("window_open_failed_code={code}");
            "failed"
        }
        Err(_) => "failed",
    };

    // 7. pty: spawn the shortest-lived child and hand the whole session to
    //    the detached reaper (milestone 23). Failure is acceptable too; the
    //    code must merely be referenced so the mechanism links in.
    let (program, args): (&str, &[&str]) = if cfg!(windows) {
        ("cmd.exe", &["/c", "exit"])
    } else {
        ("/bin/sh", &["-c", "exit"])
    };
    let mut command = ChildCommand::new(program);
    for arg in args {
        command = command.arg(arg);
    }
    let pty_open = match command
        .size(TerminalSize { rows: 24, cols: 80 })
        .spawn()
    {
        Ok(spawned) => {
            let (master, child) = spawned.into_parts();
            match shutdown_session_detached(Some(master), Some(child)) {
                Ok(()) => "ok",
                Err(e) => {
                    println!("pty_shutdown_failed={e}");
                    "failed(shutdown)"
                }
            }
        }
        Err(e) => {
            println!("pty_spawn_failed={e}");
            "failed(spawn)"
        }
    };

    println!("user_config_dir_len={config_dir_len}");
    println!("default_shell_len={shell_len}");
    println!("clipboard_has_text={clipboard_has_text}");
    println!("process_count={process_count}");
    println!("parent_console_write_stdout={parent_console}");
    println!("window_open={window_open}");
    println!("pty_open={pty_open}");
    println!("abi_version={ABI_VERSION_PLACEHOLDER}(static placeholder)");
    Ok(())
}
