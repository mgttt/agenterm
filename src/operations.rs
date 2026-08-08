use serde::Serialize;

use crate::{
    commands::{canonical_control_command, option_value},
    frontend::window::{MAX_CLIENT_EXTENT, MIN_CLIENT_HEIGHT, MIN_CLIENT_WIDTH},
    ui_geometry::{TABS_MAX_WIDTH, TABS_MIN_WIDTH},
};

pub const OPERATION_CATALOG_SCHEMA_VERSION: u32 = 1;

pub const UI_TABS_SHOW: &str = "ui.tabs.show";
pub const UI_TABS_HIDE: &str = "ui.tabs.hide";
pub const UI_TABS_TOGGLE: &str = "ui.tabs.toggle";
pub const UI_TABS_SET_WIDTH: &str = "ui.tabs.set-width";
pub const UI_WINDOW_ACTIVATE: &str = "ui.window.activate";
pub const UI_WINDOW_MAXIMIZE: &str = "ui.window.maximize";
pub const UI_WINDOW_MINIMIZE: &str = "ui.window.minimize";
pub const UI_WINDOW_RESTORE: &str = "ui.window.restore";
pub const UI_WINDOW_RESIZE: &str = "ui.window.resize";
pub const UI_WINDOW_CLOSE: &str = "ui.window.close";
pub const UI_FONT_INCREASE: &str = "ui.font.increase";
pub const UI_FONT_DECREASE: &str = "ui.font.decrease";
pub const UI_LOCALE_TOGGLE: &str = "ui.locale.toggle";
pub const TERMINAL_COPY_SELECTION: &str = "terminal.copy-selection";
pub const TERMINAL_PASTE: &str = "terminal.paste";
pub const CONTROL_CENTER_OPEN: &str = "control-center.open";
pub const CONTROL_CENTER_STATUS: &str = "control-center.status";
pub const CONTROL_CENTER_SNAPSHOT: &str = "control-center.snapshot";
pub const CONTROL_CENTER_CLOSE: &str = "control-center.close";
pub const UI_TAB_SELECT: &str = "ui.tab.select";
pub const UI_TAB_NEW_CHILD: &str = "ui.tab.new-child";
pub const UI_TREE_TOGGLE: &str = "ui.tree.toggle";
pub const UI_COMPOSER_SEND: &str = "ui.composer.send";
pub const UI_INPUT_POINTER: &str = "ui.input.pointer";
pub const UI_INPUT_WHEEL: &str = "ui.input.wheel";
pub const UI_INPUT_KEY: &str = "ui.input.key";
pub const TERMINAL_MOUSE: &str = "terminal.mouse";
pub const TABS_SET_NOTE: &str = "tabs.set-note";
pub const TAB_NOTE_MAX_BYTES: usize = 4096;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationClass {
    Observe,
    Control,
    Destructive,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct OperationParameterSpec {
    pub name: &'static str,
    pub value_type: &'static str,
    pub required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum: Option<i64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct OperationSpec {
    pub id: &'static str,
    pub script_surface: &'static str,
    pub class: OperationClass,
    pub command: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<&'static str>,
    pub aliases: &'static [&'static str],
    pub parameters: &'static [OperationParameterSpec],
    pub result_type: &'static str,
    pub errors: &'static [&'static str],
    pub events: &'static [&'static str],
    pub destructive: bool,
    pub available: bool,
    pub since: &'static str,
}

