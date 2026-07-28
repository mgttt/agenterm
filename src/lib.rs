#![cfg_attr(not(windows), allow(dead_code))]

use std::time::Duration;

mod build_identity;
mod client;
mod commands;
mod control_contract;
mod event_journal;
mod instances;
mod ipc_transport;
pub mod operations;
mod protocol;
mod rmux_status;
pub mod script_catalog;
pub mod script_fleet;
pub mod script_http;
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
mod terminal_selection;
mod theme;
pub mod ui_bridge;
mod ui_geometry;
mod upgrade_identity;
mod wake_signal;
mod working_context;
mod workspace;

mod pty;

#[cfg(not(windows))]
mod gui_wake;

#[cfg(unix)]
mod unix_app;

#[cfg(windows)]
mod script_audit;
#[cfg(any(windows, unix))]
mod terminal_runtime;
#[cfg(windows)]
mod win_app;
#[cfg(windows)]
mod worker_supervisor;

pub use client::{run_cli_entry, run_mux_entry};

#[cfg(windows)]
pub use win_app::run_gui_entry;

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
