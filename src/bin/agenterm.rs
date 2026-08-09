#![cfg_attr(windows, windows_subsystem = "windows")]

const INTERNAL_CLI_SUBCOMMAND: &str = "__agenterm-internal-cli";
const INTERNAL_TUI_SUBCOMMAND: &str = "__agenterm-internal-tui";

fn main() {
    let mut args = std::env::args().skip(1).collect::<Vec<_>>();

    // The public GUI-subsystem process attaches to the caller's console, then
    // starts the same PE with explicitly duplicated stdio handles. The child
    // sees valid handles from process startup, before Rust initializes its
    // cached stdio objects, and therefore reuses the ordinary CLI entry.
    if args.first().map(String::as_str) == Some("cli") {
        args.remove(0);
        std::process::exit(run_cli_from_gui_subsystem(args));
    }
    if args.first().map(String::as_str) == Some("tui") {
        args.remove(0);
        std::process::exit(run_tui_from_gui_subsystem(args));
    }
    if args.first().map(String::as_str) == Some(INTERNAL_CLI_SUBCOMMAND) {
        args.remove(0);
        // The worker's std slots hold explicit duplicated handles, but a
        // console handle is only writable from a process with a console
        // connection (ConDrv). Attach to the forwarder's console — same one
        // as the caller's — keeping default Ctrl+C termination; the explicit
        // handles survive the attach untouched. Pure-pipe callers have no
        // console and the attach is a harmless no-op failure.
        let _console =
            agenterm_platform::process::ScopedConsole::attach_parent_with_default_interrupts();
        std::process::exit(agenterm::run_cli_entry_with_args(args));
    }
    if args.first().map(String::as_str) == Some(INTERNAL_TUI_SUBCOMMAND) {
        args.remove(0);
        let _console =
            agenterm_platform::process::ScopedConsole::attach_parent_with_default_interrupts();
        std::process::exit(agenterm::tui::run_entry_with_args(args));
    }

    // G3: GUI PE must still print version/help when launched from a terminal.
    if let Some(code) = offline_cli_exit(&args) {
        std::process::exit(code);
    }

    let server_mode = match args.first().map(String::as_str) {
        Some("server") => {
            args.remove(0);
            true
        }
        // Transitional alias from the short-lived `--server` flag leaf.
        _ => {
            if let Some(index) = args.iter().position(|argument| argument == "--server") {
                args.remove(index);
                true
            } else {
                false
            }
        }
    };
    if server_mode {
        std::process::exit(agenterm::run_server_entry_with_args(args));
    }
    std::process::exit(agenterm::run_gui_entry());
}

#[cfg(windows)]
fn run_cli_from_gui_subsystem(args: Vec<String>) -> i32 {
    run_console_worker(INTERNAL_CLI_SUBCOMMAND, "CLI", args)
}

#[cfg(windows)]
fn run_tui_from_gui_subsystem(args: Vec<String>) -> i32 {
    run_console_worker(INTERNAL_TUI_SUBCOMMAND, "TUI", args)
}

#[cfg(windows)]
fn run_console_worker(internal_subcommand: &str, worker_name: &str, args: Vec<String>) -> i32 {
    use std::os::windows::io::OwnedHandle;
    use std::process::{Command, Stdio};

    // Attach to the caller's console (when launched from a terminal) so the
    // console-backed std slots become valid, then duplicate the real
    // stdin/stdout/stderr — console, pipe, or file — and wire the duplicates
    // into the child explicitly. `Stdio::inherit` or `println!` would trust
    // std state cached before the attach; only duplicated handles are safe,
    // and they stay valid however long the child outlives the attachment.
    let _console = agenterm_platform::process::ScopedConsole::attach_parent();
    let executable = match std::env::current_exe() {
        Ok(executable) => executable,
        Err(error) => {
            let _ = agenterm_platform::process::write_parent_console_stderr(&format!(
                "agenterm cli: could not resolve agenterm.exe: {error}"
            ));
            return 1;
        }
    };
    let [stdin_handle, stdout_handle, stderr_handle] =
        agenterm_platform::process::duplicated_std_handles();
    fn stdio(handle: Option<OwnedHandle>) -> Stdio {
        handle.map_or_else(Stdio::null, Stdio::from)
    }
    match Command::new(executable)
        .arg(internal_subcommand)
        .args(args)
        .stdin(stdio(stdin_handle))
        .stdout(stdio(stdout_handle))
        .stderr(stdio(stderr_handle))
        .status()
    {
        Ok(status) => status.code().unwrap_or(1),
        Err(error) => {
            let _ = agenterm_platform::process::write_parent_console_stderr(&format!(
                "agenterm: could not start the {worker_name} worker: {error}"
            ));
            1
        }
    }
}

#[cfg(not(windows))]
fn run_cli_from_gui_subsystem(args: Vec<String>) -> i32 {
    agenterm::run_cli_entry_with_args(args)
}

#[cfg(not(windows))]
fn run_tui_from_gui_subsystem(args: Vec<String>) -> i32 {
    agenterm::tui::run_entry_with_args(args)
}

/// Flags that must not open a window. Returns Some(exit_code) when handled.
fn offline_cli_exit(args: &[String]) -> Option<i32> {
    let alone = args.len() == 1;
    match args.first().map(String::as_str) {
        Some("--version" | "-V") if alone => {
            let line = format!("agenterm {}", env!("CARGO_PKG_VERSION"));
            let _ = agenterm_platform::process::write_parent_console_stdout(&line);
            Some(0)
        }
        Some("--help" | "-h") if alone => {
            let line = "\
Usage: agenterm [--no-activate] [--endpoint ENDPOINT | --address HOST:PORT | --instance NAME]
       agenterm server [server options]
       agenterm cli <command> [options]
       agenterm tui
       agenterm --version
       agenterm --help

GUI launcher. Use `agenterm cli` for control-plane commands (list-windows, mux, mcp, …),
or `agenterm tui` for the interactive terminal interface.
On Windows, the GUI-subsystem executable attaches to the parent console and runs this command through the same PE.";
            let _ = agenterm_platform::process::write_parent_console_stdout(line);
            Some(0)
        }
        Some("--version" | "-V" | "--help" | "-h") => {
            let _ = agenterm_platform::process::write_parent_console_stderr(
                "error: --version/--help must be used alone",
            );
            Some(2)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::offline_cli_exit;

    #[test]
    fn offline_version_and_help_are_solo_flags() {
        assert_eq!(offline_cli_exit(&["--version".to_owned()]), Some(0));
        assert_eq!(offline_cli_exit(&["-V".to_owned()]), Some(0));
        assert_eq!(offline_cli_exit(&["--help".to_owned()]), Some(0));
        assert_eq!(
            offline_cli_exit(&["--version".to_owned(), "extra".to_owned()]),
            Some(2)
        );
        assert_eq!(offline_cli_exit(&["server".to_owned()]), None);
    }
}