const NO_PARAMETERS: &[OperationParameterSpec] = &[];
const UI_HELLO_PARAMETERS: &[OperationParameterSpec] = &[
    OperationParameterSpec {
        name: "minimum",
        value_type: "uint32",
        required: true,
        minimum: Some(1),
        maximum: None,
    },
    OperationParameterSpec {
        name: "maximum",
        value_type: "uint32",
        required: true,
        minimum: Some(1),
        maximum: None,
    },
    OperationParameterSpec {
        name: "client_id",
        value_type: "string",
        required: false,
        minimum: Some(1),
        maximum: Some(128),
    },
];
const UI_DELTA_PARAMETERS: &[OperationParameterSpec] = &[
    OperationParameterSpec {
        name: "epoch",
        value_type: "string",
        required: true,
        minimum: None,
        maximum: None,
    },
    OperationParameterSpec {
        name: "after",
        value_type: "uint64",
        required: true,
        minimum: Some(0),
        maximum: None,
    },
    OperationParameterSpec {
        name: "limit",
        value_type: "uint32",
        required: false,
        minimum: Some(1),
        maximum: Some(64),
    },
];
const EVENT_POSITION_PARAMETERS: &[OperationParameterSpec] = &[
    OperationParameterSpec {
        name: "epoch",
        value_type: "string",
        required: true,
        minimum: None,
        maximum: None,
    },
    OperationParameterSpec {
        name: "after",
        value_type: "uint64",
        required: true,
        minimum: Some(0),
        maximum: None,
    },
];
const EVENT_READ_PARAMETERS: &[OperationParameterSpec] = &[
    EVENT_POSITION_PARAMETERS[0],
    EVENT_POSITION_PARAMETERS[1],
    OperationParameterSpec {
        name: "limit",
        value_type: "uint32",
        required: false,
        minimum: Some(1),
        maximum: Some(1024),
    },
];
const EVENT_WAIT_PARAMETERS: &[OperationParameterSpec] = &[
    EVENT_POSITION_PARAMETERS[0],
    EVENT_POSITION_PARAMETERS[1],
    OperationParameterSpec {
        name: "kind",
        value_type: "string",
        required: true,
        minimum: None,
        maximum: None,
    },
    OperationParameterSpec {
        name: "tab",
        value_type: "stable_tab_id",
        required: false,
        minimum: None,
        maximum: None,
    },
    OperationParameterSpec {
        name: "timeout_ms",
        value_type: "uint32",
        required: false,
        minimum: Some(0),
        maximum: Some(60_000),
    },
];
const CAPTURE_PARAMETERS: &[OperationParameterSpec] = &[
    OperationParameterSpec {
        name: "tab",
        value_type: "stable_tab_id",
        required: true,
        minimum: None,
        maximum: None,
    },
    OperationParameterSpec {
        name: "max_bytes",
        value_type: "uint32",
        required: true,
        minimum: Some(1),
        maximum: Some(1024 * 1024),
    },
];
/// `ui-input pointer` — window-chrome mouse in **pixel** coordinates, in the
/// same space `ui-snapshot` reports bounds in.
const POINTER_PARAMETERS: &[OperationParameterSpec] = &[
    OperationParameterSpec {
        name: "x",
        value_type: "number",
        required: true,
        minimum: None,
        maximum: None,
    },
    OperationParameterSpec {
        name: "y",
        value_type: "number",
        required: true,
        minimum: None,
        maximum: None,
    },
    OperationParameterSpec {
        name: "button",
        value_type: "string",
        required: false,
        minimum: None,
        maximum: None,
    },
    OperationParameterSpec {
        name: "action",
        value_type: "string",
        required: false,
        minimum: None,
        maximum: None,
    },
    OperationParameterSpec {
        name: "count",
        value_type: "integer",
        required: false,
        minimum: Some(1),
        maximum: Some(3),
    },
    OperationParameterSpec {
        name: "mods",
        value_type: "string",
        required: false,
        minimum: None,
        maximum: None,
    },
];
/// `ui-input wheel` — `delta_y` is required, which is why wheel cannot share
/// the pointer parameter set.
const WHEEL_PARAMETERS: &[OperationParameterSpec] = &[
    OperationParameterSpec {
        name: "x",
        value_type: "number",
        required: true,
        minimum: None,
        maximum: None,
    },
    OperationParameterSpec {
        name: "y",
        value_type: "number",
        required: true,
        minimum: None,
        maximum: None,
    },
    OperationParameterSpec {
        name: "delta_y",
        value_type: "number",
        required: true,
        minimum: None,
        maximum: None,
    },
    OperationParameterSpec {
        name: "units",
        value_type: "string",
        required: false,
        minimum: None,
        maximum: None,
    },
    OperationParameterSpec {
        name: "mods",
        value_type: "string",
        required: false,
        minimum: None,
        maximum: None,
    },
];
/// `ui-input key` — no coordinates; the key goes to whatever surface holds
/// focus, exactly as a human keystroke would.
const UI_KEY_PARAMETERS: &[OperationParameterSpec] = &[
    OperationParameterSpec {
        name: "key",
        value_type: "string",
        required: true,
        minimum: None,
        maximum: None,
    },
    OperationParameterSpec {
        name: "mods",
        value_type: "string",
        required: false,
        minimum: None,
        maximum: None,
    },
];
/// `send-mouse` — the PTY-level mouse, reported to the application inside the
/// pane. Coordinates are **terminal cells**, a different space from
/// `ui-input pointer`'s pixels; the two are layers, not alternatives.
const TERMINAL_MOUSE_PARAMETERS: &[OperationParameterSpec] = &[
    OperationParameterSpec {
        name: "x",
        value_type: "integer",
        required: true,
        minimum: Some(0),
        maximum: Some(u16::MAX as i64),
    },
    OperationParameterSpec {
        name: "y",
        value_type: "integer",
        required: true,
        minimum: Some(0),
        maximum: Some(u16::MAX as i64),
    },
    OperationParameterSpec {
        name: "button",
        value_type: "string",
        required: false,
        minimum: None,
        maximum: None,
    },
    OperationParameterSpec {
        name: "action",
        value_type: "string",
        required: false,
        minimum: None,
        maximum: None,
    },
    OperationParameterSpec {
        name: "protocol",
        value_type: "string",
        required: false,
        minimum: None,
        maximum: None,
    },
];
const TABS_WIDTH_PARAMETERS: &[OperationParameterSpec] = &[OperationParameterSpec {
    name: "width",
    value_type: "integer",
    required: true,
    minimum: Some(TABS_MIN_WIDTH as i64),
    maximum: Some(TABS_MAX_WIDTH as i64),
}];
const TAB_NOTE_PARAMETERS: &[OperationParameterSpec] = &[
    OperationParameterSpec {
        name: "tab",
        value_type: "stable_tab_id",
        required: true,
        minimum: None,
        maximum: None,
    },
    OperationParameterSpec {
        name: "note",
        value_type: "string",
        required: true,
        minimum: Some(0),
        maximum: Some(TAB_NOTE_MAX_BYTES as i64),
    },
];
/// `ui-action window-resize` — client-area size in **device pixels**, validated
/// by the shared `ClientSize::parse` on both hosts.
const WINDOW_RESIZE_PARAMETERS: &[OperationParameterSpec] = &[
    OperationParameterSpec {
        name: "width",
        value_type: "integer",
        required: true,
        minimum: Some(MIN_CLIENT_WIDTH as i64),
        maximum: Some(MAX_CLIENT_EXTENT as i64),
    },
    OperationParameterSpec {
        name: "height",
        value_type: "integer",
        required: true,
        minimum: Some(MIN_CLIENT_HEIGHT as i64),
        maximum: Some(MAX_CLIENT_EXTENT as i64),
    },
];
const SESSION_TARGET_PARAMETERS: &[OperationParameterSpec] = &[OperationParameterSpec {
    name: "target",
    value_type: "session_name",
    required: false,
    minimum: None,
    maximum: None,
}];

