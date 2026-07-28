#!/usr/bin/env python3
"""Split lib.rs.bak into lib.rs (root), client/mod.rs, and win_app.rs."""

from pathlib import Path

SRC = Path("src")
bak = (SRC / "lib.rs.bak").read_text()
lines = bak.splitlines(keepends=True)

# 1-based inclusive line ranges from the backup.
CLIENT_RANGES = [
    (553, 747),    # CliControlOptions + run_cli_entry + run_mux_entry
    (8263, 10700), # ipc_address .. tests
]
WIN_RANGES = [
    (1, 16),       # std imports (without windows_sys block)
    (75, 101),     # mod declarations
    (103, 236),    # uses + constants through bounded_utf8_prefix
    (247, 264),    # request_gui_wake + thread_local IPC override
    (266, 336),    # rgb helpers through bounded_utf8_prefix end? check
    (338, 551),    # run_gui_entry helpers
    (749, 8261),   # run_gui body through create_terminal_font
]

def extract(ranges):
    out = []
    for start, end in ranges:
        out.extend(lines[start - 1 : end])
    return "".join(out)

# win_app needs windows imports (lines 18-73) prepended
win_header = "".join(lines[17:73])
# client uses from line 103-159 use statements - extract portable uses only
client_uses = """use std::{
    cell::RefCell,
    env,
    fs::OpenOptions,
    io::{Read, Write},
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context as _, Result};

use crate::{
    build_identity::BuildIdentity,
    commands::{
        BACKSPACE_INPUT, COMMAND_CATALOG, COMMAND_CATALOG_SCHEMA_VERSION, MUX_COMMANDS, MuxStatus,
        canonical_control_command, control_command_requests_help, control_command_usage, has_option,
        last_positional, mux_command, option_value, parse_new_command, parse_tab_environment,
        positional_values, screenshot_output_path, snapshot_modal_matches, supported_commands,
        tmux_key_bytes, validate_control_command,
    },
    control_contract::{
        Admission, ControlError, ControlReceipt, ControlRequest, ErrorCategory,
        EventPosition as ControlEventPosition, OperationId, PayloadFingerprint, ReceiptOutcome,
        ReplayWindow, RequestId, RequestIntent, ResolvedTarget, WaitCondition, WaitDescriptor,
    },
    event_journal::{EVENT_CATALOG, EVENT_CATALOG_SCHEMA_VERSION, EventJournal, EventKind},
    instances::{discover_instances, instance_process_is_alive, prune_instance},
    ipc_transport::read_bounded_ipc_line,
    operations::{
        OPERATION_CATALOG, OPERATION_CATALOG_SCHEMA_VERSION, OperationClass, OperationSpec,
        UI_TABS_HIDE, UI_TABS_SET_WIDTH, UI_TABS_SHOW, UI_TABS_TOGGLE, operation_for_args,
        validate_operation_args,
    },
    protocol::{IpcRequest, IpcResponse},
    script_protocol::{
        SCRIPT_API_VERSION, SCRIPT_ENVELOPE_VERSION, ScriptBrokerError, ScriptBrokerRequest,
        ScriptBrokerResponse, ScriptBudgets, ScriptExitClass, ScriptInvocation, ScriptOperation,
        ScriptProfile,
    },
    settings::{AppConfig, config_path, load_config, save_config},
    tab_tree::{TabTreeNode, TabTreeRow, tree_rows, would_create_cycle},
    terminal_observation::TerminalProcessState,
    terminal_selection::{
        AutoScrollDirection, AutoScrollStep, SelectionGesture, TerminalPoint, TerminalSelection,
        autoscroll_step, terminal_selection_text, visible_row_selection, word_selection,
    },
    theme::{ThemeId, ThemePalette},
    ui_bridge,
    ui_geometry::{
        COMPOSER_HEIGHT, PixelRect, TAB_HEIGHT, TERMINAL_SCROLLBAR_WIDTH, TerminalScrollbarGeometry,
        TreeRowActionDensity, TreeRowMode, WorkspaceLayout, WorkspaceLayoutInput, reset_tabs_width,
        scrollback_for_thumb_top, tabs_width_from_drag, terminal_scrollbar_geometry, tree_connector_x,
        tree_row_at_y, tree_row_geometry_for_mode, workspace_layout,
    },
    upgrade_identity::UpgradeIdentity,
    working_context::{
        CwdSource, PROXY_MAX_BYTES, ProxyConfirmationMarker, ProxyState, cwd_command,
        parse_proxy_editor, proxy_command_with_confirmation, validate_path,
    },
    workspace::{SavedTab, SavedWorkspace, load_workspace, save_workspace, workspace_path},
};

"""

