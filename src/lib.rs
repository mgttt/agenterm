#![cfg_attr(not(windows), allow(dead_code))]

use std::time::Duration;

mod build_identity;
mod client;
mod commands;
#[cfg(any(windows, unix))]
mod control_authority;
mod control_contract;
#[cfg(any(windows, unix))]
mod control_dispatch;
mod event_journal;
mod instances;
mod ipc_transport;
mod locale;
pub mod mcp_catalog;
mod mcp_fleet;
pub mod mcp_stdio;
pub mod operations;
mod protocol;
#[cfg(windows)]
mod remote_win_app;
pub mod script_api_view;
pub mod script_catalog;
pub mod script_clipboard;
pub mod script_error;
pub mod script_fleet;
pub mod script_http;
pub mod script_image;
pub mod script_net;
pub mod script_process;
pub mod script_project;
pub mod script_protocol;
pub mod script_stdlib;
pub mod script_stream;
pub mod script_task;
mod settings;
mod tab_tree;
mod terminal_lifecycle;
mod terminal_observation;
#[cfg(unix)]
mod terminal_selection;
mod theme;
pub mod ui_bridge;
#[cfg(any(windows, unix))]
mod ui_client;
mod ui_clipboard;
#[cfg(any(windows, unix))]
mod ui_command;
mod ui_geometry;
#[cfg(any(windows, unix))]
mod ui_interaction;
#[cfg(any(windows, unix))]
mod ui_lease;
#[cfg(unix)]
mod ui_snapshot;
mod upgrade_identity;
pub use upgrade_identity::UpgradeIdentity;
mod wake_signal;
mod working_context;
mod workspace;

mod pty;

#[cfg(not(windows))]
mod gui_wake;

#[cfg(unix)]
mod unix_app;

mod script_audit;
mod worker_supervisor;

#[cfg(windows)]
mod server_app;
#[cfg(any(windows, unix))]
mod terminal_runtime;
#[cfg(windows)]
mod win_app;

pub use client::{run_cli_entry, run_mux_entry, run_script_entry_with_args};
pub use mcp_catalog::run_mcp_entry_with_args;

#[cfg(windows)]
pub use win_app::run_gui_entry;

#[cfg(windows)]
pub use server_app::run_server_entry;

#[cfg(unix)]
pub use unix_app::run_gui_entry;

pub(crate) const IPC_TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) const SCROLLBACK_LINES: usize = 10_000;

#[cfg(windows)]
pub(crate) use win_app::request_gui_wake;

#[cfg(not(windows))]
pub(crate) use gui_wake::request_gui_wake;

pub(crate) fn ipc_address() -> String {
    client::ipc_address()
}