pub const OPERATION_CATALOG: &[OperationSpec] = &[
    OperationSpec {
        id: CONTROL_CENTER_OPEN,
        script_surface: "fleet.control_center.open",
        class: OperationClass::Control,
        command: "control-center",
        action: Some("open-control-center"),
        aliases: &[],
        parameters: NO_PARAMETERS,
        result_type: "control_center_launch",
        errors: &["control_center_unavailable", "ui_client_unavailable"],
        events: &[],
        destructive: false,
        available: true,
        since: "0.1.11",
    },
    OperationSpec {
        id: CONTROL_CENTER_STATUS,
        script_surface: "fleet.control_center.status",
        class: OperationClass::Observe,
        command: "control-center",
        action: None,
        aliases: &[],
        parameters: NO_PARAMETERS,
        result_type: "control_center_status",
        errors: &[],
        events: &[],
        destructive: false,
        available: true,
        since: "0.1.11",
    },
    OperationSpec {
        id: CONTROL_CENTER_SNAPSHOT,
        script_surface: "fleet.control_center.snapshot",
        class: OperationClass::Observe,
        command: "control-center",
        action: None,
        aliases: &[],
        parameters: NO_PARAMETERS,
        result_type: "control_center_snapshot",
        errors: &["server_unavailable", "server_incompatible"],
        events: &[],
        destructive: false,
        available: true,
        since: "0.1.11",
    },
    OperationSpec {
        id: CONTROL_CENTER_CLOSE,
        script_surface: "fleet.control_center.close",
        class: OperationClass::Control,
        command: "control-center",
        action: None,
        aliases: &[],
        parameters: NO_PARAMETERS,
        result_type: "control_center_close",
        errors: &["control_center_unavailable", "control_center_owner_changed"],
        events: &[],
        destructive: false,
        available: true,
        since: "0.1.11",
    },
    OperationSpec {
        id: "protocol.info",
        script_surface: "fleet.protocol.info",
        class: OperationClass::Observe,
        command: "protocol-info",
        action: None,
        aliases: &[],
        parameters: NO_PARAMETERS,
        result_type: "protocol_info",
        errors: &[],
        events: &[],
        destructive: false,
        available: true,
        since: "0.1.5",
    },
    OperationSpec {
        id: "ui.snapshot",
        script_surface: "fleet.ui.snapshot",
        class: OperationClass::Observe,
        command: "ui-snapshot",
        action: None,
        aliases: &[],
        parameters: NO_PARAMETERS,
        result_type: "ui_snapshot",
        errors: &["server_unavailable"],
        events: &[],
        destructive: false,
        available: true,
        since: "0.1.5",
    },
    OperationSpec {
        id: "ui.hello",
        script_surface: "fleet.ui.hello",
        class: OperationClass::Observe,
        command: "ui-hello",
        action: None,
        aliases: &[],
        parameters: UI_HELLO_PARAMETERS,
        result_type: "ui_hello",
        errors: &[
            "server_unavailable",
            "ui_hello_invalid_arguments",
            "ui_hello_serialization_failed",
        ],
        events: &[],
        destructive: false,
        available: true,
        since: "0.1.9",
    },
    OperationSpec {
        id: "ui.bootstrap",
        script_surface: "fleet.ui.bootstrap",
        class: OperationClass::Observe,
        command: "ui-bootstrap",
        action: None,
        aliases: &[],
        parameters: NO_PARAMETERS,
        result_type: "ui_bootstrap_snapshot",
        errors: &[
            "server_unavailable",
            "ui_bootstrap_unavailable",
            "ui_bootstrap_serialization_failed",
        ],
        events: &[],
        destructive: false,
        available: true,
        since: "0.1.9",
    },
    OperationSpec {
        id: "ui.deltas",
        script_surface: "fleet.ui.deltas",
        class: OperationClass::Observe,
        command: "ui-deltas",
        action: None,
        aliases: &[],
        parameters: UI_DELTA_PARAMETERS,
        result_type: "ui_delta_batch",
        errors: &[
            "server_unavailable",
            "ui_delta_invalid_arguments",
            "ui_delta_unavailable",
            "ui_delta_serialization_failed",
            "server_restart",
            "journal_gap",
            "future_sequence",
        ],
        events: &[],
        destructive: false,
        available: true,
        since: "0.1.9",
    },
    OperationSpec {
        id: "workspace.info",
        script_surface: "fleet.workspace.info",
        class: OperationClass::Observe,
        command: "workspace-info",
        action: None,
        aliases: &[],
        parameters: NO_PARAMETERS,
        result_type: "workspace_metadata_with_event_position",
        errors: &["server_unavailable"],
        events: &[],
        destructive: false,
        available: true,
        since: "0.1.6",
    },
    OperationSpec {
        id: "tabs.list",
        script_surface: "fleet.tabs.list",
        class: OperationClass::Observe,
        command: "ui-snapshot",
        action: None,
        aliases: &[],
        parameters: NO_PARAMETERS,
        result_type: "tab_list",
        errors: &["server_unavailable"],
        events: &[],
        destructive: false,
        available: true,
        since: "0.1.6",
    },
    OperationSpec {
        id: "tabs.active",
        script_surface: "fleet.tabs.active",
        class: OperationClass::Observe,
        command: "ui-snapshot",
        action: None,
        aliases: &[],
        parameters: NO_PARAMETERS,
        result_type: "tab_or_null",
        errors: &["server_unavailable"],
        events: &[],
        destructive: false,
        available: true,
        since: "0.1.6",
    },
    OperationSpec {
        id: "pane.capture",
        script_surface: "FleetTerminal.capture",
        class: OperationClass::Observe,
        command: "capture-pane",
        action: None,
        aliases: &["capturep"],
        parameters: CAPTURE_PARAMETERS,
        result_type: "bounded_capture",
        errors: &["operation_invalid_arguments", "server_unavailable"],
        events: &[],
        destructive: false,
        available: true,
        since: "0.1.6",
    },
    OperationSpec {
        id: "events.read",
        script_surface: "fleet.events.read",
        class: OperationClass::Observe,
        command: "read-events",
        action: None,
        aliases: &[],
        parameters: EVENT_READ_PARAMETERS,
        result_type: "event_batch",
        errors: &[
            "operation_invalid_arguments",
            "server_restart",
            "journal_gap",
            "future_sequence",
        ],
        events: &[],
        destructive: false,
        available: true,
        since: "0.1.5",
    },
    OperationSpec {
        id: "events.wait",
        script_surface: "fleet.events.wait",
        class: OperationClass::Observe,
        command: "wait-events",
        action: None,
        aliases: &[],
        parameters: EVENT_WAIT_PARAMETERS,
        result_type: "event",
        errors: &[
            "operation_invalid_arguments",
            "event_wait_timeout",
            "server_restart",
            "journal_gap",
            "future_sequence",
        ],
        events: &[],
        destructive: false,
        available: true,
        since: "0.1.5",
    },
    OperationSpec {
        id: UI_TABS_SHOW,
        script_surface: "fleet.ui.tabs.show",
        class: OperationClass::Control,
        command: "ui-action",
        action: Some("tabs-show"),
        aliases: &[],
        parameters: NO_PARAMETERS,
        result_type: "ui_snapshot",
        errors: &["operation_invalid_arguments"],
        events: &["layout.tabs.visibility"],
        destructive: false,
        available: true,
        since: "0.1.6",
    },
    OperationSpec {
        id: UI_TABS_HIDE,
        script_surface: "fleet.ui.tabs.hide",
        class: OperationClass::Control,
        command: "ui-action",
        action: Some("tabs-hide"),
        aliases: &[],
        parameters: NO_PARAMETERS,
        result_type: "ui_snapshot",
        errors: &["operation_invalid_arguments"],
        events: &["layout.tabs.visibility"],
        destructive: false,
        available: true,
        since: "0.1.6",
    },
    OperationSpec {
        id: UI_TABS_TOGGLE,
        script_surface: "fleet.ui.tabs.toggle",
        class: OperationClass::Control,
        command: "ui-action",
        action: Some("tabs-toggle"),
        aliases: &["toggle-tabs"],
        parameters: NO_PARAMETERS,
        result_type: "ui_snapshot",
        errors: &["operation_invalid_arguments"],
        events: &["layout.tabs.visibility"],
        destructive: false,
        available: true,
        since: "0.1.6",
    },
    OperationSpec {
        id: UI_TABS_SET_WIDTH,
        script_surface: "fleet.ui.tabs.set_width",
        class: OperationClass::Control,
        command: "ui-action",
        action: Some("tabs-set-width"),
        aliases: &[],
        parameters: TABS_WIDTH_PARAMETERS,
        result_type: "ui_snapshot",
        errors: &["operation_invalid_arguments"],
        events: &["layout.tabs.width"],
        destructive: false,
        available: true,
        since: "0.1.6",
    },
    OperationSpec {
        id: UI_TAB_SELECT,
        script_surface: "fleet.ui.tab.select",
        class: OperationClass::Control,
        command: "ui-action",
        action: Some("select-tab"),
        aliases: &[],
        parameters: NO_PARAMETERS,
        result_type: "ui_snapshot",
        errors: &["operation_invalid_arguments"],
        events: &[],
        destructive: false,
        available: true,
        since: "0.1.15",
    },
    OperationSpec {
        id: UI_TAB_NEW_CHILD,
        script_surface: "fleet.ui.tab.new_child",
        class: OperationClass::Control,
        command: "ui-action",
        action: Some("new-child"),
        aliases: &[],
        parameters: NO_PARAMETERS,
        result_type: "ui_snapshot",
        errors: &["operation_invalid_arguments"],
        events: &[],
        destructive: false,
        available: true,
        since: "0.1.15",
    },
    OperationSpec {
        id: UI_TREE_TOGGLE,
        script_surface: "fleet.ui.tree.toggle",
        class: OperationClass::Control,
        command: "ui-action",
        action: Some("toggle-tree"),
        aliases: &[],
        parameters: NO_PARAMETERS,
        result_type: "ui_snapshot",
        errors: &["operation_invalid_arguments"],
        events: &[],
        destructive: false,
        available: true,
        since: "0.1.15",
    },
    OperationSpec {
        id: UI_COMPOSER_SEND,
        script_surface: "fleet.ui.composer.send",
        class: OperationClass::Control,
        command: "ui-action",
        action: Some("composer-send"),
        aliases: &[],
        parameters: NO_PARAMETERS,
        result_type: "ui_snapshot",
        errors: &["operation_invalid_arguments"],
        events: &[],
        destructive: false,
        available: true,
        since: "0.1.15",
    },
    OperationSpec {
        id: UI_INPUT_POINTER,
        script_surface: "fleet.ui.input.pointer",
        class: OperationClass::Control,
        command: "ui-input",
        action: Some("pointer"),
        aliases: &[],
        parameters: POINTER_PARAMETERS,
        result_type: "ui_snapshot",
        errors: &["operation_invalid_arguments"],
        events: &[],
        destructive: false,
        available: true,
        since: "0.1.15",
    },
    OperationSpec {
        id: UI_INPUT_WHEEL,
        script_surface: "fleet.ui.input.wheel",
        class: OperationClass::Control,
        command: "ui-input",
        action: Some("wheel"),
        aliases: &[],
        parameters: WHEEL_PARAMETERS,
        result_type: "ui_snapshot",
        errors: &["operation_invalid_arguments"],
        events: &[],
        destructive: false,
        available: true,
        since: "0.1.15",
    },
    OperationSpec {
        id: UI_INPUT_KEY,
        script_surface: "fleet.ui.input.key",
        class: OperationClass::Control,
        command: "ui-input",
        action: Some("key"),
        aliases: &[],
        parameters: UI_KEY_PARAMETERS,
        result_type: "ui_snapshot",
        errors: &["operation_invalid_arguments"],
        events: &[],
        destructive: false,
        available: true,
        since: "0.1.16",
    },
    OperationSpec {
        id: TERMINAL_MOUSE,
        script_surface: "fleet.terminal.mouse",
        class: OperationClass::Control,
        command: "send-mouse",
        action: None,
        aliases: &[],
        parameters: TERMINAL_MOUSE_PARAMETERS,
        result_type: "text",
        errors: &[
            "operation_invalid_arguments",
            "terminal_mouse_reporting_inactive",
            "terminal_mouse_encoding_range",
            "terminal_not_writable",
        ],
        events: &[],
        destructive: false,
        available: true,
        since: "0.1.16",
    },
    OperationSpec {
        id: UI_WINDOW_ACTIVATE,
        script_surface: "fleet.ui.window.activate",
        class: OperationClass::Control,
        command: "ui-action",
        action: Some("window-activate"),
        aliases: &[],
        parameters: NO_PARAMETERS,
        result_type: "ui_snapshot",
        errors: &["operation_invalid_arguments", "ui_window_activation_failed"],
        events: &[],
        destructive: false,
        available: true,
        since: "0.1.12",
    },
    // ---- P-catalog: `ui-action` verbs promoted to typed identities ----------
    //
    // Every entry below already dispatched on **both** hosts through
    // `SHARED_UI_ACTIONS` (`src/frontend/ui_action_catalog.rs`); only the typed
    // identity was missing, so `operations.rs` described a fraction of the real
    // control plane. See `plan/agent-human-parity-audit.md` § F5 for the
    // classification rules, including why `UNIX_ONLY_UI_ACTIONS` stays out and
    // why `events` is empty here (correlation needs the dispatcher to stamp the
    // operation id, the way `set_tabs_visible` does).
    OperationSpec {
        id: UI_WINDOW_MAXIMIZE,
        script_surface: "fleet.ui.window.maximize",
        class: OperationClass::Control,
        command: "ui-action",
        action: Some("window-maximize"),
        aliases: &[],
        parameters: NO_PARAMETERS,
        result_type: "ui_snapshot",
        errors: &["operation_invalid_arguments"],
        events: &[],
        destructive: false,
        available: true,
        since: "0.1.16",
    },
    OperationSpec {
        id: UI_WINDOW_MINIMIZE,
        script_surface: "fleet.ui.window.minimize",
        class: OperationClass::Control,
        command: "ui-action",
        action: Some("window-minimize"),
        aliases: &[],
        parameters: NO_PARAMETERS,
        result_type: "ui_snapshot",
        errors: &["operation_invalid_arguments"],
        events: &[],
        destructive: false,
        available: true,
        since: "0.1.16",
    },
    OperationSpec {
        id: UI_WINDOW_RESTORE,
        script_surface: "fleet.ui.window.restore",
        class: OperationClass::Control,
        command: "ui-action",
        action: Some("window-restore"),
        aliases: &[],
        parameters: NO_PARAMETERS,
        result_type: "ui_snapshot",
        errors: &["operation_invalid_arguments"],
        events: &[],
        destructive: false,
        available: true,
        since: "0.1.16",
    },
    OperationSpec {
        id: UI_WINDOW_RESIZE,
        script_surface: "fleet.ui.window.resize",
        class: OperationClass::Control,
        command: "ui-action",
        action: Some("window-resize"),
        aliases: &[],
        parameters: WINDOW_RESIZE_PARAMETERS,
        result_type: "ui_snapshot",
        errors: &[
            "operation_invalid_arguments",
            "window_width_invalid",
            "window_height_invalid",
            "window_extent_too_large",
        ],
        events: &[],
        destructive: false,
        available: true,
        since: "0.1.16",
    },
    // Not destructive: it *requests* a close, which raises the window-close
    // confirmation modal. `stop-server-and-exit` is the destructive branch.
    OperationSpec {
        id: UI_WINDOW_CLOSE,
        script_surface: "fleet.ui.window.close",
        class: OperationClass::Control,
        command: "ui-action",
        action: Some("close-window"),
        aliases: &[],
        parameters: NO_PARAMETERS,
        result_type: "ui_snapshot",
        errors: &["operation_invalid_arguments"],
        events: &[],
        destructive: false,
        available: true,
        since: "0.1.16",
    },
    OperationSpec {
        id: UI_FONT_INCREASE,
        script_surface: "fleet.ui.font.increase",
        class: OperationClass::Control,
        command: "ui-action",
        action: Some("font-increase"),
        aliases: &[],
        parameters: NO_PARAMETERS,
        result_type: "ui_snapshot",
        errors: &["operation_invalid_arguments"],
        events: &[],
        destructive: false,
        available: true,
        since: "0.1.16",
    },
    OperationSpec {
        id: UI_FONT_DECREASE,
        script_surface: "fleet.ui.font.decrease",
        class: OperationClass::Control,
        command: "ui-action",
        action: Some("font-decrease"),
        aliases: &[],
        parameters: NO_PARAMETERS,
        result_type: "ui_snapshot",
        errors: &["operation_invalid_arguments"],
        events: &[],
        destructive: false,
        available: true,
        since: "0.1.16",
    },
    OperationSpec {
        id: UI_LOCALE_TOGGLE,
        script_surface: "fleet.ui.locale.toggle",
        class: OperationClass::Control,
        command: "ui-action",
        action: Some("toggle-locale"),
        aliases: &[],
        parameters: NO_PARAMETERS,
        result_type: "ui_snapshot",
        errors: &["operation_invalid_arguments"],
        events: &[],
        destructive: false,
        available: true,
        since: "0.1.16",
    },
    // Pairs with `terminal.paste`: the copy half of the clipboard round trip a
    // human gets from the terminal context menu.
    OperationSpec {
        id: TERMINAL_COPY_SELECTION,
        script_surface: "fleet.terminal.copy_selection",
        class: OperationClass::Control,
        command: "ui-action",
        action: Some("copy-selection"),
        aliases: &[],
        parameters: NO_PARAMETERS,
        result_type: "ui_snapshot",
        errors: &["operation_invalid_arguments"],
        events: &[],
        destructive: false,
        available: true,
        since: "0.1.16",
    },
    OperationSpec {
        id: TERMINAL_PASTE,
        script_surface: "fleet.terminal.paste",
        class: OperationClass::Control,
        command: "ui-action",
        action: Some("terminal-paste"),
        aliases: &[],
        parameters: NO_PARAMETERS,
        result_type: "ui_snapshot",
        errors: &[
            "operation_invalid_arguments",
            "terminal_paste_busy",
            "terminal_paste_failed",
            "terminal_paste_unsupported",
            "clipboard_timeout",
            "clipboard_too_large",
            "clipboard_backend_error",
        ],
        events: &["terminal.pasted"],
        destructive: false,
        available: true,
        since: "0.1.12",
    },
    OperationSpec {
        id: TABS_SET_NOTE,
        script_surface: "fleet.tabs.set_note",
        class: OperationClass::Control,
        command: "set-tab-note",
        action: None,
        aliases: &[],
        parameters: TAB_NOTE_PARAMETERS,
        result_type: "tab_snapshot",
        errors: &["operation_invalid_arguments", "operation_target_not_found"],
        events: &["tab.note"],
        destructive: false,
        available: true,
        since: "0.1.9",
    },
    OperationSpec {
        id: "server.kill",
        script_surface: "fleet.server.kill",
        class: OperationClass::Destructive,
        command: "kill-server",
        action: None,
        aliases: &["server-kill"],
        parameters: SESSION_TARGET_PARAMETERS,
        result_type: "empty",
        errors: &["operation_target_not_found", "server_unavailable"],
        events: &["workspace.shutdown"],
        destructive: true,
        available: true,
        since: "0.1.5",
    },
    OperationSpec {
        id: "workspace.shutdown",
        script_surface: "fleet.workspace.shutdown",
        class: OperationClass::Destructive,
        command: "shutdown",
        action: None,
        aliases: &[],
        parameters: NO_PARAMETERS,
        result_type: "empty",
        errors: &["operation_persistence_failed", "server_unavailable"],
        events: &["workspace.saved", "workspace.shutdown"],
        destructive: true,
        available: true,
        since: "0.1.5",
    },
];