client_body = extract(CLIENT_RANGES)

# Gate windows-only imports in client body
client_script_audit = """
#[cfg(windows)]
use crate::script_audit::{
    AuditBudgets, AuditInvocation, AuditOutcome, AuditSourceKind, ScriptAuditSink,
    source_fingerprint,
};
#[cfg(windows)]
use crate::worker_supervisor::{SupervisorError, WorkerSupervisor};
"""

client_constants = """
const IPC_TIMEOUT: Duration = Duration::from_secs(5);
const IPC_DISCOVERY_TIMEOUT: Duration = Duration::from_millis(500);
const IPC_AUTOSTART_TIMEOUT: Duration = Duration::from_secs(15);
const IPC_AUTOSTART_POLL: Duration = Duration::from_millis(100);
const IPC_MAX_RESPONSE_BYTES: u64 = 8 * 1024 * 1024;
const CAPTURE_PUBLIC_MAX_BYTES: usize = 1024 * 1024;

thread_local! {
    static IPC_ADDRESS_OVERRIDE: RefCell<Option<String>> = const { RefCell::new(None) };
}

"""

client_mod = f"""pub(crate) use run_cli_entry;
pub(crate) use run_mux_entry;

{client_uses}
{client_script_audit}
{client_constants}
{client_body}
"""

# Fix ipc_address USERNAME -> portable
client_mod = client_mod.replace(
    'let user = env::var("USERNAME").unwrap_or_else(|_| "default".to_owned());',
    'let user = env::var("USERNAME")\n        .or_else(|_| env::var("USER"))\n        .unwrap_or_else(|_| "default".to_owned());',
)

# Gate script command windows-only parts - will fix compile errors after
(SRC / "client" / "mod.rs").write_text(client_mod)

win_body = extract(WIN_RANGES)
# Fix win imports - add mod-level uses from original
win_uses = "".join(lines[102:159])
win_mod = f"""{win_header}
{win_uses}
{win_body}
"""
(SRC / "win_app.rs").write_text(win_mod)

lib_rs = '''use std::time::Duration;

mod build_identity;
mod commands;
mod control_contract;
mod event_journal;
mod instances;
mod ipc_transport;
pub mod operations;
mod protocol;
mod rmux_status;
pub mod script_catalog;
pub mod script_protocol;
pub mod script_stdlib;
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

#[cfg(windows)]
mod script_audit;
#[cfg(windows)]
mod terminal_runtime;
#[cfg(windows)]
mod worker_supervisor;
#[cfg(windows)]
mod win_app;

mod client;

pub use client::{run_cli_entry, run_mux_entry};

#[cfg(windows)]
pub use win_app::run_gui_entry;

#[cfg(not(windows))]
pub fn run_gui_entry() -> i32 {
    eprintln!("AgenTerm GUI is only available on Windows.");
    1
}

pub(crate) const IPC_TIMEOUT: Duration = Duration::from_secs(5);

#[cfg(windows)]
pub(crate) use win_app::request_gui_wake;

#[cfg(not(windows))]
pub(crate) fn request_gui_wake(_wake_window: isize, _wake_signal: &wake_signal::WakeSignal) {}

#[cfg(windows)]
pub(crate) fn ipc_address() -> String {
    client::ipc_address()
}

#[cfg(not(windows))]
pub(crate) fn ipc_address() -> String {
    client::ipc_address()
}
'''
(SRC / "lib.rs").write_text(lib_rs)
print("split complete")
