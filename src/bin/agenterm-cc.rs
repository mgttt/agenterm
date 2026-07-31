#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    std::process::exit(agenterm::run_control_center_entry_with_args(
        std::env::args_os().skip(1),
    ));
}
