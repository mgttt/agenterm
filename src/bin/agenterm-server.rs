#![cfg_attr(windows, windows_subsystem = "windows")]

/// Thin image-isolation alias for `agenterm --server`.
///
/// Windows keep-alive / replaceable-GUI upgrades need a PE path distinct from
/// `agenterm.exe`. Prefer documenting `agenterm --server`; keep shipping this
/// sibling so autostart does not map the live authority onto the GUI image.
fn main() {
    std::process::exit(agenterm::run_server_entry());
}