pub fn operation_by_id(id: &str) -> Option<&'static OperationSpec> {
    OPERATION_CATALOG
        .iter()
        .find(|operation| operation.id == id)
}

pub(crate) fn operation_for_args(
    args: &[String],
) -> Result<Option<&'static OperationSpec>, String> {
    let Some(command) = args.first().map(String::as_str) else {
        return Ok(None);
    };
    let command = canonical_control_command(command);
    let operation = match command {
        "control-center" => match args.get(1).map(String::as_str) {
            Some("open") => operation_by_id(CONTROL_CENTER_OPEN),
            Some("status") => operation_by_id(CONTROL_CENTER_STATUS),
            Some("snapshot") => operation_by_id(CONTROL_CENTER_SNAPSHOT),
            Some("close") => operation_by_id(CONTROL_CENTER_CLOSE),
            _ => None,
        },
        "protocol-info" => operation_by_id("protocol.info"),
        "ui-hello" => operation_by_id("ui.hello"),
        "ui-bootstrap" => operation_by_id("ui.bootstrap"),
        "ui-deltas" => operation_by_id("ui.deltas"),
        "ui-snapshot" => operation_by_id("ui.snapshot"),
        "read-events" => operation_by_id("events.read"),
        "wait-events" => operation_by_id("events.wait"),
        "set-tab-note" if option_value(args, "-t").is_some_and(is_stable_tab_id) => {
            operation_by_id(TABS_SET_NOTE)
        }
        "kill-server" | "server-kill" => operation_by_id("server.kill"),
        "shutdown" => operation_by_id("workspace.shutdown"),
        "send-mouse" => operation_by_id(TERMINAL_MOUSE),
        "ui-input" => {
            let id = match args.get(1).map(String::as_str) {
                Some("pointer") => UI_INPUT_POINTER,
                Some("wheel") => UI_INPUT_WHEEL,
                Some("key") => UI_INPUT_KEY,
                _ => return Ok(None),
            };
            operation_by_id(id)
        }
        "ui-action" => {
            let Some(action) = args.get(1).map(String::as_str) else {
                return Ok(None);
            };
            let id = match action {
                "tabs-show" => UI_TABS_SHOW,
                "tabs-hide" => UI_TABS_HIDE,
                "tabs-toggle" | "toggle-tabs" => UI_TABS_TOGGLE,
                "tabs-set-width" => UI_TABS_SET_WIDTH,
                "window-activate" => UI_WINDOW_ACTIVATE,
                "window-maximize" => UI_WINDOW_MAXIMIZE,
                "window-minimize" => UI_WINDOW_MINIMIZE,
                "window-restore" => UI_WINDOW_RESTORE,
                "window-resize" => UI_WINDOW_RESIZE,
                "close-window" => UI_WINDOW_CLOSE,
                "font-increase" => UI_FONT_INCREASE,
                "font-decrease" => UI_FONT_DECREASE,
                "toggle-locale" => UI_LOCALE_TOGGLE,
                "copy-selection" => TERMINAL_COPY_SELECTION,
                "terminal-paste" => TERMINAL_PASTE,
                "open-control-center" => CONTROL_CENTER_OPEN,
                "select-tab" => UI_TAB_SELECT,
                "new-child" => UI_TAB_NEW_CHILD,
                "toggle-tree" => UI_TREE_TOGGLE,
                "composer-send" => UI_COMPOSER_SEND,
                action if action.starts_with("tabs-") => {
                    return Err(operation_error(
                        "operation_unknown",
                        action,
                        "unknown typed Tabs action",
                    ));
                }
                _ => return Ok(None),
            };
            operation_by_id(id)
        }
        _ => return Ok(None),
    };
    Ok(operation)
}

