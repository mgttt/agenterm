#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    std::process::exit(agenterm::run_server_entry());
}

#[cfg(not(windows))]
fn main() {
    eprintln!("agenterm-server is currently available on Windows only");
    std::process::exit(2);
}
