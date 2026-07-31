#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    std::process::exit(agenterm::run_server_entry());
}