pub(crate) fn validate_operation_args(
    args: &[String],
) -> Result<Option<&'static OperationSpec>, String> {
    let operation = operation_for_args(args)?;
    let Some(operation) = operation else {
        return Ok(None);
    };
    if operation.id == TABS_SET_NOTE {
        if args.len() != 4 || args.get(1).map(String::as_str) != Some("-t") {
            return Err(operation_error(
                "operation_invalid_arguments",
                operation.id,
                "accepts exactly -t @ID NOTE",
            ));
        }
        let note = args.get(3).expect("typed note command length was checked");
        if note.len() > TAB_NOTE_MAX_BYTES {
            return Err(operation_error(
                "operation_invalid_arguments",
                operation.id,
                &format!("NOTE must be at most {TAB_NOTE_MAX_BYTES} UTF-8 bytes"),
            ));
        }
    } else if operation.id == UI_TABS_SET_WIDTH {
        if args.len() != 4 {
            return Err(operation_error(
                "operation_invalid_arguments",
                operation.id,
                "accepts exactly --width PX",
            ));
        }
        let Some(raw_width) = option_value(args, "--width") else {
            return Err(operation_error(
                "operation_invalid_arguments",
                operation.id,
                "requires --width PX",
            ));
        };
        let Ok(width) = raw_width.parse::<i32>() else {
            return Err(operation_error(
                "operation_invalid_arguments",
                operation.id,
                "--width must be an integer",
            ));
        };
        if !(TABS_MIN_WIDTH..=TABS_MAX_WIDTH).contains(&width) {
            return Err(operation_error(
                "operation_invalid_arguments",
                operation.id,
                &format!("--width must be from {TABS_MIN_WIDTH} to {TABS_MAX_WIDTH}"),
            ));
        }
    } else if operation.id == CONTROL_CENTER_OPEN
        && args
            .first()
            .is_some_and(|command| command == "control-center")
    {
        if !matches!(
            args,
            [command, subcommand]
                if command == "control-center" && subcommand == "open"
        ) && !matches!(
            args,
            [command, subcommand, flag]
                if command == "control-center"
                    && subcommand == "open"
                    && flag == "--no-activate"
        ) {
            return Err(operation_error(
                "operation_invalid_arguments",
                operation.id,
                "accepts open with optional --no-activate",
            ));
        }
    } else if operation.id == CONTROL_CENTER_OPEN && args.len() != 2 {
        return Err(operation_error(
            "operation_invalid_arguments",
            operation.id,
            "ui-action open-control-center does not accept additional arguments",
        ));
    } else if (matches!(
        operation.id,
        CONTROL_CENTER_STATUS | CONTROL_CENTER_SNAPSHOT | CONTROL_CENTER_CLOSE
    ) || (operation.command == "ui-action" && operation.parameters.is_empty()))
        && args.len() != 2
    {
        // Only *nullary* UI actions can be arity-checked here. Parameterised
        // ones (`--width/--height`, `-t`, `--path`, …) declare their inputs in
        // the catalog and are validated by the dispatcher that owns them; a
        // blanket `len != 2` would make them unreachable.
        return Err(operation_error(
            "operation_invalid_arguments",
            operation.id,
            "does not accept additional arguments",
        ));
    }
    Ok(Some(operation))
}

