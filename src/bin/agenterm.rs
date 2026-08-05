#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    let mut args = std::env::args().skip(1).collect::<Vec<_>>();
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
