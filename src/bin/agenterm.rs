#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    let mut args = std::env::args().skip(1).collect::<Vec<_>>();
    if let Some(index) = args.iter().position(|argument| argument == "--server") {
        args.remove(index);
        std::process::exit(agenterm::run_server_entry_with_args(args));
    }
    std::process::exit(agenterm::run_gui_entry());
}
