use std::time::Duration;

mod build_identity;
mod client;
mod commands;
mod control_authority;
pub(crate) mod control_center;
mod control_contract;
mod control_dispatch;
mod event_journal;
pub mod incremental_wrapper;
mod instances;
mod ipc_endpoint;
mod ipc_transport;
mod locale;
pub mod mcp_catalog;
mod mcp_fleet;
pub mod mcp_stdio;
mod named_buffer;
pub mod operations;
mod protocol;
pub mod script_api_validate;
pub mod script_api_view;
pub mod script_backend;
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
pub mod script_repl;
pub mod script_rh_cache;
pub mod script_rh_cli;
pub mod script_rh_host;
pub mod script_rh_pack;
pub mod script_rh_run;
pub mod script_lua_host;
pub mod script_lua_run;
pub mod script_qjs_host;
pub mod script_runtime;
pub mod script_stdlib;
pub mod script_stream;
pub mod script_task;
mod script_worker;
mod settings;
mod tab_tree;
mod terminal_cursor;
mod terminal_lifecycle;
mod terminal_observation;
mod theme;
pub mod ui_bridge;
mod ui_client;
mod ui_clipboard;
mod ui_command;
mod ui_geometry;
mod ui_interaction;
mod ui_lease;
mod ui_snapshot;
mod upgrade_identity;
pub mod webview_host;
pub use upgrade_identity::UpgradeIdentity;
mod wake_signal;
mod working_context;
mod workspace;

mod pty;

/// Native OS adaptation boundary (`prd/PRD_02_20_native_platform.md`).
///
/// Shared contracts are primary-owned. OS adapters live under
/// `src/platform/{windows,macos,linux}/` and must not edit contract semantics.
mod platform;

mod script_audit;
mod worker_supervisor;

mod frontend;
mod frontend_server;
mod server_app;
mod terminal_runtime;

#[allow(unused_imports)]
pub use client::{run_cli_entry, run_mux_entry, run_script_entry_with_args};
pub use control_center::run_control_center_entry_with_args;
#[allow(unused_imports)]
pub use mcp_catalog::run_mcp_entry_with_args;
pub use platform::{
    install_console_interrupt_ignore_guard, install_console_interrupt_observer,
    is_direct_directory, is_direct_file, metadata_is_link_like, replace_file, sync_parent,
};

#[allow(unused_imports)]
pub use frontend::run_gui_entry;

pub use script_worker::{run_framed_worker_stdio, run_legacy_worker_stdio};
pub use server_app::{run_server_entry, run_server_entry_with_args};

pub(crate) const IPC_TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) const SCROLLBACK_LINES: usize = 10_000;

#[allow(unused_imports)]
pub(crate) use frontend::{request_gui_wake, request_gui_wake_best_effort};

pub(crate) fn ipc_address() -> String {
    client::ipc_address()
}