fn is_stable_tab_id(value: &str) -> bool {
    value
        .strip_prefix('@')
        .is_some_and(|id| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()))
}

fn operation_error(code: &str, identity: &str, message: &str) -> String {
    format!("{code}[{identity}]: {message}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn catalog_has_stable_unique_ids_and_all_classes() {
        let mut ids = OPERATION_CATALOG
            .iter()
            .map(|operation| operation.id)
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), OPERATION_CATALOG.len());
        assert!(
            OPERATION_CATALOG
                .iter()
                .any(|operation| { operation.class == OperationClass::Observe })
        );
        assert!(
            OPERATION_CATALOG
                .iter()
                .any(|operation| { operation.class == OperationClass::Control })
        );
        assert!(
            OPERATION_CATALOG
                .iter()
                .any(|operation| { operation.class == OperationClass::Destructive })
        );
        for operation in OPERATION_CATALOG {
            assert!(
                crate::commands::command_identity(operation.command).is_some(),
                "operation {} references unknown command {}",
                operation.id,
                operation.command
            );
            assert!(!operation.result_type.is_empty());
            assert!(!operation.since.is_empty());
            assert!(operation.available);
            assert_eq!(
                operation.destructive,
                operation.class == OperationClass::Destructive
            );
            for parameter in operation.parameters {
                assert!(!parameter.name.is_empty());
                assert!(!parameter.value_type.is_empty());
                assert!(
                    parameter
                        .minimum
                        .zip(parameter.maximum)
                        .is_none_or(|(minimum, maximum)| minimum <= maximum)
                );
            }
        }
    }

    #[test]
    fn legacy_toggle_tabs_resolves_to_the_stable_typed_identity() {
        let operation = validate_operation_args(&args(&["ui-action", "toggle-tabs"])).unwrap();
        assert_eq!(
            operation.map(|operation| operation.id),
            Some(UI_TABS_TOGGLE)
        );
    }

    #[test]
    fn native_window_activation_and_terminal_paste_have_stable_identities() {
        for (action, expected) in [
            ("window-activate", UI_WINDOW_ACTIVATE),
            ("terminal-paste", TERMINAL_PASTE),
        ] {
            let operation = validate_operation_args(&args(&["ui-action", action])).unwrap();
            assert_eq!(operation.map(|operation| operation.id), Some(expected));

            let error = validate_operation_args(&args(&["ui-action", action, "extra"]))
                .expect_err("semantic UI actions must reject extra arguments");
            assert!(error.contains("operation_invalid_arguments"));
        }
    }

    #[test]
    fn control_center_surfaces_share_the_stable_open_identity() {
        for values in [
            &["control-center", "open"][..],
            &["control-center", "open", "--no-activate"][..],
            &["ui-action", "open-control-center"][..],
        ] {
            let operation = validate_operation_args(&args(values)).unwrap();
            assert_eq!(
                operation.map(|operation| operation.id),
                Some(CONTROL_CENTER_OPEN)
            );
        }
        assert_eq!(
            validate_operation_args(&args(&["control-center", "status"]))
                .unwrap()
                .map(|operation| operation.id),
            Some(CONTROL_CENTER_STATUS)
        );
        assert_eq!(
            validate_operation_args(&args(&["control-center", "snapshot"]))
                .unwrap()
                .map(|operation| operation.id),
            Some(CONTROL_CENTER_SNAPSHOT)
        );
        assert_eq!(
            validate_operation_args(&args(&["control-center", "close"]))
                .unwrap()
                .map(|operation| operation.id),
            Some(CONTROL_CENTER_CLOSE)
        );
        assert!(
            validate_operation_args(&args(&["control-center", "snapshot", "--no-activate"]))
                .is_err()
        );
    }

    #[test]
    fn validates_typed_tabs_width_boundaries() {
        let width = operation_by_id(UI_TABS_SET_WIDTH).unwrap();
        assert_eq!(width.parameters, TABS_WIDTH_PARAMETERS);
        assert_eq!(width.result_type, "ui_snapshot");
        assert_eq!(width.events, ["layout.tabs.width"]);
        for width in [TABS_MIN_WIDTH, TABS_MAX_WIDTH] {
            let operation = validate_operation_args(&args(&[
                "ui-action",
                "tabs-set-width",
                "--width",
                &width.to_string(),
            ]))
            .unwrap();
            assert_eq!(
                operation.map(|operation| operation.id),
                Some(UI_TABS_SET_WIDTH)
            );
        }
        let error = validate_operation_args(&args(&[
            "ui-action",
            "tabs-set-width",
            "--width",
            &(TABS_MIN_WIDTH - 1).to_string(),
        ]))
        .unwrap_err();
        assert!(error.starts_with("operation_invalid_arguments[ui.tabs.set-width]"));
    }

    #[test]
    fn typed_tab_note_requires_stable_target_and_bounded_utf8() {
        let operation =
            validate_operation_args(&args(&["set-tab-note", "-t", "@42", "目录 note"])).unwrap();
        assert_eq!(operation.map(|operation| operation.id), Some(TABS_SET_NOTE));
        assert_eq!(operation.unwrap().events, ["tab.note"]);

        assert!(
            validate_operation_args(&args(&["set-tab-note", "-t", "name", "legacy"]))
                .unwrap()
                .is_none(),
            "mutable title targeting remains legacy rather than claiming a typed identity"
        );
        let oversized = "x".repeat(TAB_NOTE_MAX_BYTES + 1);
        let error =
            validate_operation_args(&args(&["set-tab-note", "-t", "@42", &oversized])).unwrap_err();
        assert!(error.starts_with("operation_invalid_arguments[tabs.set-note]"));
    }

    /// A typed identity for a verb no host dispatches would be exactly the
    /// F3-class lie this catalog exists to prevent: an agent reading the map
    /// would find a door that opens onto nothing. Machine-check it instead.
    #[test]
    fn every_typed_ui_action_is_dispatchable_on_both_hosts() {
        use crate::frontend::ui_action_catalog::SHARED_UI_ACTIONS;

        for operation in OPERATION_CATALOG {
            if operation.command != "ui-action" {
                continue;
            }
            let action = operation
                .action
                .unwrap_or_else(|| panic!("ui-action operation {} declares no verb", operation.id));
            assert!(
                SHARED_UI_ACTIONS.contains(&action),
                "operation {} claims ui-action {action}, which is not in SHARED_UI_ACTIONS",
                operation.id
            );
            for alias in operation.aliases {
                assert!(
                    SHARED_UI_ACTIONS.contains(alias),
                    "operation {} claims ui-action alias {alias}, \
                     which is not in SHARED_UI_ACTIONS",
                    operation.id
                );
            }
            let resolved = operation_for_args(&args(&["ui-action", action]))
                .unwrap_or_else(|error| panic!("{action} did not resolve: {error}"));
            assert_eq!(
                resolved.map(|resolved| resolved.id),
                Some(operation.id),
                "ui-action {action} does not round-trip to {}",
                operation.id
            );
        }
    }

    #[test]
    fn window_font_and_clipboard_chrome_have_stable_identities() {
        for (action, expected) in [
            ("window-maximize", UI_WINDOW_MAXIMIZE),
            ("window-minimize", UI_WINDOW_MINIMIZE),
            ("window-restore", UI_WINDOW_RESTORE),
            ("close-window", UI_WINDOW_CLOSE),
            ("font-increase", UI_FONT_INCREASE),
            ("font-decrease", UI_FONT_DECREASE),
            ("toggle-locale", UI_LOCALE_TOGGLE),
            ("copy-selection", TERMINAL_COPY_SELECTION),
        ] {
            let operation = validate_operation_args(&args(&["ui-action", action])).unwrap();
            assert_eq!(operation.map(|operation| operation.id), Some(expected));
            assert!(
                validate_operation_args(&args(&["ui-action", action, "extra"])).is_err(),
                "nullary UI action {action} must reject extra arguments"
            );
        }
    }

    /// `window-resize` is the first typed UI action that carries options, so it
    /// also pins that declaring parameters lifts the nullary arity check.
    #[test]
    fn parameterised_ui_actions_keep_their_declared_options() {
        let resize = operation_by_id(UI_WINDOW_RESIZE).unwrap();
        assert_eq!(resize.parameters, WINDOW_RESIZE_PARAMETERS);
        let operation = validate_operation_args(&args(&[
            "ui-action",
            "window-resize",
            "--width",
            "1024",
            "--height",
            "768",
        ]))
        .unwrap();
        assert_eq!(
            operation.map(|operation| operation.id),
            Some(UI_WINDOW_RESIZE)
        );
    }

    #[test]
    fn rejects_unknown_typed_tabs_actions() {
        let error = validate_operation_args(&args(&["ui-action", "tabs-teleport"])).unwrap_err();
        assert_eq!(
            error,
            "operation_unknown[tabs-teleport]: unknown typed Tabs action"
        );
    }
}
