#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    let mut args = std::env::args().skip(1).collect::<Vec<_>>();

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
       agenterm --version
       agenterm --help

GUI launcher. For CLI commands use agenterm-cli.";
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
