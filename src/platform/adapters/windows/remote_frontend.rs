//! Windows native replaceable GUI projection.

use std::{
    sync::{
        Arc, Condvar, Mutex,
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::frontend::interaction::{
    FocusDirection, FocusState, FocusSurface, FocusTransitionGate, ModalSurface, MouseReportInput,
    MouseReportOutcome, ScrollbarThumbDrag, WheelAccumulator, WheelTarget, modal_surface_from_gate,
    mouse_protocol_mode_from_str, mouse_report_encoding_from_str, mouse_report_outcome,
    route_wheel, sidebar_scroll_offset_for_thumb_top,
};
use crate::ui_snapshot::{
    PROJECTION_REPLACEABLE_UI_CLIENT, SYSTEM_MENU_COPY_ID as SHARED_SYSTEM_MENU_COPY_ID,
    SYSTEM_MENU_PASTE_ID as SHARED_SYSTEM_MENU_PASTE_ID,
    SYSTEM_MENU_TOGGLE_TABS_ID as SHARED_SYSTEM_MENU_TOGGLE_TABS_ID,
};
use crate::{
    client::{ipc_address, resolved_ipc_endpoint},
    commands::{
        alternate_screen_wheel_bytes, option_value, positional_values, screenshot_output_path,
        tmux_key_bytes_with_modifiers,
    },
    frontend::{
        action,
        close_confirmation::CloseConfirmation,
        composer::ComposerWriteMode,
        cwd_editor::CwdEditorDialog,
        new_terminal::{self, NewShellChoice, NewTerminalDialog},
        selection::{
            AutoScrollDirection, AutoScrollStep, RemotePoint, RemoteSelectionGesture,
            SelectionGesturePhase, autoscroll_step, remote_visible_row_selection,
            remote_word_selection,
        },
        settings::{self, AppearanceField, SettingsDialog, SettingsScope},
        tab_editor::{TabEditorDialog, TabEditorFocus},
        toolbar::NativeToolbarHit as WindowsToolbarHit,
        window::{ClientSize, WindowSemanticState},
        window_close::{WindowCloseChoice, WindowCloseDialog},
    },
    locale::UiText,
    platform::KeyClassification,
    protocol::IpcResponse,
    settings::{
        AppConfig, EffectiveTerminalAppearance, MAX_TERMINAL_FONT_SIZE, MIN_TERMINAL_FONT_SIZE,
        clamp_tabs_width, config_path, load_config, save_config,
    },
    tab_tree::{TabTreeNode, tree_rows},
    theme::{Rgb, ThemeId, ThemePalette},
    ui_bridge::{UiCellStyle, UiColor, UiScreenSnapshot, UiTabBootstrap},
    ui_client::{UiClientModel, tab_by_id},
    ui_clipboard::{TERMINAL_PASTE_LIMIT_BYTES, normalize_terminal_paste, terminal_paste_bytes},
    ui_command::{UI_CLIENT_COMMAND_FOCUS, UI_CLIENT_COMMAND_SHOW_NO_ACTIVATE, UiClientCommand},
    ui_geometry::{
        PixelRect as ProductPixelRect, TAB_HEIGHT, TAB_TOP, TERMINAL_SCROLLBAR_WIDTH,
        TerminalScrollbarGeometry, TreeRowActionDensity, TreeRowGeometry, TreeRowMode,
        WHEEL_ROWS_PER_NOTCH, WorkspaceLayout, WorkspaceLayoutInput, pixel_rect_json,
        reset_tabs_width, scrollback_for_thumb_top, sidebar_scrollbar_track,
        sidebar_tree_row_geometry, tabs_width_from_drag, terminal_cell_at,
        terminal_scrollbar_geometry, tree_connector_segments, tree_row_at_y, wheel_delta_units,
        workspace_layout,
    },
};
use agenterm_platform::{
    clipboard,
    control_window::{
        ButtonState, ControlCanvas, ControlId, ControlKind, ControlSpec, ControlWheelDelta,
        ControlWindow, ControlWindowApplication, ControlWindowDirective, ControlWindowError,
        ControlWindowEvent, ControlWindowOptions, ControlWindowQuery, ControlWindowRenderActivity,
        FocusTarget, MenuCommandId, PixelPoint, PixelRect as ControlPixelRect, PixelSize,
        PointerButton, Rgb8, SystemMenuItem, TextHorizontalAlignment, TextOptions,
        WindowPresentation, run_control_window,
    },
    input,
};
use anyhow::{Context as _, Result};
use unicode_width::UnicodeWidthChar;

const MAX_FRAME_PIXELS: i64 = 33_554_432;
const KEY_TAB: u16 = 0x09;
const KEY_ESCAPE: u16 = 0x1b;
const KEY_PAGE_UP: u16 = 0x21;
const KEY_PAGE_DOWN: u16 = 0x22;
const KEY_END: u16 = 0x23;
const KEY_HOME: u16 = 0x24;
const KEY_LEFT: u16 = 0x25;
const KEY_UP: u16 = 0x26;
const KEY_RIGHT: u16 = 0x27;
const KEY_DOWN: u16 = 0x28;
const KEY_INSERT: u16 = 0x2d;
const KEY_DELETE: u16 = 0x2e;
const KEY_F1: u16 = 0x70;
const KEY_F2: u16 = 0x71;
const KEY_F3: u16 = 0x72;
const KEY_F4: u16 = 0x73;
const KEY_F5: u16 = 0x74;
const KEY_F6: u16 = 0x75;
const KEY_F7: u16 = 0x76;
const KEY_F8: u16 = 0x77;
const KEY_F9: u16 = 0x78;
const KEY_F10: u16 = 0x79;
const KEY_F11: u16 = 0x7a;
const KEY_F12: u16 = 0x7b;
const DOUBLE_CLICK_MS: u64 = 500;

const EDIT_ID: ControlId = ControlId(2101);
const SEND_ID: ControlId = ControlId(2102);
const NEW_ID: ControlId = ControlId(2103);
const TABS_ID: ControlId = ControlId(2104);
const TAB_TITLE_EDIT_ID: ControlId = ControlId(2105);
const TAB_NOTE_EDIT_ID: ControlId = ControlId(2106);
const TAB_SAVE_ID: ControlId = ControlId(2107);
const TAB_CANCEL_ID: ControlId = ControlId(2108);
const CLOSE_KEEP_ID: ControlId = ControlId(2109);
const CLOSE_STOP_ID: ControlId = ControlId(2110);
const CLOSE_CANCEL_ID: ControlId = ControlId(2111);
const SETTINGS_ID: ControlId = ControlId(2112);
const SETTINGS_FONT_ID: ControlId = ControlId(2113);
const SETTINGS_SIZE_ID: ControlId = ControlId(2114);
const SETTINGS_DARK_ID: ControlId = ControlId(2115);
const SETTINGS_LIGHT_ID: ControlId = ControlId(2116);
const SETTINGS_APPLY_ID: ControlId = ControlId(2117);
const SETTINGS_CANCEL_ID: ControlId = ControlId(2118);
const TAB_CLOSE_CONFIRM_ID: ControlId = ControlId(2119);
const TAB_CLOSE_CANCEL_ID: ControlId = ControlId(2120);
const NEW_DEFAULT_SHELL_ID: ControlId = ControlId(2121);
const NEW_CMD_SHELL_ID: ControlId = ControlId(2122);
const NEW_POWERSHELL_ID: ControlId = ControlId(2123);
const NEW_INITIAL_COMMAND_ID: ControlId = ControlId(2124);
const NEW_HTTP_PROXY_ID: ControlId = ControlId(2125);
const NEW_HTTPS_PROXY_ID: ControlId = ControlId(2126);
const NEW_CREATE_ID: ControlId = ControlId(2127);
const NEW_CANCEL_ID: ControlId = ControlId(2128);
const LOCALE_ID: ControlId = ControlId(2129);
const FONT_DECREASE_ID: ControlId = ControlId(2130);
const FONT_INCREASE_ID: ControlId = ControlId(2131);
const SETTINGS_DEFAULT_SCOPE_ID: ControlId = ControlId(2132);
const SETTINGS_CURRENT_SCOPE_ID: ControlId = ControlId(2133);
const SETTINGS_FONT_INHERIT_ID: ControlId = ControlId(2134);
const SETTINGS_SIZE_INHERIT_ID: ControlId = ControlId(2135);
const SETTINGS_THEME_INHERIT_ID: ControlId = ControlId(2136);
const SETTINGS_RESET_OVERRIDES_ID: ControlId = ControlId(2137);
const CONTROL_CENTER_ID: ControlId = ControlId(2138);
const SYSTEM_MENU_COPY_ID: MenuCommandId = MenuCommandId(SHARED_SYSTEM_MENU_COPY_ID as u16);
const SYSTEM_MENU_PASTE_ID: MenuCommandId = MenuCommandId(SHARED_SYSTEM_MENU_PASTE_ID as u16);
const SYSTEM_MENU_TOGGLE_TABS_ID: MenuCommandId =
    MenuCommandId(SHARED_SYSTEM_MENU_TOGGLE_TABS_ID as u16);
const WM_APP_AUTOMATION_SHORTCUT: u32 = 0x8000 + 2;
const WM_APP_FOCUS_QUERY: u32 = 0x8000 + 3;

const fn windows_toolbar_hit(control_id: ControlId) -> Option<WindowsToolbarHit> {
    match control_id {
        TABS_ID => Some(WindowsToolbarHit::ToggleTabs),
        NEW_ID => Some(WindowsToolbarHit::NewTab),
        CONTROL_CENTER_ID => Some(WindowsToolbarHit::ControlCenter),
        SETTINGS_ID => Some(WindowsToolbarHit::Settings),
        LOCALE_ID => Some(WindowsToolbarHit::ToggleLocale),
        FONT_DECREASE_ID => Some(WindowsToolbarHit::FontDecrease),
        FONT_INCREASE_ID => Some(WindowsToolbarHit::FontIncrease),
        _ => None,
    }
}

fn toolbar_action_returns_terminal_focus(action_id: &str) -> bool {
    matches!(
        action_id,
        action::TOGGLE_TABS | action::TOGGLE_LOCALE | action::FONT_DECREASE | action::FONT_INCREASE
    )
}

const STATUS_HEIGHT: i32 = 26;
const COMPOSER_HEIGHT: i32 = 104;
const MARGIN: i32 = 6;
const RECONNECT_INTERVAL: Duration = Duration::from_millis(500);
const WINDOW_CLOSE_BUTTON_TEXT_FORMAT: u32 = 0x25;

trait CanvasRgb {
    fn canvas_rgb(self) -> Rgb8;
}

impl CanvasRgb for Rgb {
    fn canvas_rgb(self) -> Rgb8 {
        Rgb8::new(self.red, self.green, self.blue)
    }
}

fn control_spec(id: ControlId, kind: ControlKind, text: &str, visible: bool) -> ControlSpec {
    ControlSpec {
        id,
        kind,
        text: text.to_owned(),
        bounds: ControlPixelRect::new(0, 0, 100, 32),
        enabled: true,
        visible,
        tab_stop: true,
    }
}

fn control_rect(rect: ProductPixelRect) -> ControlPixelRect {
    ControlPixelRect::new(
        rect.left,
        rect.top,
        u32::try_from(rect.width().max(0)).unwrap_or(0),
        u32::try_from(rect.height().max(0)).unwrap_or(0),
    )
}

fn product_rect(rect: ControlPixelRect) -> ProductPixelRect {
    ProductPixelRect {
        left: rect.origin.x,
        top: rect.origin.y,
        right: rect
            .origin
            .x
            .saturating_add(i32::try_from(rect.size.width).unwrap_or(i32::MAX)),
        bottom: rect
            .origin
            .y
            .saturating_add(i32::try_from(rect.size.height).unwrap_or(i32::MAX)),
    }
}

fn remote_control_specs() -> Vec<ControlSpec> {
    let button = |id, text, visible| control_spec(id, ControlKind::Button, text, visible);
    let edit = |id, multiline, visible| {
        control_spec(
            id,
            ControlKind::TextInput {
                multiline,
                password: false,
                vertical_scroll: multiline,
                want_return: multiline,
            },
            "",
            visible,
        )
    };
    vec![
        edit(EDIT_ID, true, true),
        button(SEND_ID, "Send", true),
        button(NEW_ID, "New", true),
        button(TABS_ID, "Tabs", true),
        button(CONTROL_CENTER_ID, "Control Center", true),
        button(SETTINGS_ID, "Settings", true),
        button(LOCALE_ID, "En|Zh", true),
        button(FONT_DECREASE_ID, "z", true),
        button(FONT_INCREASE_ID, "Z", true),
        edit(TAB_TITLE_EDIT_ID, false, false),
        edit(TAB_NOTE_EDIT_ID, false, false),
        button(TAB_SAVE_ID, "Save", false),
        button(TAB_CANCEL_ID, "Cancel", false),
        button(CLOSE_KEEP_ID, "Keep Server Running", false),
        button(CLOSE_STOP_ID, "Stop Server && Exit", false),
        button(CLOSE_CANCEL_ID, "Cancel", false),
        edit(SETTINGS_FONT_ID, false, false),
        edit(SETTINGS_SIZE_ID, false, false),
        button(SETTINGS_DARK_ID, "Dark", false),
        button(SETTINGS_LIGHT_ID, "Light", false),
        button(SETTINGS_APPLY_ID, "Apply", false),
        button(SETTINGS_CANCEL_ID, "Cancel", false),
        button(SETTINGS_DEFAULT_SCOPE_ID, "Default values", false),
        button(SETTINGS_CURRENT_SCOPE_ID, "Current terminal", false),
        button(SETTINGS_FONT_INHERIT_ID, "Inherit default", false),
        button(SETTINGS_SIZE_INHERIT_ID, "Inherit default", false),
        button(SETTINGS_THEME_INHERIT_ID, "Inherit default", false),
        button(SETTINGS_RESET_OVERRIDES_ID, "Reset overrides", false),
        button(TAB_CLOSE_CONFIRM_ID, "Terminate && Close", false),
        button(TAB_CLOSE_CANCEL_ID, "Cancel", false),
        button(NEW_DEFAULT_SHELL_ID, "Default", false),
        button(NEW_CMD_SHELL_ID, "Command Prompt", false),
        button(NEW_POWERSHELL_ID, "PowerShell", false),
        edit(NEW_INITIAL_COMMAND_ID, false, false),
        edit(NEW_HTTP_PROXY_ID, false, false),
        edit(NEW_HTTPS_PROXY_ID, false, false),
        button(NEW_CREATE_ID, "Create", false),
        button(NEW_CANCEL_ID, "Cancel", false),
    ]
}

pub(crate) fn run_remote_gui(no_activate: bool) -> Result<()> {
    let client_id = format!(
        "agenterm-gui:{}:{}",
        std::process::id(),
        crate::client::unix_time_ms()
    );
    let client = crate::frontend_server::connect_or_start_frontend_gui_client(&client_id)
        .map_err(anyhow::Error::msg)?;
    let title = format!(
        "AgenTerm-{}:{}",
        env!("CARGO_PKG_VERSION"),
        resolved_ipc_endpoint()?.logical_instance
    );
    let mut options = ControlWindowOptions::new(title, PixelSize::new(1180, 760));
    options.controls = remote_control_specs();
    options.system_menu = vec![
        SystemMenuItem {
            id: SYSTEM_MENU_TOGGLE_TABS_ID,
            text: "Toggle Tabs".to_owned(),
            enabled: true,
            checked: true,
            separator_before: true,
        },
        SystemMenuItem {
            id: SYSTEM_MENU_COPY_ID,
            text: "Copy\tCtrl+C".to_owned(),
            enabled: false,
            checked: false,
            separator_before: true,
        },
        SystemMenuItem {
            id: SYSTEM_MENU_PASTE_ID,
            text: "Paste\tCtrl+V".to_owned(),
            enabled: true,
            checked: false,
            separator_before: false,
        },
    ];
    options.no_activate = no_activate;
    run_control_window(
        options,
        Box::new(RemoteWindowApplication {
            state: None,
            client_id,
            client: Some(client),
            no_activate,
        }),
    )
    .map_err(|error| anyhow::anyhow!(error))
}

struct RemoteControls {
    edit: ControlId,
    send: ControlId,
    new_tab: ControlId,
    tabs_button: ControlId,
    control_center: ControlId,
    settings: ControlId,
    locale: ControlId,
    font_decrease: ControlId,
    font_increase: ControlId,
    tab_title_edit: ControlId,
    tab_note_edit: ControlId,
    tab_save: ControlId,
    tab_cancel: ControlId,
    close_keep: ControlId,
    close_stop: ControlId,
    close_cancel: ControlId,
    settings_font: ControlId,
    settings_size: ControlId,
    settings_dark: ControlId,
    settings_light: ControlId,
    settings_apply: ControlId,
    settings_cancel: ControlId,
    settings_default_scope: ControlId,
    settings_current_scope: ControlId,
    settings_font_inherit: ControlId,
    settings_size_inherit: ControlId,
    settings_theme_inherit: ControlId,
    settings_reset_overrides: ControlId,
    tab_close_confirm: ControlId,
    tab_close_cancel: ControlId,
    new_default_shell: ControlId,
    new_cmd_shell: ControlId,
    new_powershell: ControlId,
    new_initial_command: ControlId,
    new_http_proxy: ControlId,
    new_https_proxy: ControlId,
    new_create: ControlId,
    new_cancel: ControlId,
}

impl RemoteControls {
    const fn stable() -> Self {
        Self {
            edit: EDIT_ID,
            send: SEND_ID,
            new_tab: NEW_ID,
            tabs_button: TABS_ID,
            control_center: CONTROL_CENTER_ID,
            settings: SETTINGS_ID,
            locale: LOCALE_ID,
            font_decrease: FONT_DECREASE_ID,
            font_increase: FONT_INCREASE_ID,
            tab_title_edit: TAB_TITLE_EDIT_ID,
            tab_note_edit: TAB_NOTE_EDIT_ID,
            tab_save: TAB_SAVE_ID,
            tab_cancel: TAB_CANCEL_ID,
            close_keep: CLOSE_KEEP_ID,
            close_stop: CLOSE_STOP_ID,
            close_cancel: CLOSE_CANCEL_ID,
            settings_font: SETTINGS_FONT_ID,
            settings_size: SETTINGS_SIZE_ID,
            settings_dark: SETTINGS_DARK_ID,
            settings_light: SETTINGS_LIGHT_ID,
            settings_apply: SETTINGS_APPLY_ID,
            settings_cancel: SETTINGS_CANCEL_ID,
            settings_default_scope: SETTINGS_DEFAULT_SCOPE_ID,
            settings_current_scope: SETTINGS_CURRENT_SCOPE_ID,
            settings_font_inherit: SETTINGS_FONT_INHERIT_ID,
            settings_size_inherit: SETTINGS_SIZE_INHERIT_ID,
            settings_theme_inherit: SETTINGS_THEME_INHERIT_ID,
            settings_reset_overrides: SETTINGS_RESET_OVERRIDES_ID,
            tab_close_confirm: TAB_CLOSE_CONFIRM_ID,
            tab_close_cancel: TAB_CLOSE_CANCEL_ID,
            new_default_shell: NEW_DEFAULT_SHELL_ID,
            new_cmd_shell: NEW_CMD_SHELL_ID,
            new_powershell: NEW_POWERSHELL_ID,
            new_initial_command: NEW_INITIAL_COMMAND_ID,
            new_http_proxy: NEW_HTTP_PROXY_ID,
            new_https_proxy: NEW_HTTPS_PROXY_ID,
            new_create: NEW_CREATE_ID,
            new_cancel: NEW_CANCEL_ID,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RemoteFocusSurface {
    Terminal,
    Composer,
    Tabs,
}

impl RemoteFocusSurface {
    const fn to_shared(self) -> FocusSurface {
        match self {
            Self::Terminal => FocusSurface::Terminal,
            Self::Composer => FocusSurface::Composer,
            Self::Tabs => FocusSurface::Sidebar,
        }
    }

    const fn from_shared(surface: FocusSurface) -> Self {
        match surface {
            FocusSurface::Terminal => Self::Terminal,
            FocusSurface::Composer => Self::Composer,
            FocusSurface::Sidebar => Self::Tabs,
        }
    }
}

#[derive(Clone, Copy)]
enum RemoteTabAction {
    AddChild,
    Close,
}

#[derive(Clone)]
struct RemoteTreeRow {
    tab_index: usize,
    depth: usize,
    is_last: bool,
    guides: Vec<bool>,
    has_children: bool,
    collapsed: bool,
}

#[derive(Clone, Debug)]
struct RemoteTerminalSelection {
    tab_id: String,
    rows: u32,
    columns: u32,
    gesture: RemoteSelectionGesture,
    cached_text: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RemoteSidebarTextClick {
    tab_id: String,
    geometry_generation: u64,
}

impl RemoteSidebarTextClick {
    fn matches(&self, tab_id: &str, geometry_generation: u64) -> bool {
        self.tab_id == tab_id && self.geometry_generation == geometry_generation
    }
}

#[derive(Clone, Debug)]
struct RecentRemoteTerminalClick {
    tab_id: String,
    point: RemotePoint,
    at: Instant,
}

#[derive(Clone, Debug)]
struct RemoteTerminalDoubleClick {
    tab_id: String,
    point: RemotePoint,
    expires_at: Instant,
}

impl RemoteTerminalSelection {
    fn bounds(&self) -> (RemotePoint, RemotePoint) {
        self.gesture.bounds()
    }

    fn is_empty(&self) -> bool {
        self.gesture.is_empty()
    }

    fn active_gesture(&self) -> bool {
        self.gesture.active()
    }

    fn phase(&self) -> SelectionGesturePhase {
        self.gesture.phase()
    }

    fn drag_to(&mut self, point: RemotePoint) {
        self.gesture = self.gesture.clone().drag_to(point);
    }

    fn complete(&mut self) -> bool {
        let updated = self.gesture.clone().complete();
        let completed = updated.phase() == SelectionGesturePhase::Completed;
        self.gesture = updated;
        completed
    }

    fn cancel(&mut self) {
        self.gesture = self.gesture.clone().cancel();
    }

    fn can_copy(&self) -> bool {
        self.phase() == SelectionGesturePhase::Completed
            && !self.is_empty()
            && self
                .cached_text
                .as_ref()
                .is_some_and(|text| !text.is_empty())
    }

    fn matches_screen(&self, screen: &UiScreenSnapshot) -> bool {
        self.tab_id == screen.tab_id && self.rows == screen.rows && self.columns == screen.columns
    }
}
fn selection_claims_copy_shortcut(selection: Option<&RemoteTerminalSelection>) -> bool {
    selection.is_some_and(RemoteTerminalSelection::can_copy)
}

fn terminal_selection_highlight_rects(
    selection: &RemoteTerminalSelection,
    screen: &UiScreenSnapshot,
    terminal: ProductPixelRect,
    cell_width: i32,
    cell_height: i32,
) -> Vec<ProductPixelRect> {
    if !selection.matches_screen(screen) || screen.rows == 0 || screen.columns == 0 {
        return Vec::new();
    }
    let (start, end) = selection.bounds();
    (start.row..=end.row.min(screen.rows.saturating_sub(1)))
        .map(|row| {
            let first = if row == start.row { start.column } else { 0 }
                .min(screen.columns.saturating_sub(1));
            let last = if row == end.row {
                end.column
            } else {
                screen.columns.saturating_sub(1)
            }
            .min(screen.columns.saturating_sub(1));
            ProductPixelRect {
                left: terminal.left + i32::try_from(first).unwrap_or_default() * cell_width,
                top: terminal.top + i32::try_from(row).unwrap_or_default() * cell_height,
                right: terminal.left
                    + i32::try_from(last.saturating_add(1)).unwrap_or_default() * cell_width,
                bottom: terminal.top
                    + i32::try_from(row.saturating_add(1)).unwrap_or_default() * cell_height,
            }
        })
        .collect()
}

fn remote_composer_identity(
    client: &UiClientModel,
) -> Option<(String, Option<String>, bool, usize)> {
    let tab_id = client.snapshot().active_tab_id.clone()?;
    let composer = &tab_by_id(client.snapshot(), &tab_id)?.composer;
    Some((
        tab_id,
        composer.text.clone(),
        composer.sensitive,
        composer.byte_length,
    ))
}

struct RemoteWindowState {
    window: ControlWindow,
    edit: ControlId,
    send: ControlId,
    new_tab: ControlId,
    tabs_button: ControlId,
    control_center: ControlId,
    settings: ControlId,
    locale: ControlId,
    font_decrease: ControlId,
    font_increase: ControlId,
    tab_title_edit: ControlId,
    tab_note_edit: ControlId,
    tab_save: ControlId,
    tab_cancel: ControlId,
    close_keep: ControlId,
    close_stop: ControlId,
    close_cancel: ControlId,
    settings_font: ControlId,
    settings_size: ControlId,
    settings_dark: ControlId,
    settings_light: ControlId,
    settings_apply: ControlId,
    settings_cancel: ControlId,
    settings_default_scope: ControlId,
    settings_current_scope: ControlId,
    settings_font_inherit: ControlId,
    settings_size_inherit: ControlId,
    settings_theme_inherit: ControlId,
    settings_reset_overrides: ControlId,
    tab_close_confirm: ControlId,
    tab_close_cancel: ControlId,
    new_default_shell: ControlId,
    new_cmd_shell: ControlId,
    new_powershell: ControlId,
    new_initial_command: ControlId,
    new_http_proxy: ControlId,
    new_https_proxy: ControlId,
    new_create: ControlId,
    new_cancel: ControlId,
    client_id: String,
    client: Option<UiClientModel>,
    reconnect_after: Instant,
    server_recovery: crate::frontend_server::FrontendServerRecoveryState,
    last_message: Option<String>,
    last_error: Option<String>,
    tabs_visible: bool,
    config: AppConfig,
    font: agenterm_platform::font::NativeFont,
    cell_width: i32,
    cell_height: i32,
    terminal_text_decoder: input::Utf16TextDecoder,
    last_active_id: Option<String>,
    last_composer_identity: Option<(String, Option<String>, bool, usize)>,
    pending_terminal_resize: Option<RemoteTerminalResize>,
    terminal_resize_worker: RemoteTerminalResizeWorker,
    pending_terminal_paste: Option<RemoteTerminalPasteRequest>,
    terminal_paste_worker: RemoteTerminalPasteWorker,
    tabs_resize_dragging: bool,
    tab_editor_dialog: TabEditorDialog,
    window_close_dialog: WindowCloseDialog,
    new_terminal_dialog: NewTerminalDialog,
    settings_dialog: SettingsDialog,
    terminal_selection: Option<RemoteTerminalSelection>,
    terminal_selection_pointer: Option<(i32, i32)>,
    terminal_selection_autoscroll: Option<AutoScrollStep>,
    pointer_modifiers: input::ModifierState,
    mouse_report_button: Option<u8>,
    mouse_report_cell: Option<(u16, u16)>,
    wheel_accumulator: WheelAccumulator,
    scroll_drag: Option<ScrollbarThumbDrag>,
    sidebar_scroll_offset: usize,
    sidebar_scroll_drag: Option<ScrollbarThumbDrag>,
    sidebar_geometry_generation: u64,
    recent_sidebar_text_click: Option<RemoteSidebarTextClick>,
    recent_terminal_click: Option<RecentRemoteTerminalClick>,
    terminal_double_click: Option<RemoteTerminalDoubleClick>,
    focus_surface: RemoteFocusSurface,
    focus_state: FocusState,
    close_confirmation: CloseConfirmation,
    cwd_editor_dialog: CwdEditorDialog,
    last_published_snapshot: Option<String>,
    render_activity_sample: Option<ControlWindowRenderActivity>,
    render_activity_sample_sequence: u64,
    relay_close_after_completion: Option<WindowCloseChoice>,
    no_activate: bool,
}

impl RemoteWindowState {
    fn set_control_text(&self, control: ControlId, text: &str) {
        if let Err(error) = self.window.set_control_text(control, text) {
            eprintln!("AgenTerm control text update failed: {error}");
        }
    }

    fn control_text(&self, control: ControlId) -> String {
        self.window.control_text(control).unwrap_or_default()
    }

    fn set_control_bounds(&self, control: ControlId, bounds: ProductPixelRect) {
        if let Err(error) = self
            .window
            .set_control_bounds(control, control_rect(bounds))
        {
            eprintln!("AgenTerm control layout failed: {error}");
        }
    }

    fn set_control_visible(&self, control: ControlId, visible: bool) {
        if let Err(error) = self.window.set_control_visible(control, visible) {
            eprintln!("AgenTerm control visibility update failed: {error}");
        }
    }

    fn set_control_enabled(&self, control: ControlId, enabled: bool) {
        if let Err(error) = self.window.set_control_enabled(control, enabled) {
            eprintln!("AgenTerm control state update failed: {error}");
        }
    }

    fn focus_control(&self, control: ControlId) {
        if let Err(error) = self.window.focus_control(control) {
            eprintln!("AgenTerm control focus failed: {error}");
        }
    }

    fn new(
        window: ControlWindow,
        controls: RemoteControls,
        client_id: String,
        client: UiClientModel,
        no_activate: bool,
    ) -> Result<Self> {
        let RemoteControls {
            edit,
            send,
            new_tab,
            tabs_button,
            control_center,
            settings,
            locale,
            font_decrease,
            font_increase,
            tab_title_edit,
            tab_note_edit,
            tab_save,
            tab_cancel,
            close_keep,
            close_stop,
            close_cancel,
            settings_font,
            settings_size,
            settings_dark,
            settings_light,
            settings_apply,
            settings_cancel,
            settings_default_scope,
            settings_current_scope,
            settings_font_inherit,
            settings_size_inherit,
            settings_theme_inherit,
            settings_reset_overrides,
            tab_close_confirm,
            tab_close_cancel,
            new_default_shell,
            new_cmd_shell,
            new_powershell,
            new_initial_command,
            new_http_proxy,
            new_https_proxy,
            new_create,
            new_cancel,
        } = controls;
        let config = load_config();
        let settings_default_draft = config.effective_terminal_appearance(&ipc_address(), None);

        let last_active_id = client.snapshot().active_tab_id.clone();
        let appearance =
            config.effective_terminal_appearance(&ipc_address(), last_active_id.as_deref());
        let (font, cell_width, cell_height) = create_terminal_font(
            &window,
            &appearance.terminal_font_family,
            appearance.terminal_font_size,
        )?;
        let last_composer_identity = remote_composer_identity(&client);
        let terminal_resize_worker = RemoteTerminalResizeWorker::spawn()?;
        let terminal_paste_worker = RemoteTerminalPasteWorker::spawn()?;
        Ok(Self {
            window,
            edit,
            send,
            new_tab,
            tabs_button,
            control_center,
            settings,
            locale,
            font_decrease,
            font_increase,
            tab_title_edit,
            tab_note_edit,
            tab_save,
            tab_cancel,
            close_keep,
            close_stop,
            close_cancel,
            settings_font,
            settings_size,
            settings_dark,
            settings_light,
            settings_apply,
            settings_cancel,
            settings_default_scope,
            settings_current_scope,
            settings_font_inherit,
            settings_size_inherit,
            settings_theme_inherit,
            settings_reset_overrides,
            tab_close_confirm,
            tab_close_cancel,
            new_default_shell,
            new_cmd_shell,
            new_powershell,
            new_initial_command,
            new_http_proxy,
            new_https_proxy,
            new_create,
            new_cancel,
            client_id,
            client: Some(client),
            reconnect_after: Instant::now(),
            server_recovery: crate::frontend_server::FrontendServerRecoveryState::new(
                Instant::now(),
            ),
            last_message: None,
            last_error: None,
            tabs_visible: config.tabs_visible,
            config,
            font,
            cell_width,
            cell_height,
            terminal_text_decoder: input::Utf16TextDecoder::default(),
            last_active_id,
            last_composer_identity,
            pending_terminal_resize: None,
            terminal_resize_worker,
            pending_terminal_paste: None,
            terminal_paste_worker,
            tabs_resize_dragging: false,
            tab_editor_dialog: TabEditorDialog::new(),
            window_close_dialog: WindowCloseDialog::new(),
            new_terminal_dialog: NewTerminalDialog::new(),
            settings_dialog: SettingsDialog::new(settings_default_draft),
            terminal_selection: None,
            terminal_selection_pointer: None,
            terminal_selection_autoscroll: None,
            pointer_modifiers: input::ModifierState::empty(),
            mouse_report_button: None,
            mouse_report_cell: None,
            wheel_accumulator: WheelAccumulator::default(),
            scroll_drag: None,
            sidebar_scroll_offset: 0,
            sidebar_scroll_drag: None,
            sidebar_geometry_generation: 0,
            recent_sidebar_text_click: None,
            recent_terminal_click: None,
            terminal_double_click: None,
            focus_surface: RemoteFocusSurface::Terminal,
            focus_state: FocusState::new(FocusSurface::Terminal, FocusTransitionGate::default()),
            close_confirmation: CloseConfirmation::new(),
            cwd_editor_dialog: CwdEditorDialog::new(),
            last_published_snapshot: None,
            render_activity_sample: None,
            render_activity_sample_sequence: 0,
            relay_close_after_completion: None,
            no_activate,
        })
    }

    fn tick(&mut self) -> bool {
        let resize_changed = self.process_terminal_resize_results();
        let paste_changed = self.process_terminal_paste_results();
        let result = self
            .client
            .as_mut()
            .context("replaceable UI is disconnected")
            .and_then(|client| {
                client.maintain_lease_if_due()?;
                client.poll_deltas()
            });
        match result {
            Ok(changed) => {
                self.reconcile_pending_terminal_resize();
                self.reconcile_tab_editor();
                self.reconcile_terminal_selection();
                let autoscroll_changed = self.tick_terminal_selection_autoscroll();
                self.reconcile_tab_close();
                self.reconcile_cwd_editor();
                let active = self
                    .client
                    .as_ref()
                    .and_then(|client| client.snapshot().active_tab_id.clone());
                let composer = self.client.as_ref().and_then(remote_composer_identity);
                if active != self.last_active_id {
                    self.invalidate_sidebar_text_click();
                    if self.tab_editor_dialog.is_open()
                        && self.tab_editor_dialog.target() != active.as_deref()
                    {
                        self.finish_tab_edit(false);
                    }
                }
                if active != self.last_active_id || composer != self.last_composer_identity {
                    self.last_active_id = active;
                    self.last_composer_identity = composer;
                    if let Err(error) = self.apply_effective_terminal_font() {
                        self.last_error = Some(format!("Terminal font update failed: {error:#}"));
                    }
                    self.load_composer();
                }
                let command_changed = match self.process_client_command() {
                    Ok(changed) => {
                        if changed {
                            self.last_error = None;
                        }
                        changed
                    }
                    Err(error) => {
                        self.last_error =
                            Some(format!("UI client command relay failed: {error:#}"));
                        true
                    }
                };
                match self.publish_ui_snapshot() {
                    Ok(published) => {
                        resize_changed
                            || paste_changed
                            || changed
                            || command_changed
                            || autoscroll_changed
                            || published
                    }
                    Err(error) => {
                        self.last_error =
                            Some(format!("UI snapshot publication failed: {error:#}"));
                        true
                    }
                }
            }
            Err(error) => {
                let disconnected_server_pid = self.client.as_ref().map(UiClientModel::server_pid);
                // A failed poll invalidates this causal client projection. Do not
                // keep rendering it as connected or accept input against stale
                // server-owned state while recovery is in progress.
                self.client = None;
                self.pending_terminal_resize = None;
                self.pending_terminal_paste = None;
                let server_address = ipc_address();
                self.server_recovery
                    .on_disconnected(&server_address, disconnected_server_pid);
                self.show_workspace_controls(false);
                self.show_tab_editor(false);
                self.show_tab_close_controls(false);
                self.last_error = Some(format!("{error:#}"));
                if Instant::now() >= self.reconnect_after {
                    self.reconnect_after = Instant::now() + RECONNECT_INTERVAL;
                    match UiClientModel::connect(self.client_id.clone()) {
                        Ok(client) => {
                            self.client = Some(client);
                            self.server_recovery.on_reconnected(Instant::now());
                            self.last_published_snapshot = None;
                            self.tab_editor_dialog.close();
                            self.terminal_selection = None;
                            self.close_confirmation.close();
                            self.cwd_editor_dialog.close();
                            self.show_tab_editor(false);
                            self.show_tab_close_controls(false);
                            self.apply_locale();
                            self.show_workspace_controls(true);
                            self.last_active_id = self
                                .client
                                .as_ref()
                                .and_then(|client| client.snapshot().active_tab_id.clone());
                            self.last_composer_identity =
                                self.client.as_ref().and_then(remote_composer_identity);
                            self.last_error = None;
                            if let Err(error) = self.apply_effective_terminal_font() {
                                self.last_error =
                                    Some(format!("Terminal font update failed: {error:#}"));
                            }
                            self.load_composer();
                            self.resize_active_terminal();
                            return true;
                        }
                        Err(reconnect_error) => {
                            let now = Instant::now();
                            let recovery = self.server_recovery.maybe_recover(now);
                            let recovery_message = match recovery {
                                crate::frontend_server::FrontendServerRecovery::NoAction => None,
                                crate::frontend_server::FrontendServerRecovery::Started => {
                                    Some("Server disappeared; recovery server started".to_owned())
                                }
                                crate::frontend_server::FrontendServerRecovery::Failed(error) => {
                                    Some(format!("Server recovery failed: {error:#}"))
                                }
                            };
                            self.last_error =
                                Some(recovery_message.unwrap_or_else(|| {
                                    format!("Disconnected: {reconnect_error:#}")
                                }));
                        }
                    }
                }
                true
            }
        }
    }

    fn process_client_command(&mut self) -> Result<bool> {
        let Some(command) = self
            .client
            .as_mut()
            .context("replaceable UI is disconnected")?
            .poll_client_command()?
        else {
            return Ok(false);
        };
        let outcome = self.execute_client_command(&command);
        let close_after_completion = outcome
            .as_ref()
            .ok()
            .and_then(|_| self.relay_close_after_completion.take());
        let response = match outcome {
            Ok(Some(output)) => IpcResponse::success(output),
            Ok(None) if close_after_completion.is_some() => {
                IpcResponse::success(self.detached_ui_snapshot_json()?)
            }
            Ok(None) => IpcResponse::success(self.ui_snapshot_json()?),
            Err(error) => {
                let error = format!("{error:#}");
                self.last_error = Some(error.clone());
                IpcResponse::typed_failure(error, "ui_client_command_failed", "command", false)
            }
        };
        self.client
            .as_mut()
            .context("replaceable UI is disconnected")?
            .complete_client_command(
                &command.command_id,
                &response,
                close_after_completion.is_some(),
                matches!(
                    close_after_completion,
                    Some(WindowCloseChoice::StopServerAndExit)
                ),
            )?;
        if let Some(choice) = close_after_completion {
            self.window_close_dialog.close();
            match choice {
                WindowCloseChoice::KeepServerRunning | WindowCloseChoice::StopServerAndExit => {
                    self.window.close();
                }
                WindowCloseChoice::Cancel => {}
            }
            return Ok(true);
        }
        if response.ok
            && serde_json::from_str::<serde_json::Value>(&response.output)
                .ok()
                .is_some_and(|value| {
                    value["projection"].as_str() == Some(PROJECTION_REPLACEABLE_UI_CLIENT)
                })
        {
            self.last_published_snapshot = Some(response.output);
        }
        Ok(true)
    }

    fn execute_client_command(&mut self, command: &UiClientCommand) -> Result<Option<String>> {
        if command.args.first().map(String::as_str) != Some("ui-action") {
            return self.execute_client_local_command(command);
        }
        let action = command
            .args
            .get(1)
            .map(String::as_str)
            .context("ui-action requires an action")?;
        match action {
            "new-tab" => {
                self.sync_composer()?;
                self.apply_client_command(&command.command_id)?;
                self.last_active_id = self
                    .client
                    .as_ref()
                    .and_then(|client| client.snapshot().active_tab_id.clone());
                self.load_composer();
                self.resize_active_terminal();
            }
            "new-child" => {
                self.sync_composer()?;
                self.apply_client_command(&command.command_id)?;
                self.last_active_id = self
                    .client
                    .as_ref()
                    .and_then(|client| client.snapshot().active_tab_id.clone());
                self.load_composer();
                self.resize_active_terminal();
                if let Some(tab) = self.active_tab().cloned() {
                    self.begin_tab_edit(tab);
                }
            }
            "open-control-center" => {
                crate::control_center::open_control_center(
                    self.no_activate,
                    &crate::client::ipc_address(),
                )?;
                self.last_message = Some("Control Center opened".to_owned());
            }
            "select-tab" => {
                self.invalidate_sidebar_text_click();
                self.finish_tab_edit(false);
                self.sync_composer()?;
                self.apply_client_command(&command.command_id)?;
                self.set_focus_surface_unchecked(RemoteFocusSurface::Terminal);
                self.last_active_id = self
                    .client
                    .as_ref()
                    .and_then(|client| client.snapshot().active_tab_id.clone());
                self.load_composer();
                self.resize_active_terminal();
                self.window.focus();
            }
            "toggle-tree" => {
                self.apply_client_command(&command.command_id)?;
                self.reconcile_tab_editor();
            }
            "composer-send" => {
                self.sync_composer()?;
                self.apply_client_command(&command.command_id)?;
                self.set_control_text(self.edit, "");
            }
            "tabs-show" => self.set_tabs_visible(true),
            "tabs-hide" => self.set_tabs_visible(false),
            "tabs-toggle" | "toggle-tabs" => self.toggle_tabs(),
            "tabs-set-width" => {
                let width = option_value(&command.args, "--width")
                    .and_then(|value| value.parse::<i32>().ok())
                    .context("tabs-set-width requires numeric --width")?;
                self.config.tabs_width = clamp_tabs_width(width);
                save_config(&self.config).context("could not save Tabs width")?;
                self.layout();
                self.resize_active_terminal();
            }
            "edit-tab" => {
                let tab = self.command_target_tab(&command.args)?;
                self.begin_tab_edit(tab);
            }
            "tab-editor-save" => {
                if !self.tab_editor_dialog.is_open() {
                    anyhow::bail!("no tab editor is open");
                }
                self.finish_tab_edit(true);
                if self.tab_editor_dialog.is_open() {
                    anyhow::bail!(
                        "{}",
                        self.last_error
                            .clone()
                            .unwrap_or_else(|| "tab edit could not be saved".to_owned())
                    );
                }
            }
            "tab-editor-cancel" => {
                if !self.tab_editor_dialog.is_open() {
                    anyhow::bail!("no tab editor is open");
                }
                self.finish_tab_edit(false);
            }
            "open-settings" => {
                self.open_settings();
                if !self.settings_dialog.is_open() {
                    anyhow::bail!("Settings could not be opened");
                }
            }
            "toggle-locale" => self.toggle_locale(),
            "font-decrease" => self.adjust_active_terminal_font(-1),
            "font-increase" => self.adjust_active_terminal_font(1),
            "settings-defaults" => {
                if !self.settings_dialog.is_open() {
                    anyhow::bail!("Settings is not open");
                }
                self.switch_settings_scope(SettingsScope::Defaults);
            }
            "settings-current" => {
                if !self.settings_dialog.is_open() {
                    anyhow::bail!("Settings is not open");
                }
                self.switch_settings_scope(SettingsScope::CurrentTerminal);
            }
            "settings-font-toggle" => {
                if !self.settings_dialog.is_open() {
                    anyhow::bail!("Settings is not open");
                }
                self.toggle_settings_inheritance(AppearanceField::FontFamily);
            }
            "settings-size-toggle" => {
                if !self.settings_dialog.is_open() {
                    anyhow::bail!("Settings is not open");
                }
                self.toggle_settings_inheritance(AppearanceField::FontSize);
            }
            "settings-theme-toggle" => {
                if !self.settings_dialog.is_open() {
                    anyhow::bail!("Settings is not open");
                }
                self.toggle_settings_inheritance(AppearanceField::Theme);
            }
            "settings-reset-overrides" => {
                if !self.settings_dialog.is_open() {
                    anyhow::bail!("Settings is not open");
                }
                self.reset_settings_overrides();
            }
            "settings-theme-dark" => {
                if !self.settings_dialog.is_open() {
                    anyhow::bail!("Settings is not open");
                }
                self.preview_settings_theme(ThemeId::Dark);
            }
            "settings-theme-light" => {
                if !self.settings_dialog.is_open() {
                    anyhow::bail!("Settings is not open");
                }
                self.preview_settings_theme(ThemeId::Light);
            }
            "settings-apply" => {
                if !self.settings_dialog.is_open() {
                    anyhow::bail!("Settings is not open");
                }
                self.finish_settings(true);
                if self.settings_dialog.is_open() {
                    anyhow::bail!(
                        "{}",
                        self.last_error
                            .clone()
                            .unwrap_or_else(|| "Settings could not be applied".to_owned())
                    );
                }
            }
            "open-cwd-editor" => {
                if let Some(target) = option_value(&command.args, "-t")
                    && self.active_tab().is_none_or(|tab| tab.id != target)
                {
                    self.client
                        .as_mut()
                        .context("UI is disconnected")?
                        .select_tab(target)?;
                    self.client
                        .as_mut()
                        .context("UI is disconnected")?
                        .poll_deltas()?;
                }
                self.open_cwd_editor();
                if !self.cwd_editor_dialog.is_open() {
                    anyhow::bail!("CWD editor could not be opened");
                }
            }
            "window-activate" => {
                self.window.focus();
            }
            "terminal-paste" => {
                self.paste_terminal_clipboard()?;
            }
            "cwd-prepare" | "cwd-prepare-append" | "cwd-prepare-replace" | "cwd-send-now" => {
                let mut direct = vec![action.to_owned()];
                direct.extend(command.args.iter().skip(2).cloned());
                self.client
                    .as_mut()
                    .context("UI is disconnected")?
                    .run_control(direct)?;
                self.client
                    .as_mut()
                    .context("UI is disconnected")?
                    .poll_deltas()?;
            }
            "close-tab" => {
                let tab = self.command_target_tab(&command.args)?;
                if tab.dead {
                    if !self.close_tab_now(tab.id) {
                        anyhow::bail!("dead tab could not be closed");
                    }
                } else {
                    self.sync_composer()?;
                    self.finish_tab_edit(false);
                    self.close_confirmation.open(tab.id);
                    self.show_workspace_controls(false);
                    self.layout_tab_close_controls();
                    self.window.focus();
                }
            }
            "confirm" => {
                if !self.close_confirmation.is_open() {
                    anyhow::bail!("no confirmation is pending");
                }
                self.finish_close_tab(true);
                if self.close_confirmation.is_open() {
                    anyhow::bail!("tab close could not be confirmed");
                }
            }
            "cancel" => {
                if self.window_close_dialog.is_open() {
                    self.finish_window_close(WindowCloseChoice::Cancel);
                } else if self.new_terminal_dialog.is_open() {
                    self.finish_new_terminal(false);
                } else if self.settings_dialog.is_open() {
                    self.finish_settings(false);
                } else if self.cwd_editor_dialog.is_open() {
                    self.finish_cwd_editor(false, ComposerWriteMode::EmptyOnly);
                } else if self.close_confirmation.is_open() {
                    self.finish_close_tab(false);
                } else if self.tab_editor_dialog.is_open() {
                    self.finish_tab_edit(false);
                } else {
                    anyhow::bail!("no modal is pending");
                }
            }
            "copy-selection" => self.copy_terminal_selection()?,
            "close-window" => {
                self.request_window_close();
                if !self.window_close_dialog.is_open() {
                    anyhow::bail!("window-close confirmation could not be opened");
                }
            }
            "window-minimize" => self.window.set_presentation(WindowPresentation::Minimized),
            "window-maximize" => self.window.set_presentation(WindowPresentation::Maximized),
            "window-restore" => self.window.set_presentation(WindowPresentation::Restored),
            "window-resize" => {
                let size = ClientSize::parse(
                    option_value(&command.args, "--width"),
                    option_value(&command.args, "--height"),
                )
                .map_err(|error| anyhow::anyhow!("{} ({})", error.message(), error.code()))?;
                self.window
                    .set_client_size(PixelSize::new(size.width, size.height))
                    .map_err(|error| anyhow::anyhow!(error))?;
            }
            "keep-server-running" => {
                if !self.window_close_dialog.is_open() {
                    anyhow::bail!("no window-close confirmation is pending");
                }
                self.relay_close_after_completion = Some(WindowCloseChoice::KeepServerRunning);
            }
            "stop-server-and-exit" => {
                if !self.window_close_dialog.is_open() {
                    anyhow::bail!("no window-close confirmation is pending");
                }
                self.relay_close_after_completion = Some(WindowCloseChoice::StopServerAndExit);
            }
            action if action.starts_with("proxy-") || action == "open-proxy-editor" => {
                anyhow::bail!("proxy workbench controls are archived")
            }
            other => anyhow::bail!("unknown UI action: {other}"),
        }
        self.window.request_redraw();
        Ok(None)
    }

    fn execute_client_local_command(
        &mut self,
        command: &UiClientCommand,
    ) -> Result<Option<String>> {
        let name = command
            .args
            .first()
            .map(String::as_str)
            .context("relayed UI command is empty")?;
        let mut output = None;
        match name {
            "set-composer" => {
                let target = option_value(&command.args, "-t")
                    .or_else(|| self.active_tab().map(|tab| tab.id.as_str()))
                    .context("set-composer requires an active tab")?;
                if self.tab_editor_dialog.target() != Some(target) {
                    anyhow::bail!("set-composer target is not open in the inline tab editor");
                }
                let text = positional_values(&command.args, &["-t"], &[]).join(" ");
                let normalized = text.replace("\r\n", "\n");
                let (title, note) = normalized
                    .split_once('\n')
                    .unwrap_or((normalized.as_str(), ""));
                self.set_control_text(self.tab_title_edit, title);
                self.set_control_text(self.tab_note_edit, note);
                self.sync_tab_editor_drafts();
                self.focus_control(self.tab_title_edit);
            }
            "focus" => {
                self.apply_client_command(&command.command_id)?;
                let surface = command
                    .args
                    .get(1)
                    .map(String::as_str)
                    .unwrap_or("terminal");
                let target = match surface {
                    "terminal" => RemoteFocusSurface::Terminal,
                    "composer" => RemoteFocusSurface::Composer,
                    "tabs" | "sidebar" => RemoteFocusSurface::Tabs,
                    other => anyhow::bail!("unknown focus surface: {other}"),
                };
                if !self.set_focus_surface(target) {
                    anyhow::bail!("focus surface is unavailable: {surface}");
                }
            }
            "get-settings" => {
                let active_tab_id = self
                    .client
                    .as_ref()
                    .and_then(|client| client.snapshot().active_tab_id.as_deref());
                let terminal_override = active_tab_id.and_then(|tab_id| {
                    self.config
                        .terminal_override_entry(&ipc_address(), tab_id)
                        .cloned()
                });
                let effective = self
                    .config
                    .effective_terminal_appearance(&ipc_address(), active_tab_id);
                output = Some(
                    serde_json::to_string_pretty(&serde_json::json!({
                        "terminal_font_family": self.config.terminal_font_family,
                        "terminal_font_size": self.config.terminal_font_size,
                        "color_theme": self.config.color_theme.as_str(),
                        "locale": self.config.locale.as_str(),
                        "active_tab_id": active_tab_id,
                        "current_terminal_override": terminal_override,
                        "effective": {
                            "terminal_font_family": effective.terminal_font_family,
                            "terminal_font_size": effective.terminal_font_size,
                            "color_theme": effective.color_theme.as_str(),
                        },
                        "resolved_font_family": effective.terminal_font_family,
                        "config_path": config_path(),
                        "recommended_cjk_font": "Sarasa Fixed SC",
                        "recommended_font_license": "SIL Open Font License 1.1",
                    }))
                    .context("could not encode Settings")?,
                );
            }
            "set-setting" => {
                let key = command
                    .args
                    .get(1)
                    .map(String::as_str)
                    .context("set-setting requires a key")?;
                let value = command.args.get(2..).unwrap_or_default().join(" ");
                let mut next = self.config.clone();
                match key {
                    "terminal.font-family" if !value.trim().is_empty() => {
                        next.terminal_font_family = value;
                    }
                    "terminal.font-family" => anyhow::bail!("font family cannot be empty"),
                    "terminal.font-size" => {
                        let size = value
                            .parse::<u16>()
                            .context("font size must be a number from 8 to 36")?;
                        if !(8..=36).contains(&size) {
                            anyhow::bail!("font size must be from 8 to 36");
                        }
                        next.terminal_font_size = size;
                    }
                    other => anyhow::bail!("unknown setting: {other}"),
                }
                let appearance = next.effective_terminal_appearance(
                    &ipc_address(),
                    self.client
                        .as_ref()
                        .and_then(|client| client.snapshot().active_tab_id.as_deref()),
                );
                let (font, cell_width, cell_height) = create_terminal_font(
                    &self.window,
                    &appearance.terminal_font_family,
                    appearance.terminal_font_size,
                )?;
                save_config(&next).context("could not save settings")?;
                self.font = font;
                self.cell_width = cell_width;
                self.cell_height = cell_height;
                self.config = next;
                self.layout();
                self.resize_active_terminal();
            }
            "screenshot" => {
                let path = screenshot_output_path(&command.args, "agenterm-window");
                self.window.request_redraw();
                self.window
                    .capture_png(
                        &path,
                        agenterm_platform::screenshot::NativeCaptureArea::Window,
                    )
                    .map_err(|error| anyhow::anyhow!(error))?;
                output = Some(path.display().to_string());
            }
            "screenshot-pane" | "screenshot-tab" => {
                if let Some(target) = option_value(&command.args, "-t") {
                    let stable = self.resolve_stable_tab_target(target)?;
                    if self.active_tab().is_none_or(|tab| tab.id != stable) {
                        self.client
                            .as_mut()
                            .context("UI is disconnected")?
                            .select_tab(&stable)?;
                        self.client
                            .as_mut()
                            .context("UI is disconnected")?
                            .poll_deltas()?;
                        self.load_composer();
                        self.resize_active_terminal();
                    }
                }
                let path = screenshot_output_path(&command.args, "agenterm-pane");
                let terminal = self.workspace_geometry().terminal;
                self.window.request_redraw();
                self.window
                    .capture_png(
                        &path,
                        agenterm_platform::screenshot::NativeCaptureArea::Client {
                            left: terminal.left,
                            top: terminal.top,
                            width: terminal.width(),
                            height: terminal.height(),
                        },
                    )
                    .map_err(|error| anyhow::anyhow!(error))?;
                output = Some(path.display().to_string());
            }
            UI_CLIENT_COMMAND_FOCUS => {
                self.window.set_presentation(WindowPresentation::Restored);
                self.window.focus();
            }
            UI_CLIENT_COMMAND_SHOW_NO_ACTIVATE => self.window.show_without_activation(),
            other => anyhow::bail!("unsupported relayed UI command: {other}"),
        }
        self.window.request_redraw();
        Ok(output)
    }

    fn resolve_stable_tab_target(&mut self, target: &str) -> Result<String> {
        if target.starts_with('@') {
            return Ok(target.to_owned());
        }
        let response = self
            .client
            .as_mut()
            .context("UI is disconnected")?
            .run_control(vec![
                "display-message".to_owned(),
                "-p".to_owned(),
                "-t".to_owned(),
                target.to_owned(),
                "#{window_id}".to_owned(),
            ])?;
        let stable = response.output.trim();
        if !stable.starts_with('@') {
            anyhow::bail!("can't resolve tab target: {target}");
        }
        Ok(stable.to_owned())
    }

    fn apply_client_command(&mut self, command_id: &str) -> Result<()> {
        self.client
            .as_mut()
            .context("UI is disconnected")?
            .apply_client_command(command_id)?;
        self.client
            .as_mut()
            .context("UI is disconnected")?
            .poll_deltas()?;
        Ok(())
    }

    fn detached_ui_snapshot_json(&mut self) -> Result<String> {
        let mut value: serde_json::Value = serde_json::from_str(&self.ui_snapshot_json()?)
            .context("could not decode replaceable UI snapshot")?;
        value["window"]["visible"] = serde_json::Value::Bool(false);
        value["window"]["detached"] = serde_json::Value::Bool(true);
        value["layout"]["composer"]["visible"] = serde_json::Value::Bool(false);
        value["layout"]["composer"]["input_visible"] = serde_json::Value::Bool(false);
        value["layout"]["composer"]["send_visible"] = serde_json::Value::Bool(false);
        value["modal"] = serde_json::Value::Null;
        value["tab_editor"] = serde_json::Value::Null;
        value["focus"]["surface"] = serde_json::Value::String("terminal".to_owned());
        serde_json::to_string_pretty(&value)
            .context("could not encode detached replaceable UI snapshot")
    }

    fn command_target_tab(&self, args: &[String]) -> Result<UiTabBootstrap> {
        let client = self
            .client
            .as_ref()
            .context("replaceable UI is disconnected")?;
        let target = option_value(args, "-t")
            .or(client.snapshot().active_tab_id.as_deref())
            .context("no tab is active")?;
        client
            .snapshot()
            .tabs
            .iter()
            .find(|tab| tab.id == target)
            .cloned()
            .with_context(|| format!("can't find stable tab: {target}"))
    }

    fn active_tab(&self) -> Option<&UiTabBootstrap> {
        let client = self.client.as_ref()?;
        let active = client.snapshot().active_tab_id.as_deref()?;
        tab_by_id(client.snapshot(), active)
    }

    fn reconcile_tab_editor(&mut self) {
        let still_exists = self.tab_editor_dialog.target().is_some_and(|id| {
            self.client
                .as_ref()
                .is_some_and(|client| client.snapshot().tabs.iter().any(|tab| tab.id == id))
        });
        if self.tab_editor_dialog.is_open() && !still_exists {
            self.tab_editor_dialog.close();
            self.show_tab_editor(false);
        }
    }

    fn reconcile_terminal_selection(&mut self) {
        let still_current = self.terminal_selection.as_ref().is_some_and(|selection| {
            self.active_tab()
                .is_some_and(|tab| selection.matches_screen(&tab.screen))
        });
        if self.terminal_selection.is_some() && !still_current {
            self.cancel_terminal_selection();
        }
    }

    fn reconcile_tab_close(&mut self) {
        let still_exists = self.close_confirmation.target().is_some_and(|id| {
            self.client
                .as_ref()
                .is_some_and(|client| client.snapshot().tabs.iter().any(|tab| tab.id == id))
        });
        if self.close_confirmation.is_open() && !still_exists {
            self.close_confirmation.close();
            self.show_tab_close_controls(false);
            self.show_workspace_controls(true);
        }
    }

    fn reconcile_cwd_editor(&mut self) {
        let still_active = self.cwd_editor_dialog.target().is_some_and(|id| {
            self.client
                .as_ref()
                .is_some_and(|client| client.snapshot().active_tab_id.as_deref() == Some(id))
        });
        if self.cwd_editor_dialog.is_open() && !still_active {
            self.cwd_editor_dialog.close();
            self.set_control_text(self.send, "Send");
            self.load_composer();
        }
    }

    fn workspace_geometry(&self) -> WorkspaceLayout {
        let client = self.window.client_size();
        workspace_layout(WorkspaceLayoutInput {
            client_width: i32::try_from(client.width).unwrap_or(i32::MAX),
            client_height: i32::try_from(client.height).unwrap_or(i32::MAX),
            tabs_visible: self.tabs_visible,
            configured_tabs_width: i32::from(self.config.tabs_width),
            composer_height: COMPOSER_HEIGHT,
            status_height: STATUS_HEIGHT,
        })
    }

    fn sidebar_row_capacity(&self) -> usize {
        let height = self.workspace_geometry().sidebar_tree.height();
        usize::try_from((height - TAB_TOP).max(0) / TAB_HEIGHT)
            .unwrap_or_default()
            .max(1)
    }

    fn sidebar_row_count(&self) -> usize {
        self.client
            .as_ref()
            .map(|client| remote_tree_rows(&client.snapshot().tabs).len())
            .unwrap_or_default()
    }

    fn sidebar_max_offset(&self) -> usize {
        self.sidebar_row_count()
            .saturating_sub(self.sidebar_row_capacity())
    }

    fn sidebar_offset(&self) -> usize {
        self.sidebar_scroll_offset.min(self.sidebar_max_offset())
    }

    fn sidebar_row_geometry(
        &self,
        visual_position: usize,
        depth: usize,
        mode: TreeRowMode,
    ) -> TreeRowGeometry {
        sidebar_tree_row_geometry(
            self.workspace_geometry().sidebar_tree,
            visual_position,
            depth,
            mode,
        )
    }

    fn sidebar_scrollbar_state(&self) -> Option<(TerminalScrollbarGeometry, usize, usize)> {
        if !self.tabs_visible {
            return None;
        }
        let layout = self.workspace_geometry();
        let track = sidebar_scrollbar_track(layout.sidebar_tree);
        let maximum = self.sidebar_max_offset();
        let offset = self.sidebar_offset();
        let track_height = track.height().max(0);
        let total = self.sidebar_row_count().max(1);
        let visible = self.sidebar_row_capacity().min(total).max(1);
        let proportional = (i64::from(track_height) * visible as i64 / total as i64) as i32;
        let thumb_height = if maximum == 0 {
            track_height
        } else {
            proportional.max(24).min(track_height)
        };
        let travel = (track_height - thumb_height).max(0);
        let thumb_top = if maximum == 0 {
            track.top
        } else {
            track.top + (offset as i64 * i64::from(travel) / maximum as i64) as i32
        };
        Some((
            TerminalScrollbarGeometry {
                track,
                thumb: ProductPixelRect {
                    left: track.left + 2,
                    top: thumb_top,
                    right: (track.right - 2).max(track.left + 2),
                    bottom: thumb_top + thumb_height,
                },
            },
            offset,
            maximum,
        ))
    }

    fn sidebar_row_index_at_y(&self, y: i32) -> Option<usize> {
        tree_row_at_y(y).map(|position| self.sidebar_offset().saturating_add(position))
    }

    fn ui_snapshot_json(&mut self) -> Result<String> {
        self.sync_new_terminal_drafts();
        self.sync_settings_drafts();
        let client = self
            .client
            .as_ref()
            .context("replaceable UI is disconnected")?;
        let source = client.snapshot();
        let active_override = source.active_tab_id.as_deref().and_then(|tab_id| {
            self.config
                .terminal_override_entry(&ipc_address(), tab_id)
                .cloned()
        });
        let effective = self
            .config
            .effective_terminal_appearance(&ipc_address(), source.active_tab_id.as_deref());
        let client_size = self.window.client_size();
        let layout = self.workspace_geometry();
        let pointer_capture_owned = self
            .window
            .pointer_capture_owned()
            .context("query native pointer capture ownership")?;
        let visible_rows = remote_tree_rows(&source.tabs);
        let tabs = source
            .tabs
            .iter()
            .map(|tab| {
                let visible_position = self
                    .tabs_visible
                    .then(|| {
                        let source_position = visible_rows
                            .iter()
                            .position(|row| source.tabs[row.tab_index].id == tab.id)?;
                        source_position
                            .checked_sub(self.sidebar_offset())
                            .filter(|position| *position < self.sidebar_row_capacity())
                    })
                    .flatten();
                let depth = remote_tab_depth(&source.tabs, tab);
                let mode = if self.tab_editor_dialog.target() == Some(tab.id.as_str()) {
                    TreeRowMode::Editing
                } else {
                    TreeRowMode::Normal
                };
                let geometry = visible_position
                    .map(|position| self.sidebar_row_geometry(position, depth, mode));
                let selection = self
                    .terminal_selection
                    .as_ref()
                    .filter(|selection| selection.tab_id == tab.id)
                    .map(|selection| {
                        let (start, end) = selection.bounds();
                        serde_json::json!({
                            "start": {"row": start.row, "col": start.column},
                            "end": {"row": end.row, "col": end.column},
                            "dragging": selection.active_gesture(),
                        })
                    });
                let actions = visible_position
                    .filter(|_| source.active_tab_id.as_ref() == Some(&tab.id))
                    .map(|position| {
                        let geometry = self.sidebar_row_geometry(position, depth, mode);
                        let action = |id: &str, label: &str, bounds: ProductPixelRect| {
                            serde_json::json!({
                                "id": id,
                                "label": label,
                                "bounds": pixel_rect_json(bounds),
                                "x": bounds.left,
                                "y": bounds.top,
                                "width": bounds.width(),
                                "height": bounds.height(),
                            })
                        };
                        match mode {
                            TreeRowMode::Normal => serde_json::json!({
                                "mode": "normal",
                                "density": tree_action_density_name(geometry.actions.density),
                                "new_child": action(
                                    "new-child",
                                    self.config.locale.text(UiText::Add),
                                    geometry.actions.add_child.expect("normal row has Add"),
                                ),
                                "close": action(
                                    "close-tab",
                                    self.config.locale.text(UiText::Close),
                                    geometry.actions.secondary,
                                ),
                            }),
                            TreeRowMode::Editing => serde_json::json!({
                                "mode": "editing",
                                "density": tree_action_density_name(geometry.actions.density),
                                "save": action(
                                    "tab-editor-save",
                                    self.config.locale.text(UiText::Save),
                                    geometry.actions.primary,
                                ),
                                "cancel": action(
                                    "tab-editor-cancel",
                                    self.config.locale.text(UiText::Cancel),
                                    geometry.actions.secondary,
                                ),
                            }),
                        }
                    });
                serde_json::json!({
                    "id": tab.id,
                    "index": tab.index,
                    "parent_id": tab.parent_id,
                    "depth": depth,
                    "has_children": source.tabs.iter().any(
                        |candidate| candidate.parent_id.as_ref() == Some(&tab.id)
                    ),
                    "collapsed": tab.collapsed,
                    "visible": visible_position.is_some(),
                    "name": tab.title,
                    "terminal_title": tab.screen.terminal_title,
                    "note": tab.note,
                    "active": source.active_tab_id.as_ref() == Some(&tab.id),
                    "pid": tab.process_id,
                    "state": if tab.dead { "dead" } else { "running" },
                    "exit_code": tab.exit_code,
                    "working_context": {
                        "cwd": {
                            "path": tab.working_context.cwd,
                            "confirmed_path": tab.working_context.cwd_confirmed_path,
                            "confirmed": tab.working_context.cwd_confirmed,
                            "source": tab.working_context.cwd_source,
                            "pending": tab.working_context.cwd_request_pending,
                        },
                        "shell": tab.working_context.shell,
                        "proxy": {
                            "configured": tab.working_context.proxy_configured,
                            "source": tab.working_context.proxy_source,
                            "application_state": tab.working_context.proxy_application_state,
                            "request_pending": tab.working_context.proxy_request_pending,
                            "endpoint_visible": false,
                            "credential_revealed": false,
                        },
                    },
                    "scrollback_offset": tab.screen.scrollback_offset,
                    "selection": selection,
                    "draft": tab.composer.byte_length > 0,
                    "bounds": geometry.map(|value| pixel_rect_json(value.row)),
                    "render": geometry.map(|value| serde_json::json!({
                        "mode": match value.mode {
                            TreeRowMode::Normal => "normal",
                            TreeRowMode::Editing => "editing",
                        },
                        "row": pixel_rect_json(value.row),
                        "selection": pixel_rect_json(value.selection),
                        "node": {"x": value.node_x, "y": value.node_y},
                        "expander": pixel_rect_json(value.expander),
                        "status": pixel_rect_json(value.status),
                        "disclosure_hit": pixel_rect_json(value.disclosure_hit),
                        "text": pixel_rect_json(value.text),
                        "name": pixel_rect_json(value.name),
                        "note": pixel_rect_json(value.note),
                        "editors": value.editors.map(|editors| serde_json::json!({
                            "name": pixel_rect_json(editors.name),
                            "note": pixel_rect_json(editors.note),
                        })),
                    })),
                    "actions": actions,
                })
            })
            .collect::<Vec<_>>();
        let scrollbar = self.scrollbar_state().map(|(geometry, offset, maximum)| {
            serde_json::json!({
                "visible": true,
                "track": pixel_rect_json(geometry.track),
                "thumb": pixel_rect_json(geometry.thumb),
                "offset": offset,
                "max_offset": maximum,
            })
        });
        let sidebar_scrollbar =
            self.sidebar_scrollbar_state()
                .map(|(geometry, offset, maximum)| {
                    serde_json::json!({
                        "visible": true,
                        "track": pixel_rect_json(geometry.track),
                        "thumb": pixel_rect_json(geometry.thumb),
                        "offset": offset,
                        "max_offset": maximum,
                    })
                });
        let (rows, columns) = self
            .active_tab()
            .map(|tab| (tab.screen.rows, tab.screen.columns))
            .unwrap_or_default();
        let (desired_rows, desired_columns) = self.desired_terminal_grid();
        let tab_editor = if self.tab_editor_dialog.is_open() {
            let focused = self.window.focused_target();
            if focused == FocusTarget::Control(self.tab_title_edit) {
                self.tab_editor_dialog.set_focus(TabEditorFocus::Name);
            } else if focused == FocusTarget::Control(self.tab_note_edit) {
                self.tab_editor_dialog.set_focus(TabEditorFocus::Note);
            }
            Some(self.tab_editor_dialog.snapshot_modal())
        } else {
            None
        };
        let modal = modal_surface_from_gate(self.focus_gate()).map(|surface| match surface {
            ModalSurface::WindowClose => self.window_close_dialog.snapshot_modal(),
            ModalSurface::Settings => self.settings_dialog.snapshot_modal(),
            ModalSurface::NewTerminal => self.new_terminal_dialog.snapshot_modal(),
            ModalSurface::CwdEditor => self.cwd_editor_dialog.snapshot_modal(),
            ModalSurface::TabClose => self.close_confirmation.snapshot_modal(),
        });
        let (copy_enabled, paste_enabled) = self.system_menu_state();
        let composer_input = ProductPixelRect {
            left: layout.composer.left + MARGIN,
            top: layout.composer.top + 26,
            right: (layout.composer.right - 76 - MARGIN * 2)
                .max(layout.composer.left + MARGIN + 80),
            bottom: (layout.composer.bottom - 8).max(layout.composer.top + 56),
        };
        let focus = if let Some(surface) = modal_surface_from_gate(self.focus_gate()) {
            surface.as_str()
        } else if self.tab_editor_dialog.is_open() {
            "tab-editor"
        } else {
            match self.current_focus_surface() {
                RemoteFocusSurface::Terminal => "terminal",
                RemoteFocusSurface::Composer => "composer",
                RemoteFocusSurface::Tabs => "tabs",
            }
        };
        let native_window_state = self.window.state();
        let window_state = WindowSemanticState::from_native_flags(
            native_window_state.minimized,
            native_window_state.maximized,
        );
        let workspace_controls_visible = !self.window_close_dialog.is_open()
            && !self.settings_dialog.is_open()
            && !self.new_terminal_dialog.is_open()
            && !self.close_confirmation.is_open();
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": crate::ui_bridge::UI_CLIENT_STATE_SCHEMA_VERSION,
            "protocol_version": 1,
            "projection": PROJECTION_REPLACEABLE_UI_CLIENT,
            "client_pid": std::process::id(),
            "server_pid": source.server_pid,
            "event_position": {
                "epoch": source.server_epoch,
                "sequence": source.position.sequence,
            },
            "window": {
                "title": format!("AgenTerm-{}:{}", env!("CARGO_PKG_VERSION"), resolved_ipc_endpoint()?.logical_instance),
                "client_width": client_size.width,
                "client_height": client_size.height,
                "visible": native_window_state.visible,
                "detached": false,
                "minimized": window_state.is_minimized(),
                "state": window_state.as_str(),
            },
            "render_activity": self.render_activity_sample.map(|activity| serde_json::json!({
                "sequence": self.render_activity_sample_sequence,
                "redraw_requests": activity.redraw_requests,
                "parent_paints": activity.parent_paints,
                "control_bounds_updates": activity.control_bounds_updates,
                "control_bounds_skips": activity.control_bounds_skips,
                "control_visibility_updates": activity.control_visibility_updates,
                "control_visibility_skips": activity.control_visibility_skips,
            })),
            "layout": {
                "sidebar": {
                    "x": layout.sidebar.left,
                    "y": layout.sidebar.top,
                    "visible": self.tabs_visible,
                    "configured_width": self.config.tabs_width,
                    "effective_width": layout.effective_tabs_width,
                    "width": layout.sidebar.width(),
                    "height": layout.sidebar.height(),
                    "bounds": pixel_rect_json(layout.sidebar),
                    "resize_grip": layout.resize_grip.map(pixel_rect_json),
                    "resizing": self.tabs_resize_dragging,
                    "scrollbar": sidebar_scrollbar,
                },
                "toolbar": layout.workspace_toolbar.map(|toolbar| serde_json::json!({
                    "bounds": pixel_rect_json(toolbar.bounds),
                    "new": pixel_rect_json(toolbar.new_tab),
                    "tabs": pixel_rect_json(toolbar.tabs),
                    "control_center": pixel_rect_json(toolbar.control_center),
                    "settings": pixel_rect_json(toolbar.settings),
                    "locale": pixel_rect_json(toolbar.locale),
                    "font_decrease": pixel_rect_json(toolbar.font_decrease),
                    "font_increase": pixel_rect_json(toolbar.font_increase),
                })),
                "terminal": {
                    "x": layout.terminal.left,
                    "y": layout.terminal.top,
                    "width": layout.terminal.width(),
                    "viewport_width": (
                        layout.terminal.width() - TERMINAL_SCROLLBAR_WIDTH
                    ).max(0),
                    "height": layout.terminal.height(),
                    "bounds": pixel_rect_json(layout.terminal),
                    "rows": rows,
                    "cols": columns,
                    "desired_rows": desired_rows,
                    "desired_cols": desired_columns,
                    "alternate_screen": self.active_tab().is_some_and(|tab| tab.screen.alternate_screen),
                    "application_cursor": self.active_tab().is_some_and(|tab| tab.screen.application_cursor),
                    "resize_pending": self.pending_terminal_resize.is_some(),
                    "scrollbar": scrollbar,
                },
                "composer": {
                    "visible": workspace_controls_visible,
                    "input_visible": workspace_controls_visible,
                    "send_visible": workspace_controls_visible,
                    "x": layout.composer.left,
                    "y": layout.composer.top,
                    "width": layout.composer.width(),
                    "height": layout.composer.height(),
                    "bounds": pixel_rect_json(layout.composer),
                    "input": {
                        "bounds": pixel_rect_json(composer_input),
                        "target_rows": 3,
                        "vertical_scrollbar": true,
                    },
                },
                "status_bar": {
                    "x": layout.status.left,
                    "y": layout.status.top,
                    "width": layout.status.width(),
                    "height": layout.status.height(),
                    "bounds": pixel_rect_json(layout.status),
                    "tabs_recovery": layout.status_segments.tabs_recovery.map(pixel_rect_json),
                    "cwd": {
                        "bounds": pixel_rect_json(layout.status_segments.cwd),
                        "action": "open-cwd-editor",
                    },
                    "proxy": {
                        "bounds": pixel_rect_json(layout.status_segments.proxy),
                        "available": false,
                        "archived": true,
                        "action": serde_json::Value::Null,
                        "eye_action": serde_json::Value::Null,
                    },
                    "provider": "placeholder",
                },
            },
            "focus": {
                "surface": focus,
                "window_id": source.active_tab_id,
                "window_focused": self.window.focused_target() != FocusTarget::None,
            },
            "terminal_paste": {
                "state": if self.pending_terminal_paste.is_some() { "pending" } else { "idle" },
                "target": self.pending_terminal_paste
                    .as_ref()
                    .map(|pending| pending.tab_id.as_str()),
            },
            "system_menu": {
                "toggle_tabs": {
                    "id": SYSTEM_MENU_TOGGLE_TABS_ID.0,
                    "label": self.config.locale.text(UiText::ToggleTabs),
                    "checked": self.tabs_visible,
                },
                "copy": {
                    "id": SYSTEM_MENU_COPY_ID.0,
                    "label": self.config.locale.text(UiText::Copy),
                    "enabled": copy_enabled,
                },
                "paste": {
                    "id": SYSTEM_MENU_PASTE_ID.0,
                    "label": self.config.locale.text(UiText::Paste),
                    "enabled": paste_enabled,
                },
            },
            "terminal_interaction": {
                "selection": self.terminal_selection.as_ref().map(|selection| {
                    let (start, end) = selection.bounds();
                    let highlight = source
                        .tabs
                        .iter()
                        .find(|tab| tab.id == selection.tab_id)
                        .map(|tab| terminal_selection_highlight_rects(
                            selection,
                            &tab.screen,
                            layout.terminal,
                            self.cell_width,
                            self.cell_height,
                        ))
                        .unwrap_or_default();
                    serde_json::json!({
                        "phase": selection.phase().as_str(),
                        "tab_id": selection.tab_id,
                        "copyable": selection.can_copy(),
                        "capture_owned": selection.active_gesture() && pointer_capture_owned,
                        "selection": {
                            "start": {"row": start.row, "col": start.column},
                            "end": {"row": end.row, "col": end.column},
                        },
                        "highlight": {
                            "rendered": !highlight.is_empty(),
                            "bounds": highlight.into_iter().map(pixel_rect_json).collect::<Vec<_>>(),
                        },
                        "autoscroll": {"active": self.terminal_selection_autoscroll.is_some()},
                    })
                }),
                "raw_mouse_arbitration": true,
                "rectangular_selection": false,
            },
            "tabs": tabs,
            "tab_editor": tab_editor,
            "modal": modal,
            "settings": {
                "terminal_font_family": self.config.terminal_font_family,
                "terminal_font_size": self.config.terminal_font_size,
                "color_theme": self.config.color_theme.as_str(),
                "scope": self.settings_dialog.scope().as_str(),
                "target_tab_id": self.settings_dialog.target_tab_id(),
                "current_terminal_override": active_override,
                "effective": {
                    "terminal_font_family": effective.terminal_font_family,
                    "terminal_font_size": effective.terminal_font_size,
                    "color_theme": effective.color_theme.as_str(),
                },
                "theme_draft": self.settings_dialog.is_open().then(
                    || self.settings_dialog.theme_draft().as_str()
                ),
                "theme_options": ThemeId::ALL.map(|theme| serde_json::json!({
                    "id": theme.as_str(),
                    "label": theme.label(),
                })),
                "tabs_visible": self.tabs_visible,
                "tabs_width": self.config.tabs_width,
            },
            "locale": {
                "id": self.config.locale.as_str(),
                "controls": {
                    "send": self.config.locale.text(UiText::Send),
                    "settings": self.config.locale.text(UiText::Settings),
                    "new": self.config.locale.text(UiText::New),
                    "apply": self.config.locale.text(UiText::Apply),
                    "save": self.config.locale.text(UiText::Save),
                },
            },
            "feedback": {
                "message": self.last_message,
                "error": self.last_error,
            },
        }))
        .context("could not encode replaceable UI snapshot")
    }

    fn publish_ui_snapshot(&mut self) -> Result<bool> {
        let json = self.ui_snapshot_json()?;
        if self.last_published_snapshot.as_deref() == Some(json.as_str()) {
            return Ok(false);
        }
        self.client
            .as_mut()
            .context("replaceable UI is disconnected")?
            .publish_snapshot(&json)?;
        self.last_published_snapshot = Some(json);
        Ok(true)
    }

    fn layout_rects(
        &self,
    ) -> (
        ProductPixelRect,
        ProductPixelRect,
        ProductPixelRect,
        ProductPixelRect,
    ) {
        let geometry = self.workspace_geometry();
        (
            win_rect(geometry.sidebar),
            win_rect(geometry.terminal),
            win_rect(geometry.composer),
            win_rect(geometry.status),
        )
    }

    fn cwd_status_rect(&self) -> ProductPixelRect {
        win_rect(self.workspace_geometry().status_segments.cwd)
    }

    fn tabs_recovery_rect(&self) -> Option<ProductPixelRect> {
        self.workspace_geometry()
            .status_segments
            .tabs_recovery
            .map(win_rect)
    }

    fn set_tabs_visible(&mut self, visible: bool) {
        self.invalidate_sidebar_text_click();
        self.finish_cwd_editor(false, ComposerWriteMode::EmptyOnly);
        self.finish_tab_edit(false);
        self.tabs_visible = visible;
        self.config.tabs_visible = visible;
        if let Err(error) = save_config(&self.config) {
            self.last_error = Some(format!("Tabs visibility save failed: {error:#}"));
        }
        if !visible && self.focus_surface == RemoteFocusSurface::Tabs {
            self.set_focus_surface_unchecked(RemoteFocusSurface::Terminal);
            self.window.focus();
        }
        self.layout();
        self.resize_active_terminal();
    }

    fn toggle_tabs(&mut self) {
        self.set_tabs_visible(!self.tabs_visible);
    }

    fn active_terminal_appearance(&self) -> EffectiveTerminalAppearance {
        let active = self
            .client
            .as_ref()
            .and_then(|client| client.snapshot().active_tab_id.as_deref());
        self.config
            .effective_terminal_appearance(&ipc_address(), active)
    }

    fn apply_effective_terminal_font(&mut self) -> Result<()> {
        let appearance = self.active_terminal_appearance();
        let (font, cell_width, cell_height) = create_terminal_font(
            &self.window,
            &appearance.terminal_font_family,
            appearance.terminal_font_size,
        )?;
        self.font = font;
        self.cell_width = cell_width;
        self.cell_height = cell_height;
        self.layout();
        self.resize_active_terminal();
        Ok(())
    }

    fn adjust_active_terminal_font(&mut self, delta: i16) {
        let Some(tab_id) = self
            .client
            .as_ref()
            .and_then(|client| client.snapshot().active_tab_id.clone())
        else {
            return;
        };
        let current = self.active_terminal_appearance().terminal_font_size;
        let next = i32::from(current).saturating_add(i32::from(delta)).clamp(
            i32::from(MIN_TERMINAL_FONT_SIZE),
            i32::from(MAX_TERMINAL_FONT_SIZE),
        ) as u16;
        if next == current {
            return;
        }
        let mut terminal_override = self.config.terminal_override(&ipc_address(), &tab_id);
        terminal_override.terminal_font_size = Some(next);
        self.config
            .set_terminal_override(&ipc_address(), &tab_id, terminal_override);
        if let Err(error) = save_config(&self.config) {
            self.last_error = Some(format!("Terminal font override save failed: {error:#}"));
            return;
        }
        if let Err(error) = self.apply_effective_terminal_font() {
            self.last_error = Some(format!("Terminal font update failed: {error:#}"));
        }
    }

    fn toggle_locale(&mut self) {
        self.config.locale = self.config.locale.toggled();
        if let Err(error) = save_config(&self.config) {
            self.last_error = Some(format!("Locale save failed: {error:#}"));
            return;
        }
        self.apply_locale();
        self.layout();
    }

    fn apply_locale(&self) {
        let locale = self.config.locale;
        let set = |control: ControlId, text: &str| self.set_control_text(control, text);
        set(self.send, locale.text(UiText::Send));
        set(self.new_tab, locale.text(UiText::New));
        set(self.settings, locale.text(UiText::Settings));
        set(self.locale, locale.toolbar_label());
        set(self.tab_save, locale.text(UiText::Save));
        set(self.tab_cancel, locale.text(UiText::Cancel));
        set(self.close_keep, locale.text(UiText::KeepServerRunning));
        set(self.close_stop, locale.text(UiText::StopServerAndExit));
        set(self.close_cancel, locale.text(UiText::Cancel));
        set(self.settings_apply, locale.text(UiText::Apply));
        set(self.settings_cancel, locale.text(UiText::Cancel));
        set(
            self.settings_default_scope,
            locale.text(UiText::DefaultValues),
        );
        set(
            self.settings_current_scope,
            locale.text(UiText::CurrentTerminal),
        );
        set(
            self.settings_reset_overrides,
            locale.text(UiText::ResetOverrides),
        );
        set(
            self.tab_close_confirm,
            locale.text(UiText::TerminateAndClose),
        );
        set(self.tab_close_cancel, locale.text(UiText::Cancel));
        set(self.new_create, locale.text(UiText::Create));
        set(self.new_cancel, locale.text(UiText::Cancel));
        set(self.new_default_shell, locale.text(UiText::Default));
        for (id, label) in [
            (SYSTEM_MENU_COPY_ID, locale.text(UiText::Copy)),
            (SYSTEM_MENU_PASTE_ID, locale.text(UiText::Paste)),
            (SYSTEM_MENU_TOGGLE_TABS_ID, locale.text(UiText::ToggleTabs)),
        ] {
            let _ = self.window.set_system_menu_text(id, label);
        }
    }

    fn layout(&mut self) {
        self.invalidate_sidebar_text_click();
        let geometry = self.workspace_geometry();
        let composer = geometry.composer;
        self.set_control_text(
            self.tabs_button,
            &format!(
                "{}{}",
                if self.tabs_visible { '<' } else { '>' },
                self.config.locale.text(UiText::Tabs)
            ),
        );
        if let Some(toolbar) = geometry.workspace_toolbar {
            for (control, bounds) in [
                (self.new_tab, toolbar.new_tab),
                (self.tabs_button, toolbar.tabs),
                (self.control_center, toolbar.control_center),
                (self.settings, toolbar.settings),
                (self.locale, toolbar.locale),
                (self.font_decrease, toolbar.font_decrease),
                (self.font_increase, toolbar.font_increase),
            ] {
                self.set_control_bounds(control, bounds);
            }
        }
        let send_width = 76;
        self.set_control_bounds(
            self.edit,
            ProductPixelRect {
                left: composer.left + MARGIN,
                top: composer.top + 26,
                right: composer.right - send_width - MARGIN * 2,
                bottom: (composer.bottom - 8).max(composer.top + 56),
            },
        );
        self.set_control_bounds(
            self.send,
            ProductPixelRect {
                left: composer.right - send_width - MARGIN,
                top: composer.top + 26,
                right: composer.right - MARGIN,
                bottom: composer.top + 60,
            },
        );
        let toolbar_visible = geometry.workspace_toolbar.is_some()
            && !self.window_close_dialog.is_open()
            && !self.settings_dialog.is_open()
            && !self.new_terminal_dialog.is_open();
        self.set_control_visible(
            self.new_tab,
            toolbar_visible && !self.close_confirmation.is_open(),
        );
        for control in [
            self.tabs_button,
            self.control_center,
            self.settings,
            self.locale,
            self.font_decrease,
            self.font_increase,
        ] {
            self.set_control_visible(control, toolbar_visible);
        }
        self.layout_tab_editor();
        self.layout_close_controls();
        self.layout_settings_controls();
        self.layout_tab_close_controls();
        self.layout_new_terminal_controls();
    }

    fn layout_tab_editor(&self) {
        let Some(tab_id) = self.tab_editor_dialog.target() else {
            self.show_tab_editor(false);
            return;
        };
        let Some(client) = &self.client else {
            self.show_tab_editor(false);
            return;
        };
        let rows = remote_tree_rows(&client.snapshot().tabs);
        let Some((position, row)) = rows
            .iter()
            .enumerate()
            .find(|(_, row)| client.snapshot().tabs[row.tab_index].id == tab_id)
        else {
            self.show_tab_editor(false);
            return;
        };
        let Some(viewport_position) = position
            .checked_sub(self.sidebar_offset())
            .filter(|position| *position < self.sidebar_row_capacity())
        else {
            self.show_tab_editor(false);
            return;
        };
        let geometry =
            self.sidebar_row_geometry(viewport_position, row.depth, TreeRowMode::Editing);
        let Some(editors) = geometry.editors else {
            self.show_tab_editor(false);
            return;
        };
        self.set_control_bounds(self.tab_title_edit, editors.name);
        self.set_control_bounds(self.tab_note_edit, editors.note);
        self.set_control_bounds(self.tab_save, geometry.actions.primary);
        self.set_control_bounds(self.tab_cancel, geometry.actions.secondary);
        self.show_tab_editor(
            self.tabs_visible
                && !self.window_close_dialog.is_open()
                && !self.close_confirmation.is_open(),
        );
    }

    fn show_tab_editor(&self, visible: bool) {
        for control in [
            self.tab_title_edit,
            self.tab_note_edit,
            self.tab_save,
            self.tab_cancel,
        ] {
            self.set_control_visible(control, visible);
        }
    }

    fn close_modal_geometry(&self) -> (ProductPixelRect, [ProductPixelRect; 3]) {
        let client = self.window.client_size();
        let client_right = i32::try_from(client.width).unwrap_or(i32::MAX);
        let client_bottom = i32::try_from(client.height).unwrap_or(i32::MAX);
        let width = (client_right - 32).clamp(360, 620);
        let height = 230;
        let left = ((client_right - width) / 2).max(0);
        let top = ((client_bottom - height) / 2).max(0);
        let modal = ProductPixelRect {
            left,
            top,
            right: left + width,
            bottom: top + height,
        };
        let gap = 8;
        let button_left = left + 20;
        let button_width = ((width - 40 - gap * 2) / 3).max(90);
        let button_top = top + 158;
        let button_bottom = button_top + 40;
        let keep = ProductPixelRect {
            left: button_left,
            top: button_top,
            right: button_left + button_width,
            bottom: button_bottom,
        };
        let stop = ProductPixelRect {
            left: keep.right + gap,
            top: button_top,
            right: keep.right + gap + button_width,
            bottom: button_bottom,
        };
        let cancel = ProductPixelRect {
            left: stop.right + gap,
            top: button_top,
            right: stop.right + gap + button_width,
            bottom: button_bottom,
        };
        (modal, [keep, stop, cancel])
    }

    fn layout_close_controls(&self) {
        let (_, buttons) = self.close_modal_geometry();
        for (control, rect) in [
            (self.close_keep, buttons[0]),
            (self.close_stop, buttons[1]),
            (self.close_cancel, buttons[2]),
        ] {
            self.set_control_bounds(control, rect);
        }
        self.show_close_controls(self.window_close_dialog.is_open());
    }

    fn show_close_controls(&self, visible: bool) {
        for control in [self.close_keep, self.close_stop, self.close_cancel] {
            self.set_control_visible(control, visible);
        }
    }

    fn show_workspace_controls(&self, visible: bool) {
        self.set_control_visible(self.edit, visible);
        self.set_control_visible(self.send, visible);
        let toolbar_visible = visible
            && self.workspace_geometry().workspace_toolbar.is_some()
            && !self.close_confirmation.is_open();
        for control in [
            self.tabs_button,
            self.control_center,
            self.settings,
            self.locale,
            self.font_decrease,
            self.font_increase,
            self.new_tab,
        ] {
            self.set_control_visible(control, toolbar_visible);
        }
        self.show_tab_editor(visible && self.tabs_visible && self.tab_editor_dialog.is_open());
    }

    fn settings_modal_geometry(&self) -> (ProductPixelRect, [ProductPixelRect; 12]) {
        let client = self.window.client_size();
        let client_right = i32::try_from(client.width).unwrap_or(i32::MAX);
        let client_bottom = i32::try_from(client.height).unwrap_or(i32::MAX);
        let width = (client_right - 32).clamp(520, 680);
        let height = (client_bottom - 32).clamp(390, 430);
        let left = ((client_right - width) / 2).max(0);
        let top = ((client_bottom - height) / 2).max(0);
        let modal = ProductPixelRect {
            left,
            top,
            right: left + width,
            bottom: top + height,
        };
        let default_scope = ProductPixelRect {
            left: left + 32,
            top: top + 54,
            right: left + 210,
            bottom: top + 88,
        };
        let current_scope = ProductPixelRect {
            left: default_scope.right + 8,
            top: default_scope.top,
            right: (default_scope.right + 228).min(left + width - 32),
            bottom: default_scope.bottom,
        };
        let font = ProductPixelRect {
            left: left + 32,
            top: top + 132,
            right: left + width - 174,
            bottom: top + 164,
        };
        let font_inherit = ProductPixelRect {
            left: left + width - 158,
            top: font.top,
            right: left + width - 32,
            bottom: font.bottom,
        };
        let size = ProductPixelRect {
            left: left + 32,
            top: top + 204,
            right: left + 164,
            bottom: top + 236,
        };
        let size_inherit = ProductPixelRect {
            left: left + width - 158,
            top: size.top,
            right: left + width - 32,
            bottom: size.bottom,
        };
        let dark = ProductPixelRect {
            left: left + 32,
            top: top + 276,
            right: left + 178,
            bottom: top + 310,
        };
        let light = ProductPixelRect {
            left: left + 190,
            top: dark.top,
            right: left + 336,
            bottom: dark.bottom,
        };
        let theme_inherit = ProductPixelRect {
            left: left + width - 158,
            top: dark.top,
            right: left + width - 32,
            bottom: dark.bottom,
        };
        let reset = ProductPixelRect {
            left: left + 32,
            top: top + height - 54,
            right: left + 174,
            bottom: top + height - 18,
        };
        let apply = ProductPixelRect {
            left: left + width - 126,
            top: top + height - 54,
            right: left + width - 32,
            bottom: top + height - 18,
        };
        let cancel = ProductPixelRect {
            left: apply.left - 106,
            top: apply.top,
            right: apply.left - 12,
            bottom: apply.bottom,
        };
        (
            modal,
            [
                default_scope,
                current_scope,
                font,
                font_inherit,
                size,
                size_inherit,
                dark,
                light,
                theme_inherit,
                reset,
                apply,
                cancel,
            ],
        )
    }

    fn layout_settings_controls(&self) {
        let (_, controls) = self.settings_modal_geometry();
        for (control, rect) in [
            (self.settings_default_scope, controls[0]),
            (self.settings_current_scope, controls[1]),
            (self.settings_font, controls[2]),
            (self.settings_font_inherit, controls[3]),
            (self.settings_size, controls[4]),
            (self.settings_size_inherit, controls[5]),
            (self.settings_dark, controls[6]),
            (self.settings_light, controls[7]),
            (self.settings_theme_inherit, controls[8]),
            (self.settings_reset_overrides, controls[9]),
            (self.settings_apply, controls[10]),
            (self.settings_cancel, controls[11]),
        ] {
            self.set_control_bounds(control, rect);
        }
        self.show_settings_controls(self.settings_dialog.is_open());
    }

    fn show_settings_controls(&self, visible: bool) {
        for control in [
            self.settings_font,
            self.settings_size,
            self.settings_dark,
            self.settings_light,
            self.settings_apply,
            self.settings_cancel,
            self.settings_default_scope,
            self.settings_current_scope,
        ] {
            self.set_control_visible(control, visible);
        }
        let override_visible =
            visible && self.settings_dialog.scope() == SettingsScope::CurrentTerminal;
        for control in [
            self.settings_font_inherit,
            self.settings_size_inherit,
            self.settings_theme_inherit,
            self.settings_reset_overrides,
        ] {
            self.set_control_visible(control, override_visible);
        }
    }

    fn new_terminal_modal_geometry(&self) -> (ProductPixelRect, [ProductPixelRect; 8]) {
        let client = self.window.client_size();
        let client_right = i32::try_from(client.width).unwrap_or(i32::MAX);
        let client_bottom = i32::try_from(client.height).unwrap_or(i32::MAX);
        let width = (client_right - 32).clamp(480, 620);
        let height = (client_bottom - 32).clamp(390, 450);
        let left = ((client_right - width) / 2).max(0);
        let top = ((client_bottom - height) / 2).max(0);
        let modal = ProductPixelRect {
            left,
            top,
            right: left + width,
            bottom: top + height,
        };
        let inner_left = left + 28;
        let inner_right = left + width - 28;
        let gap = 8;
        let shell_width = ((inner_right - inner_left - gap * 2) / 3).max(90);
        let shell_top = top + 74;
        let shell = |index: i32| ProductPixelRect {
            left: inner_left + index * (shell_width + gap),
            top: shell_top,
            right: inner_left + index * (shell_width + gap) + shell_width,
            bottom: shell_top + 34,
        };
        let field = |field_top: i32| ProductPixelRect {
            left: inner_left,
            top: field_top,
            right: inner_right,
            bottom: field_top + 30,
        };
        let create = ProductPixelRect {
            left: inner_right - 96,
            top: top + height - 52,
            right: inner_right,
            bottom: top + height - 18,
        };
        let cancel = ProductPixelRect {
            left: create.left - 106,
            top: create.top,
            right: create.left - 10,
            bottom: create.bottom,
        };
        (
            modal,
            [
                shell(0),
                shell(1),
                shell(2),
                field(top + 142),
                field(top + 218),
                field(top + 294),
                create,
                cancel,
            ],
        )
    }

    fn layout_new_terminal_controls(&self) {
        let (_, bounds) = self.new_terminal_modal_geometry();
        for (control, rect) in [
            (self.new_default_shell, bounds[0]),
            (self.new_cmd_shell, bounds[1]),
            (self.new_powershell, bounds[2]),
            (self.new_initial_command, bounds[3]),
            (self.new_http_proxy, bounds[4]),
            (self.new_https_proxy, bounds[5]),
            (self.new_create, bounds[6]),
            (self.new_cancel, bounds[7]),
        ] {
            self.set_control_bounds(control, rect);
        }
        self.show_new_terminal_controls(self.new_terminal_dialog.is_open());
    }

    fn show_new_terminal_controls(&self, visible: bool) {
        for control in [
            self.new_default_shell,
            self.new_cmd_shell,
            self.new_powershell,
            self.new_initial_command,
            self.new_http_proxy,
            self.new_https_proxy,
            self.new_create,
            self.new_cancel,
        ] {
            self.set_control_visible(control, visible);
        }
    }

    fn tab_close_modal_geometry(&self) -> (ProductPixelRect, [ProductPixelRect; 2]) {
        let client = self.window.client_size();
        let client_right = i32::try_from(client.width).unwrap_or(i32::MAX);
        let client_bottom = i32::try_from(client.height).unwrap_or(i32::MAX);
        let width = (client_right - 32).clamp(380, 500);
        let height = 210;
        let left = ((client_right - width) / 2).max(0);
        let top = ((client_bottom - height) / 2).max(0);
        let modal = ProductPixelRect {
            left,
            top,
            right: left + width,
            bottom: top + height,
        };
        let confirm = ProductPixelRect {
            left: left + width - 250,
            top: top + 148,
            right: left + width - 116,
            bottom: top + 184,
        };
        let cancel = ProductPixelRect {
            left: left + width - 104,
            top: top + 148,
            right: left + width - 24,
            bottom: top + 184,
        };
        (modal, [confirm, cancel])
    }

    fn layout_tab_close_controls(&self) {
        let (_, buttons) = self.tab_close_modal_geometry();
        for (control, rect) in [
            (self.tab_close_confirm, buttons[0]),
            (self.tab_close_cancel, buttons[1]),
        ] {
            self.set_control_bounds(control, rect);
        }
        self.show_tab_close_controls(self.close_confirmation.is_open());
    }

    fn show_tab_close_controls(&self, visible: bool) {
        for control in [self.tab_close_confirm, self.tab_close_cancel] {
            self.set_control_visible(control, visible);
        }
    }

    fn resize_active_terminal(&mut self) {
        let Some((server_epoch, tab_id, current_rows, current_columns)) =
            self.client.as_ref().and_then(|client| {
                let snapshot = client.snapshot();
                let active_tab_id = snapshot.active_tab_id.as_deref()?;
                let tab = snapshot.tabs.iter().find(|tab| tab.id == active_tab_id)?;
                Some((
                    snapshot.server_epoch.clone(),
                    tab.id.clone(),
                    tab.screen.rows,
                    tab.screen.columns,
                ))
            })
        else {
            return;
        };
        let (rows, columns) = self.desired_terminal_grid();
        let requested = RemoteTerminalResize {
            server_epoch,
            tab_id,
            rows,
            columns,
        };
        match terminal_resize_decision(
            current_rows,
            current_columns,
            self.pending_terminal_resize.as_ref(),
            &requested,
        ) {
            RemoteTerminalResizeDecision::Current => {
                self.pending_terminal_resize = None;
                return;
            }
            RemoteTerminalResizeDecision::InFlight => return,
            RemoteTerminalResizeDecision::Send => {}
        }
        let Some(request) = self.client.as_ref().map(|client| {
            client.resize_request(requested.tab_id.clone(), requested.rows, requested.columns)
        }) else {
            return;
        };
        self.pending_terminal_resize = Some(requested.clone());
        self.terminal_resize_worker.queue(RemoteTerminalResizeTask {
            requested,
            execute: Box::new(move || request.execute()),
        });
    }

    fn desired_terminal_grid(&self) -> (u16, u16) {
        let (_, terminal, _, _) = self.layout_rects();
        let cell_height = self.cell_height.max(1);
        let cell_width = self.cell_width.max(1);
        let rows = ((terminal.bottom - terminal.top).max(cell_height) / cell_height)
            .clamp(2, 512)
            .try_into()
            .unwrap_or(2);
        let columns = ((terminal.right - terminal.left - TERMINAL_SCROLLBAR_WIDTH).max(cell_width)
            / cell_width)
            .clamp(2, 512)
            .try_into()
            .unwrap_or(2);
        (rows, columns)
    }

    fn reconcile_pending_terminal_resize(&mut self) {
        let Some(pending) = self.pending_terminal_resize.as_ref() else {
            return;
        };
        let matches = self.client.as_ref().is_some_and(|client| {
            let snapshot = client.snapshot();
            snapshot.server_epoch == pending.server_epoch
                && snapshot.active_tab_id.as_deref() == Some(pending.tab_id.as_str())
                && snapshot.tabs.iter().any(|tab| {
                    tab.id == pending.tab_id
                        && tab.screen.rows == u32::from(pending.rows)
                        && tab.screen.columns == u32::from(pending.columns)
                })
        });
        if matches {
            self.pending_terminal_resize = None;
        }
    }

    fn process_terminal_resize_results(&mut self) -> bool {
        let mut changed = false;
        while let Ok(completed) = self.terminal_resize_worker.try_result() {
            if self.pending_terminal_resize.as_ref() != Some(&completed.requested) {
                continue;
            }
            if let Err(error) = completed.result {
                self.pending_terminal_resize = None;
                self.last_error = Some(format!("PTY resize failed: {error}"));
                changed = true;
            }
        }
        changed
    }

    fn process_terminal_paste_results(&mut self) -> bool {
        let mut changed = false;
        loop {
            let completed = match self.terminal_paste_worker.try_result() {
                Ok(completed) => completed,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    if self.pending_terminal_paste.take().is_some() {
                        self.last_error =
                            Some("Paste failed: terminal paste worker stopped".to_owned());
                        changed = true;
                    }
                    break;
                }
            };
            if self.pending_terminal_paste.as_ref() != Some(&completed.requested) {
                continue;
            }
            self.pending_terminal_paste = None;
            let text = match completed.result {
                Ok(text) => text,
                Err(error) => {
                    self.last_error = Some(format!("Paste failed: {error}"));
                    changed = true;
                    continue;
                }
            };
            let current = self.client.as_ref().is_some_and(|client| {
                let snapshot = client.snapshot();
                snapshot.server_epoch == completed.requested.server_epoch
                    && snapshot.active_tab_id.as_deref()
                        == Some(completed.requested.tab_id.as_str())
                    && snapshot.tabs.iter().any(|tab| {
                        tab.id == completed.requested.tab_id
                            && tab.screen.bracketed_paste == completed.requested.bracketed
                    })
            }) && self.window.focused_target() == FocusTarget::Window
                && self.current_focus_surface() == RemoteFocusSurface::Terminal
                && !self.window_close_dialog.is_open()
                && !self.settings_dialog.is_open()
                && !self.new_terminal_dialog.is_open()
                && !self.tab_editor_dialog.is_open()
                && !self.close_confirmation.is_open()
                && !self.cwd_editor_dialog.is_open();
            if !current {
                self.last_error = Some(
                    "Paste cancelled because the active terminal or paste mode changed".to_owned(),
                );
                changed = true;
                continue;
            }
            let bytes = terminal_paste_bytes(&text, completed.requested.bracketed);
            let result = self
                .client
                .as_mut()
                .context("replaceable UI is disconnected")
                .and_then(|client| {
                    client.paste_input(
                        &completed.requested.tab_id,
                        &bytes,
                        text.len(),
                        text.chars().count(),
                        completed.requested.bracketed,
                    )
                });
            match result {
                Ok(()) => {
                    self.cancel_terminal_selection();
                    self.last_error = None;
                    self.last_message = Some(format!(
                        "Pasted {} characters into {}",
                        text.chars().count(),
                        completed.requested.tab_id
                    ));
                }
                Err(error) => {
                    self.last_error = Some(format!("Paste failed: {error:#}"));
                }
            }
            changed = true;
        }
        changed
    }

    fn load_composer(&self) {
        if self.cwd_editor_dialog.is_open() {
            return;
        }
        let text = self
            .active_tab()
            .and_then(|tab| tab.composer.text.as_deref())
            .unwrap_or_default();
        self.set_control_text(self.edit, text);
    }

    fn sync_composer(&mut self) -> Result<()> {
        if self.cwd_editor_dialog.is_open() {
            return Ok(());
        }
        let Some(tab_id) = self
            .client
            .as_ref()
            .and_then(|client| client.snapshot().active_tab_id.clone())
        else {
            return Ok(());
        };
        let text = self.control_text(self.edit);
        self.client
            .as_mut()
            .context("UI is disconnected")?
            .run_control(vec![
                "set-composer".to_owned(),
                "-t".to_owned(),
                tab_id,
                "--".to_owned(),
                text,
            ])?;
        Ok(())
    }

    fn send_composer(&mut self) {
        if self.cwd_editor_dialog.is_open() {
            self.finish_cwd_editor(true, ComposerWriteMode::Replace);
            return;
        }
        let Some(tab_id) = self
            .client
            .as_ref()
            .and_then(|client| client.snapshot().active_tab_id.clone())
        else {
            return;
        };
        let text = self.control_text(self.edit);
        let result =
            self.client
                .as_mut()
                .context("UI is disconnected")
                .and_then(|client| -> Result<()> {
                    client.run_control(vec![
                        "set-composer".to_owned(),
                        "-t".to_owned(),
                        tab_id.clone(),
                        "--".to_owned(),
                        text,
                    ])?;
                    client.run_control(vec![
                        "send-composer".to_owned(),
                        "-t".to_owned(),
                        tab_id,
                    ])?;
                    Ok(())
                });
        match result {
            Ok(()) => self.set_control_text(self.edit, ""),
            Err(error) => self.last_error = Some(format!("Composer send failed: {error:#}")),
        }
    }

    fn open_cwd_editor(&mut self) {
        if self.cwd_editor_dialog.is_open()
            || self.window_close_dialog.is_open()
            || self.settings_dialog.is_open()
            || self.new_terminal_dialog.is_open()
            || self.close_confirmation.is_open()
            || self.tab_editor_dialog.is_open()
        {
            return;
        }
        let Some(tab) = self.active_tab().cloned() else {
            return;
        };
        if let Err(error) = self.sync_composer() {
            self.last_error = Some(format!("Composer save failed: {error:#}"));
            return;
        }
        self.cancel_terminal_selection();
        self.cwd_editor_dialog.open(tab.id.clone());
        self.set_control_text(
            self.edit,
            tab.working_context.cwd.as_deref().unwrap_or_default(),
        );
        self.set_control_text(self.send, "Prepare");
        self.focus_control(self.edit);
        self.set_focus_surface_unchecked(RemoteFocusSurface::Composer);
        self.last_error = None;
    }

    fn finish_cwd_editor(&mut self, prepare: bool, mode: ComposerWriteMode) {
        let Some(tab_id) = self.cwd_editor_dialog.target().map(str::to_owned) else {
            return;
        };
        if prepare {
            let path = self.control_text(self.edit).trim().to_owned();
            if path.is_empty() {
                self.last_error = Some("CWD path cannot be empty".to_owned());
                return;
            }
            let result = self
                .client
                .as_mut()
                .context("UI is disconnected")
                .and_then(|client| {
                    let command = CwdEditorDialog::prepare_action(mode);
                    client.run_control(vec![
                        command.to_owned(),
                        "-t".to_owned(),
                        tab_id,
                        "--path".to_owned(),
                        path,
                    ])?;
                    client.poll_deltas()?;
                    Ok(())
                });
            if let Err(error) = result {
                self.last_error = Some(format!("CWD prepare failed: {error:#}"));
                return;
            }
        }
        self.cwd_editor_dialog.close();
        self.set_control_text(self.send, "Send");
        self.load_composer();
        self.set_focus_surface_unchecked(RemoteFocusSurface::Composer);
        self.focus_control(self.edit);
        self.last_error = None;
    }

    fn handle_cwd_editor_keydown(&mut self, key: u32, modifiers: input::ModifierState) -> bool {
        if !self.cwd_editor_dialog.is_open()
            || self.window.focused_target() != FocusTarget::Control(self.edit)
        {
            return false;
        }
        if key == u32::from(KEY_ESCAPE) {
            self.finish_cwd_editor(false, ComposerWriteMode::EmptyOnly);
            return true;
        }
        if key == 0x0d
            && let Some(mode) = CwdEditorDialog::submit_mode(modifiers)
        {
            self.finish_cwd_editor(true, mode);
            return true;
        }
        false
    }

    fn handle_tab_editor_keydown(&mut self, key: u32, modifiers: input::ModifierState) -> bool {
        if !self.tab_editor_dialog.is_open() {
            return false;
        }
        let focused = self.window.focused_target();
        if focused != FocusTarget::Control(self.tab_title_edit)
            && focused != FocusTarget::Control(self.tab_note_edit)
        {
            return false;
        }
        if key == u32::from(KEY_ESCAPE) {
            self.finish_tab_edit(false);
            return true;
        }
        if key == 0x0d && modifiers.control {
            self.finish_tab_edit(true);
            return true;
        }
        false
    }

    fn open_new_terminal(&mut self) {
        if self.new_terminal_dialog.is_open()
            || self.settings_dialog.is_open()
            || self.window_close_dialog.is_open()
            || self.close_confirmation.is_open()
        {
            return;
        }
        self.cancel_terminal_selection();
        if let Err(error) = self.sync_composer() {
            self.last_error = Some(format!("Composer save failed: {error:#}"));
            return;
        }
        self.finish_tab_edit(false);
        self.finish_cwd_editor(false, ComposerWriteMode::EmptyOnly);
        new_terminal::ui_action_open(&mut self.new_terminal_dialog);
        self.last_error = None;
        self.set_control_text(self.new_initial_command, "");
        self.set_control_text(self.new_http_proxy, "");
        self.set_control_text(self.new_https_proxy, "");
        self.sync_new_terminal_drafts();
        self.refresh_new_shell_controls();
        self.show_workspace_controls(false);
        self.layout_new_terminal_controls();
        self.focus_control(self.new_initial_command);
    }

    fn choose_new_shell(&mut self, choice: NewShellChoice) {
        if !self.new_terminal_dialog.is_open() {
            return;
        }
        new_terminal::ui_action_choose_shell(&mut self.new_terminal_dialog, choice);
        self.refresh_new_shell_controls();
    }

    fn refresh_new_shell_controls(&self) {
        for (control, choice, label) in [
            (self.new_default_shell, NewShellChoice::Default, "Default"),
            (
                self.new_cmd_shell,
                NewShellChoice::Primary,
                "Command Prompt",
            ),
            (self.new_powershell, NewShellChoice::Alternate, "PowerShell"),
        ] {
            let selected = self.new_terminal_dialog.shell_choice() == choice;
            self.set_control_text(
                control,
                &format!("{} {label}", if selected { "●" } else { "○" }),
            );
        }
    }

    fn sync_new_terminal_drafts(&mut self) {
        if !self.new_terminal_dialog.is_open() {
            return;
        }
        self.new_terminal_dialog
            .set_initial_command_draft(self.control_text(self.new_initial_command));
        self.new_terminal_dialog
            .set_http_proxy_draft(self.control_text(self.new_http_proxy));
        self.new_terminal_dialog
            .set_https_proxy_draft(self.control_text(self.new_https_proxy));
    }

    fn finish_new_terminal(&mut self, create: bool) {
        if !self.new_terminal_dialog.is_open() {
            return;
        }
        self.sync_new_terminal_drafts();
        let create_params = if create {
            match new_terminal::ui_action_create(&mut self.new_terminal_dialog) {
                Ok(Some(params)) => Some(params),
                Ok(None) => None,
                Err(error) => {
                    self.last_error = Some(error);
                    return;
                }
            }
        } else {
            new_terminal::ui_action_cancel(&mut self.new_terminal_dialog);
            None
        };
        if let Some(params) = create_params {
            let mut args = vec![
                "new-window".to_owned(),
                "-P".to_owned(),
                "-F".to_owned(),
                "#{window_id}".to_owned(),
            ];
            for (name, value) in &params.tab_environment {
                args.push("-e".to_owned());
                args.push(format!("{name}={value}"));
            }
            if !params.command_line.is_empty() {
                args.push("--".to_owned());
                args.extend(params.command_line);
            }
            let result = self.client.as_mut().context("UI is disconnected").and_then(
                |client| -> Result<()> {
                    client.run_control(args)?;
                    client.poll_deltas()?;
                    Ok(())
                },
            );
            if result.is_err() {
                self.last_error =
                    Some("New terminal could not be created; check its configuration".to_owned());
            }
        }
        self.show_new_terminal_controls(false);
        self.show_workspace_controls(true);
        self.layout();
        self.last_active_id = self
            .client
            .as_ref()
            .and_then(|client| client.snapshot().active_tab_id.clone());
        self.load_composer();
        self.resize_active_terminal();
        self.set_focus_surface_unchecked(RemoteFocusSurface::Terminal);
        self.window.focus();
    }

    fn select_tab_at(&mut self, y: i32) {
        if !self.tabs_visible {
            return;
        }
        let Some(row_index) = self.sidebar_row_index_at_y(y) else {
            return;
        };
        let Some(tab_id) = self
            .client
            .as_ref()
            .and_then(|client| {
                remote_tree_rows(&client.snapshot().tabs)
                    .get(row_index)
                    .map(|row| &client.snapshot().tabs[row.tab_index])
            })
            .map(|tab| tab.id.clone())
        else {
            return;
        };
        if self
            .client
            .as_ref()
            .and_then(|client| client.snapshot().active_tab_id.as_deref())
            != Some(tab_id.as_str())
        {
            self.invalidate_sidebar_text_click();
        }
        self.cancel_terminal_selection();
        if let Err(error) = self.sync_composer() {
            self.last_error = Some(format!("Composer save failed: {error:#}"));
            return;
        }
        let result = self
            .client
            .as_mut()
            .context("UI is disconnected")
            .and_then(|client| {
                client.select_tab(&tab_id)?;
                client.poll_deltas()?;
                Ok(())
            });
        if let Err(error) = result {
            self.last_error = Some(format!("Tab selection failed: {error:#}"));
        } else {
            self.set_focus_surface_unchecked(RemoteFocusSurface::Tabs);
            self.last_active_id = Some(tab_id);
            self.load_composer();
            self.resize_active_terminal();
        }
    }

    fn tab_at_y(&self, y: i32) -> Option<&UiTabBootstrap> {
        if !self.tabs_visible || y < 0 {
            return None;
        }
        let row_index = self.sidebar_row_index_at_y(y)?;
        let client = self.client.as_ref()?;
        let row = remote_tree_rows(&client.snapshot().tabs)
            .get(row_index)
            .cloned()?;
        client.snapshot().tabs.get(row.tab_index)
    }

    fn tab_action_at(&self, x: i32, y: i32) -> Option<RemoteTabAction> {
        let row_index = self.sidebar_row_index_at_y(y)?;
        let client = self.client.as_ref()?;
        let row = remote_tree_rows(&client.snapshot().tabs)
            .get(row_index)
            .cloned()?;
        let tab = client.snapshot().tabs.get(row.tab_index)?;
        if client.snapshot().active_tab_id.as_deref() != Some(tab.id.as_str()) {
            return None;
        }
        let viewport_position = row_index.checked_sub(self.sidebar_offset())?;
        let geometry = self.sidebar_row_geometry(viewport_position, row.depth, TreeRowMode::Normal);
        if geometry
            .actions
            .add_child
            .is_some_and(|bounds| bounds.contains(x, y))
        {
            Some(RemoteTabAction::AddChild)
        } else if geometry.actions.secondary.contains(x, y) {
            Some(RemoteTabAction::Close)
        } else {
            None
        }
    }

    fn tab_text_id_at(&self, x: i32, y: i32) -> Option<String> {
        let row_index = self.sidebar_row_index_at_y(y)?;
        let client = self.client.as_ref()?;
        let row = remote_tree_rows(&client.snapshot().tabs)
            .get(row_index)
            .cloned()?;
        let viewport_position = row_index.checked_sub(self.sidebar_offset())?;
        self.sidebar_row_geometry(viewport_position, row.depth, TreeRowMode::Normal)
            .text
            .contains(x, y)
            .then(|| client.snapshot().tabs[row.tab_index].id.clone())
    }

    fn record_sidebar_text_click(&mut self, tab_id: String) {
        self.recent_sidebar_text_click = Some(RemoteSidebarTextClick {
            tab_id,
            geometry_generation: self.sidebar_geometry_generation,
        });
    }

    fn invalidate_sidebar_text_click(&mut self) {
        self.sidebar_geometry_generation = self.sidebar_geometry_generation.wrapping_add(1);
        self.recent_sidebar_text_click = None;
    }

    fn take_matching_sidebar_text_click(&mut self, x: i32, y: i32) -> Option<String> {
        let candidate = self.recent_sidebar_text_click.take()?;
        let tab_id = self.tab_text_id_at(x, y)?;
        candidate
            .matches(&tab_id, self.sidebar_geometry_generation)
            .then_some(tab_id)
    }

    fn begin_tab_edit_id(&mut self, tab_id: &str) {
        let tab = self.client.as_ref().and_then(|client| {
            client
                .snapshot()
                .tabs
                .iter()
                .find(|tab| tab.id == tab_id)
                .cloned()
        });
        if let Some(tab) = tab {
            self.begin_tab_edit(tab);
        }
    }

    fn tab_disclosure_at(&self, x: i32, y: i32) -> Option<String> {
        let row_index = self.sidebar_row_index_at_y(y)?;
        let client = self.client.as_ref()?;
        let row = remote_tree_rows(&client.snapshot().tabs)
            .get(row_index)
            .cloned()?;
        if !row.has_children {
            return None;
        }
        let viewport_position = row_index.checked_sub(self.sidebar_offset())?;
        let geometry = self.sidebar_row_geometry(viewport_position, row.depth, TreeRowMode::Normal);
        geometry
            .disclosure_hit
            .contains(x, y)
            .then(|| client.snapshot().tabs[row.tab_index].id.clone())
    }

    fn toggle_tree_at(&mut self, x: i32, y: i32) -> bool {
        let Some(tab_id) = self.tab_disclosure_at(x, y) else {
            return false;
        };
        self.invalidate_sidebar_text_click();
        let result = self
            .client
            .as_mut()
            .context("UI is disconnected")
            .and_then(|client| {
                client.invoke_client_action(vec![
                    "ui-action".to_owned(),
                    "toggle-tree".to_owned(),
                    "-t".to_owned(),
                    tab_id,
                ])?;
                client.poll_deltas()?;
                Ok(())
            });
        if let Err(error) = result {
            self.last_error = Some(format!("Toggle tree failed: {error:#}"));
        } else {
            self.last_error = None;
            self.reconcile_tab_editor();
        }
        true
    }

    fn new_child_at(&mut self, y: i32) {
        let Some(parent_id) = self.tab_at_y(y).map(|tab| tab.id.clone()) else {
            return;
        };
        self.cancel_terminal_selection();
        if let Err(error) = self.sync_composer() {
            self.last_error = Some(format!("Composer save failed: {error:#}"));
            return;
        }
        let result = self
            .client
            .as_mut()
            .context("UI is disconnected")
            .and_then(|client| {
                client.invoke_client_action(vec![
                    "ui-action".to_owned(),
                    "new-child".to_owned(),
                    "-t".to_owned(),
                    parent_id,
                ])?;
                client.poll_deltas()?;
                Ok(())
            });
        if let Err(error) = result {
            self.last_error = Some(format!("Add child failed: {error:#}"));
        } else {
            self.last_active_id = self
                .client
                .as_ref()
                .and_then(|client| client.snapshot().active_tab_id.clone());
            self.load_composer();
            self.resize_active_terminal();
            if let Some(child) = self.active_tab().cloned() {
                self.begin_tab_edit(child);
            }
        }
    }

    fn request_close_tab_at(&mut self, y: i32) {
        let Some(tab) = self.tab_at_y(y).cloned() else {
            return;
        };
        self.cancel_terminal_selection();
        if tab.dead {
            let _ = self.close_tab_now(tab.id);
            return;
        }
        if let Err(error) = self.sync_composer() {
            self.last_error = Some(format!("Composer save failed: {error:#}"));
            return;
        }
        self.finish_tab_edit(false);
        self.close_confirmation.open(tab.id);
        self.show_workspace_controls(false);
        self.layout_tab_close_controls();
        self.window.focus();
    }

    fn close_tab_now(&mut self, tab_id: String) -> bool {
        let result = self
            .client
            .as_mut()
            .context("UI is disconnected")
            .and_then(|client| {
                client.run_control(vec!["kill-window".to_owned(), "-t".to_owned(), tab_id])?;
                client.poll_deltas()?;
                Ok(())
            });
        if let Err(error) = result {
            self.last_error = Some(format!("Close tab failed: {error:#}"));
            false
        } else {
            self.last_error = None;
            self.last_active_id = self
                .client
                .as_ref()
                .and_then(|client| client.snapshot().active_tab_id.clone());
            self.load_composer();
            self.resize_active_terminal();
            true
        }
    }

    fn finish_close_tab(&mut self, confirm: bool) {
        let pending = self.close_confirmation.target().map(str::to_owned);
        if confirm
            && let Some(tab_id) = pending
            && !self.close_tab_now(tab_id)
        {
            return;
        }
        self.close_confirmation.close();
        self.show_tab_close_controls(false);
        self.show_workspace_controls(true);
        self.set_focus_surface_unchecked(RemoteFocusSurface::Tabs);
        self.window.focus();
    }

    fn sync_tab_editor_drafts(&mut self) {
        if !self.tab_editor_dialog.is_open() {
            return;
        }
        self.tab_editor_dialog
            .set_name_draft(self.control_text(self.tab_title_edit));
        self.tab_editor_dialog
            .set_note_draft(self.control_text(self.tab_note_edit));
    }

    fn begin_tab_edit(&mut self, tab: UiTabBootstrap) {
        self.set_focus_surface_unchecked(RemoteFocusSurface::Tabs);
        let tab_id = tab.id.clone();
        let title = tab.title.clone();
        let note = tab.note.clone();
        self.tab_editor_dialog
            .open(tab_id, title.clone(), note.clone());
        self.set_control_text(self.tab_title_edit, &title);
        self.set_control_text(self.tab_note_edit, &note);
        self.layout_tab_editor();
        self.focus_control(self.tab_title_edit);
    }

    fn finish_tab_edit(&mut self, save: bool) {
        if !self.tab_editor_dialog.is_open() {
            return;
        }
        self.sync_tab_editor_drafts();
        let Some(tab_id) = self.tab_editor_dialog.target().map(str::to_owned) else {
            return;
        };
        if save {
            let (title, note) = match self.tab_editor_dialog.capture(true) {
                Ok(Some(changes)) => (changes.name, changes.note),
                Ok(None) => return,
                Err(error) => {
                    self.last_error = Some(error);
                    return;
                }
            };
            let result = self
                .client
                .as_mut()
                .context("UI is disconnected")
                .and_then(|client| {
                    client.run_control(vec![
                        "rename-window".to_owned(),
                        "-t".to_owned(),
                        tab_id.clone(),
                        title,
                    ])?;
                    client.run_control(vec![
                        "set-tab-note".to_owned(),
                        "-t".to_owned(),
                        tab_id,
                        note,
                    ])?;
                    client.poll_deltas()?;
                    Ok(())
                });
            if let Err(error) = result {
                self.last_error = Some(format!("Tab edit failed: {error:#}"));
                return;
            }
        }
        self.tab_editor_dialog.close();
        self.show_tab_editor(false);
        self.set_focus_surface_unchecked(RemoteFocusSurface::Tabs);
        self.window.focus();
    }

    fn open_settings(&mut self) {
        if self.settings_dialog.is_open()
            || self.new_terminal_dialog.is_open()
            || self.window_close_dialog.is_open()
        {
            return;
        }
        self.cancel_terminal_selection();
        if let Err(error) = self.sync_composer() {
            self.last_error = Some(format!("Composer save failed: {error:#}"));
            return;
        }
        self.finish_tab_edit(false);
        let target_tab_id = self
            .client
            .as_ref()
            .and_then(|client| client.snapshot().active_tab_id.clone());
        let override_draft = target_tab_id
            .as_deref()
            .map(|tab_id| self.config.terminal_override(&ipc_address(), tab_id))
            .unwrap_or_default();
        settings::ui_action_open(
            &mut self.settings_dialog,
            self.config
                .effective_terminal_appearance(&ipc_address(), None),
            target_tab_id,
            override_draft,
        );
        self.last_error = None;
        self.load_settings_scope_controls();
        self.show_workspace_controls(false);
        self.layout_settings_controls();
        self.focus_control(self.settings_font);
    }

    fn sync_settings_drafts(&mut self) {
        if !self.settings_dialog.is_open() {
            return;
        }
        self.settings_dialog
            .set_font_family_draft(self.control_text(self.settings_font));
        self.settings_dialog
            .set_font_size_draft(self.control_text(self.settings_size));
    }

    fn capture_settings_scope(&mut self) -> Result<()> {
        self.sync_settings_drafts();
        self.settings_dialog.capture().map_err(|error| {
            self.last_error = Some(error.clone());
            anyhow::Error::msg(error)
        })
    }

    fn switch_settings_scope(&mut self, scope: SettingsScope) {
        self.sync_settings_drafts();
        match self.settings_dialog.switch_scope(scope) {
            Ok(true) => {
                self.load_settings_scope_controls();
                self.layout_settings_controls();
            }
            Ok(false) => {}
            Err(error) => {
                self.last_error = Some(format!("Settings draft invalid: {error:#}"));
            }
        }
    }

    fn load_settings_scope_controls(&mut self) {
        let locale = self.config.locale;
        self.settings_dialog.load_effective_drafts();
        let family = self.settings_dialog.font_family_draft().to_owned();
        let size = self.settings_dialog.font_size_draft().to_owned();
        self.set_control_text(self.settings_font, &family);
        self.set_control_text(self.settings_size, &size);
        self.set_control_enabled(
            self.settings_current_scope,
            self.settings_dialog.target_tab_id().is_some(),
        );
        let current = self.settings_dialog.scope() == SettingsScope::CurrentTerminal;
        let override_draft = self.settings_dialog.override_draft();
        self.set_control_enabled(
            self.settings_font,
            !current || override_draft.terminal_font_family.is_some(),
        );
        self.set_control_enabled(
            self.settings_size,
            !current || override_draft.terminal_font_size.is_some(),
        );
        let theme_enabled = !current || override_draft.color_theme.is_some();
        self.set_control_enabled(self.settings_dark, theme_enabled);
        self.set_control_enabled(self.settings_light, theme_enabled);
        for (control, overridden) in [
            (
                self.settings_font_inherit,
                override_draft.terminal_font_family.is_some(),
            ),
            (
                self.settings_size_inherit,
                override_draft.terminal_font_size.is_some(),
            ),
            (
                self.settings_theme_inherit,
                override_draft.color_theme.is_some(),
            ),
        ] {
            self.set_control_text(
                control,
                locale.text(if overridden {
                    UiText::InheritDefault
                } else {
                    UiText::Override
                }),
            );
        }
        self.set_control_text(
            self.settings_default_scope,
            &format!(
                "{}{}",
                locale.text(UiText::DefaultValues),
                if self.settings_dialog.scope() == SettingsScope::Defaults {
                    " · ✓"
                } else {
                    ""
                }
            ),
        );
        self.set_control_text(
            self.settings_current_scope,
            &format!(
                "{}{}",
                locale.text(UiText::CurrentTerminal),
                if self.settings_dialog.scope() == SettingsScope::CurrentTerminal {
                    " · ✓"
                } else {
                    ""
                }
            ),
        );
        self.refresh_settings_theme_controls();
    }

    fn toggle_settings_inheritance(&mut self, field: AppearanceField) {
        self.sync_settings_drafts();
        match self.settings_dialog.toggle_inheritance(field) {
            Ok(true) => self.load_settings_scope_controls(),
            Ok(false) => {}
            Err(error) => {
                self.last_error = Some(format!("Settings draft invalid: {error:#}"));
            }
        }
    }

    fn reset_settings_overrides(&mut self) {
        self.sync_settings_drafts();
        if self.settings_dialog.reset_overrides() {
            self.load_settings_scope_controls();
        }
    }

    fn preview_settings_theme(&mut self, theme: ThemeId) {
        if self.settings_dialog.preview_theme(theme) {
            self.refresh_settings_theme_controls();
        }
    }

    fn refresh_settings_theme_controls(&self) {
        let theme_draft = self.settings_dialog.theme_draft();
        for (theme, control) in [
            (ThemeId::Dark, self.settings_dark),
            (ThemeId::Light, self.settings_light),
        ] {
            let state = if theme == theme_draft {
                self.config.locale.text(UiText::Selected)
            } else {
                self.config.locale.text(UiText::Preview)
            };
            self.set_control_text(
                control,
                &format!(
                    "{} · {state}",
                    self.config.locale.text(match theme {
                        ThemeId::Dark => UiText::ThemeDark,
                        ThemeId::Light => UiText::Light,
                    })
                ),
            );
        }
    }

    fn apply_settings(&mut self) -> Result<()> {
        self.capture_settings_scope()?;
        let changes = self.settings_dialog.changes();
        let mut next = self.config.clone();
        next.terminal_font_family = changes.default_appearance.terminal_font_family.clone();
        next.terminal_font_size = changes.default_appearance.terminal_font_size;
        next.color_theme = changes.default_appearance.color_theme;
        if let Some(tab_id) = changes.target_tab_id.as_deref() {
            next.set_terminal_override(&ipc_address(), tab_id, changes.override_draft.clone());
        }
        let active = self
            .client
            .as_ref()
            .and_then(|client| client.snapshot().active_tab_id.as_deref());
        let appearance = next.effective_terminal_appearance(&ipc_address(), active);
        let (font, cell_width, cell_height) = create_terminal_font(
            &self.window,
            &appearance.terminal_font_family,
            appearance.terminal_font_size,
        )?;
        save_config(&next).context("could not save settings")?;
        self.font = font;
        self.cell_width = cell_width;
        self.cell_height = cell_height;
        self.config = next;
        self.last_error = None;
        Ok(())
    }

    fn finish_settings(&mut self, apply: bool) {
        if !self.settings_dialog.is_open() {
            return;
        }
        if apply && let Err(error) = self.apply_settings() {
            self.last_error = Some(format!("Settings apply failed: {error:#}"));
            return;
        }
        self.settings_dialog.close_without_apply();
        self.show_settings_controls(false);
        self.show_workspace_controls(true);
        self.set_focus_surface_unchecked(RemoteFocusSurface::Terminal);
        self.layout();
        self.load_composer();
        self.resize_active_terminal();
        self.window.focus();
    }

    fn request_window_close(&mut self) {
        if self.window_close_dialog.is_open() {
            return;
        }
        if self.cwd_editor_dialog.is_open() {
            self.finish_cwd_editor(false, ComposerWriteMode::EmptyOnly);
        }
        if self.close_confirmation.is_open() {
            self.finish_close_tab(false);
            return;
        }
        self.cancel_terminal_selection();
        if self.settings_dialog.is_open() {
            self.finish_settings(false);
        }
        if self.new_terminal_dialog.is_open() {
            self.finish_new_terminal(false);
        }
        if self.client.is_some()
            && let Err(error) = self.sync_composer()
        {
            self.last_error = Some(format!("Composer save failed: {error:#}"));
        }
        self.finish_tab_edit(false);
        self.window_close_dialog.open();
        self.show_workspace_controls(false);
        self.layout_close_controls();
        // A close request can arrive from the taskbar while this window is
        // minimized.  The confirmation choices are native child controls, so
        // leaving the parent iconic would make the required decision
        // unreachable and look like a failed close.
        if self.window.state().minimized {
            self.window.set_presentation(WindowPresentation::Restored);
        }
        self.window.focus();
    }

    fn finish_window_close(&mut self, choice: WindowCloseChoice) {
        if !self.window_close_dialog.is_open() {
            return;
        }
        match choice {
            WindowCloseChoice::KeepServerRunning => {
                self.window_close_dialog.close();
                self.window.close();
            }
            WindowCloseChoice::StopServerAndExit => {
                let result =
                    self.client
                        .as_mut()
                        .context("UI is disconnected")
                        .and_then(|client| {
                            client.run_control(vec!["shutdown".to_owned()])?;
                            Ok(())
                        });
                if let Err(error) = result {
                    self.last_error = Some(format!("Server shutdown failed: {error:#}"));
                    self.window_close_dialog.close();
                    self.show_close_controls(false);
                    self.show_workspace_controls(true);
                    self.window.focus();
                    return;
                }
                self.window_close_dialog.close();
                self.window.close();
            }
            WindowCloseChoice::Cancel => {
                self.window_close_dialog.close();
                self.show_close_controls(false);
                self.show_workspace_controls(true);
                self.set_focus_surface_unchecked(RemoteFocusSurface::Terminal);
                self.window.focus();
            }
        }
    }

    fn terminal_point(&self, x: i32, y: i32, clamp: bool) -> Option<RemotePoint> {
        let (_, terminal, _, _) = self.layout_rects();
        let tab = self.active_tab()?;
        if !clamp
            && (x < terminal.left
                || x >= terminal.right - TERMINAL_SCROLLBAR_WIDTH
                || y < terminal.top
                || y >= terminal.bottom)
        {
            return None;
        }
        let local_x = (x - terminal.left).clamp(
            0,
            (terminal.right - terminal.left - TERMINAL_SCROLLBAR_WIDTH - 1).max(0),
        );
        let local_y = (y - terminal.top).clamp(0, (terminal.bottom - terminal.top - 1).max(0));
        Some(RemotePoint {
            row: u32::try_from(local_y / self.cell_height)
                .unwrap_or_default()
                .min(tab.screen.rows.saturating_sub(1)),
            column: u32::try_from(local_x / self.cell_width)
                .unwrap_or_default()
                .min(tab.screen.columns.saturating_sub(1)),
        })
    }

    fn begin_terminal_selection(&mut self, x: i32, y: i32) -> bool {
        let Some(point) = self.terminal_point(x, y, false) else {
            return false;
        };
        let Some((tab_id, rows, columns)) = self
            .active_tab()
            .map(|tab| (tab.id.clone(), tab.screen.rows, tab.screen.columns))
        else {
            return false;
        };
        self.set_focus_surface_unchecked(RemoteFocusSurface::Terminal);
        self.window.focus();
        if let Err(error) = self.window.set_pointer_capture(true) {
            self.terminal_selection = None;
            self.last_error = Some(format!("Selection failed: {error}"));
            return true;
        }
        self.terminal_selection = Some(RemoteTerminalSelection {
            tab_id: tab_id.clone(),
            rows,
            columns,
            gesture: RemoteSelectionGesture::begin(tab_id.clone(), point),
            cached_text: None,
        });
        self.terminal_selection_pointer = Some((x, y));
        self.terminal_selection_autoscroll = None;
        self.terminal_double_click = None;
        self.recent_terminal_click = Some(RecentRemoteTerminalClick {
            tab_id: tab_id.clone(),
            point,
            at: Instant::now(),
        });
        self.last_error = None;
        true
    }

    fn drag_terminal_selection(&mut self, x: i32, y: i32) -> bool {
        if !self
            .terminal_selection
            .as_ref()
            .is_some_and(RemoteTerminalSelection::active_gesture)
        {
            return false;
        }
        let Some(point) = self.terminal_point(x, y, true) else {
            return false;
        };
        if let Some(selection) = self.terminal_selection.as_mut() {
            selection.drag_to(point);
        }
        let terminal = self.workspace_geometry().terminal;
        self.terminal_selection_pointer = Some((x, y));
        self.terminal_selection_autoscroll =
            autoscroll_step(y, terminal.top, terminal.bottom, self.cell_height.max(1));
        true
    }

    fn finish_terminal_selection(&mut self, x: i32, y: i32) -> bool {
        if !self.drag_terminal_selection(x, y) {
            return false;
        }
        self.clear_terminal_selection_autoscroll();
        let completed = self
            .terminal_selection
            .as_mut()
            .is_some_and(RemoteTerminalSelection::complete);
        // Mark the gesture completed before ReleaseCapture synchronously emits
        // WM_CAPTURECHANGED; only an unfinished gesture is cancelled there.
        if let Err(error) = self.window.set_pointer_capture(false) {
            self.terminal_selection = None;
            self.last_error = Some(format!(
                "Selection failed to release pointer capture: {error}"
            ));
            return true;
        }
        if !completed {
            self.terminal_selection = None;
            return true;
        }
        self.cache_terminal_selection_text();
        true
    }

    fn terminal_selection_capture_lost(&mut self) {
        if self
            .terminal_selection
            .as_ref()
            .is_some_and(RemoteTerminalSelection::active_gesture)
        {
            if let Some(selection) = self.terminal_selection.as_mut() {
                selection.cancel();
            }
            self.terminal_selection = None;
            self.clear_terminal_selection_autoscroll();
        }
    }

    fn cancel_terminal_selection(&mut self) {
        let captured = self
            .terminal_selection
            .as_ref()
            .is_some_and(RemoteTerminalSelection::active_gesture);
        if let Some(selection) = self.terminal_selection.as_mut() {
            selection.cancel();
        }
        self.terminal_selection = None;
        self.clear_terminal_selection_autoscroll();
        if captured && let Err(error) = self.window.set_pointer_capture(false) {
            self.last_error = Some(format!(
                "Selection failed to release pointer capture: {error}"
            ));
        }
    }

    fn clear_terminal_selection_autoscroll(&mut self) {
        self.terminal_selection_pointer = None;
        self.terminal_selection_autoscroll = None;
    }

    fn tick_terminal_selection_autoscroll(&mut self) -> bool {
        let Some(step) = self.terminal_selection_autoscroll else {
            return false;
        };
        if !self
            .terminal_selection
            .as_ref()
            .is_some_and(RemoteTerminalSelection::active_gesture)
        {
            self.clear_terminal_selection_autoscroll();
            return false;
        }
        let Some((x, y)) = self.terminal_selection_pointer else {
            self.clear_terminal_selection_autoscroll();
            return false;
        };
        let Some(tab) = self.active_tab() else {
            self.clear_terminal_selection_autoscroll();
            return false;
        };
        let tab_id = tab.id.clone();
        let action = match step.direction {
            AutoScrollDirection::Up => "up",
            AutoScrollDirection::Down => "down",
        };
        let terminal = self.workspace_geometry().terminal;
        let result = self
            .client
            .as_mut()
            .context("UI is disconnected")
            .and_then(|client| {
                client.run_control(vec![
                    "scroll-pane".to_owned(),
                    "-t".to_owned(),
                    tab_id,
                    action.to_owned(),
                    step.rows.to_string(),
                ])?;
                client.poll_deltas()
            });
        match result {
            Ok(_) => {
                if let Some(point) = self.terminal_point(x, y, true)
                    && let Some(selection) = self.terminal_selection.as_mut()
                {
                    selection.drag_to(point);
                }
                self.terminal_selection_autoscroll =
                    autoscroll_step(y, terminal.top, terminal.bottom, self.cell_height.max(1));
                true
            }
            Err(error) => {
                self.last_error = Some(format!("Selection autoscroll failed: {error:#}"));
                self.clear_terminal_selection_autoscroll();
                false
            }
        }
    }
    fn copy_terminal_selection(&mut self) -> Result<()> {
        let selection = self
            .terminal_selection
            .as_ref()
            .context("no terminal selection is active")?;
        if !selection.can_copy() {
            anyhow::bail!("terminal selection is not complete or contains no text");
        }
        self.active_tab()
            .filter(|tab| selection.matches_screen(&tab.screen))
            .context("terminal selection is stale")?;
        let text = selection
            .cached_text
            .as_deref()
            .context("terminal selection contains no text")?;
        clipboard::set_text(text).map_err(|error| anyhow::anyhow!(error))?;
        self.last_error = None;
        Ok(())
    }

    fn paste_terminal_clipboard(&mut self) -> Result<()> {
        if self.pending_terminal_paste.is_some() {
            anyhow::bail!("a terminal clipboard read is already pending");
        }
        if self.window.focused_target() != FocusTarget::Window
            || self.current_focus_surface() != RemoteFocusSurface::Terminal
        {
            anyhow::bail!("paste requires terminal focus");
        }
        let client = self
            .client
            .as_ref()
            .context("replaceable UI is disconnected")?;
        let snapshot = client.snapshot();
        let tab_id = snapshot
            .active_tab_id
            .clone()
            .context("no active terminal is available for paste")?;
        let tab = snapshot
            .tabs
            .iter()
            .find(|tab| tab.id == tab_id)
            .context("the active terminal is unavailable for paste")?;
        let requested = RemoteTerminalPasteRequest {
            server_epoch: snapshot.server_epoch.clone(),
            tab_id,
            bracketed: tab.screen.bracketed_paste,
        };
        self.terminal_paste_worker
            .queue(RemoteTerminalPasteTask {
                requested: requested.clone(),
                read: Box::new(|| {
                    let raw = clipboard::get_text(TERMINAL_PASTE_LIMIT_BYTES)
                        .map_err(|error| error.to_string())?;
                    let text = normalize_terminal_paste(&raw);
                    if text.is_empty() {
                        return Err("clipboard text contains no pasteable characters".to_owned());
                    }
                    if text.len() > TERMINAL_PASTE_LIMIT_BYTES {
                        return Err(format!(
                            "normalized clipboard text exceeds the {TERMINAL_PASTE_LIMIT_BYTES}-byte limit"
                        ));
                    }
                    Ok(text)
                }),
            })
            .map_err(anyhow::Error::msg)?;
        self.pending_terminal_paste = Some(requested);
        self.last_error = None;
        Ok(())
    }

    fn is_edit_control(&self, control: ControlId) -> bool {
        [
            self.edit,
            self.tab_title_edit,
            self.tab_note_edit,
            self.settings_font,
            self.settings_size,
            self.new_initial_command,
            self.new_http_proxy,
            self.new_https_proxy,
        ]
        .contains(&control)
    }

    fn current_focus_surface(&self) -> RemoteFocusSurface {
        if self.window.focused_target() == FocusTarget::Control(self.edit) {
            RemoteFocusSurface::Composer
        } else {
            RemoteFocusSurface::from_shared(self.focus_state.surface())
        }
    }

    fn focus_gate(&self) -> FocusTransitionGate {
        FocusTransitionGate {
            window_close_pending: self.window_close_dialog.is_open(),
            settings_open: self.settings_dialog.is_open(),
            new_terminal_open: self.new_terminal_dialog.is_open(),
            tab_editor_open: self.tab_editor_dialog.is_open(),
            close_confirmation_open: self.close_confirmation.is_open(),
            cwd_editor_open: self.cwd_editor_dialog.is_open(),
        }
    }

    fn set_focus_surface_unchecked(&mut self, target: RemoteFocusSurface) {
        let gate = self.focus_gate();
        self.focus_surface = target;
        self.focus_state = FocusState::new(target.to_shared(), gate);
    }

    fn native_focus_surface_code(&self) -> isize {
        let focused = self.window.focused_target();
        if focused == FocusTarget::Control(self.edit) {
            return 2;
        }
        if focused != FocusTarget::Window {
            return 0;
        }
        match self.focus_surface {
            RemoteFocusSurface::Terminal => 1,
            RemoteFocusSurface::Composer => 2,
            RemoteFocusSurface::Tabs => 3,
        }
    }

    fn set_focus_surface(&mut self, target: RemoteFocusSurface) -> bool {
        let mut state =
            FocusState::new(self.current_focus_surface().to_shared(), self.focus_gate());
        if !state.transition(target.to_shared()) {
            return false;
        }
        match target {
            RemoteFocusSurface::Terminal => {
                self.set_focus_surface_unchecked(target);
                self.window.focus();
            }
            RemoteFocusSurface::Composer => {
                if self.active_tab().is_none() {
                    return false;
                }
                self.set_focus_surface_unchecked(target);
                self.focus_control(self.edit);
            }
            RemoteFocusSurface::Tabs => {
                if !self.tabs_visible {
                    self.set_tabs_visible(true);
                }
                self.set_focus_surface_unchecked(target);
                self.window.focus();
            }
        }
        true
    }

    fn handle_surface_navigation(
        &mut self,
        key: u32,
        control: bool,
        shift: bool,
        alt: bool,
    ) -> bool {
        let Some(direction) = FocusDirection::from_virtual_key_code(key) else {
            return false;
        };
        let state = FocusState::new(self.current_focus_surface().to_shared(), self.focus_gate());
        let Some(target) = state.navigate(direction, control, shift, alt) else {
            return false;
        };
        self.set_focus_surface(RemoteFocusSurface::from_shared(target))
    }

    fn handle_keyboard_navigation_with_modifiers(
        &mut self,
        key: u32,
        modifiers: input::ModifierState,
    ) -> bool {
        self.handle_surface_navigation(key, modifiers.control, modifiers.shift, modifiers.alt)
    }

    fn system_menu_state(&self) -> (bool, bool) {
        let focused = self.window.focused_target();
        if let FocusTarget::Control(control) = focused
            && self.is_edit_control(control)
        {
            return (true, clipboard::has_unicode_text());
        }
        let terminal_ready = focused == FocusTarget::Window
            && !self.window_close_dialog.is_open()
            && !self.settings_dialog.is_open()
            && !self.new_terminal_dialog.is_open()
            && self.active_tab().is_some_and(|tab| !tab.dead);
        (
            terminal_ready
                && self
                    .terminal_selection
                    .as_ref()
                    .is_some_and(RemoteTerminalSelection::can_copy),
            terminal_ready && clipboard::has_unicode_text(),
        )
    }

    fn refresh_system_menu(&self) {
        let (copy, paste) = self.system_menu_state();
        let _ = self
            .window
            .set_system_menu_checked(SYSTEM_MENU_TOGGLE_TABS_ID, self.tabs_visible);
        let _ = self
            .window
            .set_system_menu_enabled(SYSTEM_MENU_COPY_ID, copy);
        let _ = self
            .window
            .set_system_menu_enabled(SYSTEM_MENU_PASTE_ID, paste);
    }

    fn system_menu_copy(&mut self) {
        let focused = self.window.focused_target();
        if let FocusTarget::Control(control) = focused
            && self.is_edit_control(control)
        {
            if let Err(error) = self.window.copy_control_selection(control) {
                self.last_error = Some(format!("Copy failed: {error}"));
            }
        } else if let Err(error) = self.copy_terminal_selection() {
            self.last_error = Some(format!("Copy failed: {error:#}"));
        }
    }

    fn system_menu_paste(&mut self) {
        let focused = self.window.focused_target();
        if let FocusTarget::Control(control) = focused
            && self.is_edit_control(control)
        {
            if let Err(error) = self.window.paste_control_selection(control) {
                self.last_error = Some(format!("Paste failed: {error}"));
            }
        } else if let Err(error) = self.paste_terminal_clipboard() {
            self.last_error = Some(format!("Paste failed: {error:#}"));
        }
    }

    fn terminal_input(&mut self, bytes: &[u8]) {
        self.cancel_terminal_selection();
        let Some(tab_id) = self
            .client
            .as_ref()
            .and_then(|client| client.snapshot().active_tab_id.clone())
        else {
            return;
        };
        if let Some(client) = self.client.as_mut()
            && let Err(error) = client.send_input(&tab_id, bytes)
        {
            self.last_error = Some(format!("Terminal input failed: {error:#}"));
        }
    }

    fn remote_cell(&self, x: i32, y: i32) -> Option<(u16, u16)> {
        let tab = self.active_tab()?;
        terminal_cell_at(
            self.workspace_geometry().terminal,
            x,
            y,
            u16::try_from(tab.screen.rows).ok()?,
            u16::try_from(tab.screen.columns).ok()?,
            self.cell_width,
            self.cell_height,
        )
    }

    /// Forwards one pointer event to the running application when it
    /// negotiated xterm mouse tracking on the active tab.
    ///
    /// Shift bypasses reporting so local selection and scrollback stay
    /// reachable (the xterm convention), and reports are suppressed while the
    /// viewport is scrolled back because reported cells would not match what
    /// the application drew.
    fn forward_terminal_mouse(
        &mut self,
        x: i32,
        y: i32,
        button: Option<u8>,
        pressed: bool,
        motion: bool,
    ) -> bool {
        if self.settings_dialog.is_open()
            || self.window_close_dialog.is_open()
            || self.new_terminal_dialog.is_open()
            || self.close_confirmation.is_open()
        {
            return false;
        }
        let Some(tab) = self.active_tab() else {
            return false;
        };
        let product_mode = mouse_protocol_mode_from_str(&tab.screen.mouse_protocol_mode);
        let encoding = mouse_report_encoding_from_str(&tab.screen.mouse_protocol_encoding);
        let dragging = self.mouse_report_button.is_some();
        let Some((column, row)) = self.remote_cell(x, y) else {
            return false;
        };
        let input = MouseReportInput {
            mode: product_mode,
            encoding,
            shift: self.pointer_modifiers.shift,
            alt: self.pointer_modifiers.alt,
            control: self.pointer_modifiers.control,
            scrolled_back: tab.screen.scrollback_offset != 0,
            motion,
            dragging,
            pressed,
            button,
            current_button: self.mouse_report_button,
            current_cell: self.mouse_report_cell,
            column,
            row,
        };
        match mouse_report_outcome(input) {
            MouseReportOutcome::LocalSelection => false,
            MouseReportOutcome::Deduplicated => true,
            MouseReportOutcome::Send(bytes) => {
                self.terminal_input(&bytes);
                self.mouse_report_cell = Some((column, row));
                true
            }
        }
    }

    fn scroll_terminal(&mut self, wheel_notches: i32) {
        self.cancel_terminal_selection();
        let Some(tab) = self.active_tab() else {
            return;
        };
        let tab_id = tab.id.clone();
        let count = wheel_notches.unsigned_abs() as usize * WHEEL_ROWS_PER_NOTCH;
        let before = tab.screen.scrollback_offset;
        let alternate_screen = tab.screen.alternate_screen;
        let application_cursor = tab.screen.application_cursor;
        let full_screen_input = alternate_screen || application_cursor;
        if tab.screen.max_scrollback == 0 {
            if full_screen_input {
                self.terminal_input(&alternate_screen_wheel_bytes(
                    wheel_notches > 0,
                    count,
                    application_cursor,
                ));
            }
            return;
        }
        let action = if wheel_notches > 0 { "up" } else { "down" };
        let result = self
            .client
            .as_mut()
            .context("UI is disconnected")
            .and_then(|client| {
                client.run_control(vec![
                    "scroll-pane".to_owned(),
                    "-t".to_owned(),
                    tab_id,
                    action.to_owned(),
                    count.to_string(),
                ])?;
                client.poll_deltas()?;
                Ok(())
            });
        match result {
            Ok(()) => {
                let scrolled = self
                    .active_tab()
                    .is_some_and(|tab| tab.screen.scrollback_offset != before);
                if !scrolled && full_screen_input {
                    self.terminal_input(&alternate_screen_wheel_bytes(
                        wheel_notches > 0,
                        count,
                        application_cursor,
                    ));
                }
            }
            Err(error) => {
                self.last_error = Some(format!("Terminal scroll failed: {error:#}"));
            }
        }
    }

    fn scrollbar_state(&self) -> Option<(TerminalScrollbarGeometry, usize, usize)> {
        let tab = self.active_tab()?;
        let geometry = terminal_scrollbar_geometry(
            self.workspace_geometry().terminal,
            usize::try_from(tab.screen.rows).unwrap_or_default(),
            tab.screen.scrollback_offset,
            tab.screen.max_scrollback,
        );
        Some((
            geometry,
            tab.screen.scrollback_offset,
            tab.screen.max_scrollback,
        ))
    }

    fn set_scrollback(&mut self, requested: usize) {
        let Some(tab) = self.active_tab() else {
            return;
        };
        let tab_id = tab.id.clone();
        let current = tab.screen.scrollback_offset;
        let target = requested.min(tab.screen.max_scrollback);
        if target == current {
            return;
        }
        self.cancel_terminal_selection();
        let action = if target > current { "up" } else { "down" };
        let count = target.abs_diff(current);
        let result = self
            .client
            .as_mut()
            .context("UI is disconnected")
            .and_then(|client| {
                client.run_control(vec![
                    "scroll-pane".to_owned(),
                    "-t".to_owned(),
                    tab_id,
                    action.to_owned(),
                    count.to_string(),
                ])?;
                client.poll_deltas()?;
                Ok(())
            });
        if let Err(error) = result {
            self.last_error = Some(format!("Terminal scrollbar failed: {error:#}"));
        }
    }

    fn click_scrollbar(&mut self, x: i32, y: i32) -> bool {
        let Some((geometry, current, maximum)) = self.scrollbar_state() else {
            return false;
        };
        if !geometry.track.contains(x, y) {
            return false;
        }
        self.cancel_terminal_selection();
        if maximum == 0 {
            return true;
        }
        if geometry.thumb.contains(x, y) {
            self.scroll_drag = Some(ScrollbarThumbDrag::begin(y, geometry.thumb.top));
            let _ = self.window.set_pointer_capture(true);
        } else {
            let page = self
                .active_tab()
                .map(|tab| usize::try_from(tab.screen.rows).unwrap_or(1))
                .unwrap_or(1)
                .max(1);
            self.set_scrollback(if y < geometry.thumb.top {
                current.saturating_add(page).min(maximum)
            } else {
                current.saturating_sub(page)
            });
        }
        true
    }

    fn drag_scrollbar(&mut self, y: i32) -> bool {
        let Some(drag) = self.scroll_drag else {
            return false;
        };
        let Some((geometry, _, maximum)) = self.scrollbar_state() else {
            self.end_scroll_drag();
            return false;
        };
        let offset = scrollback_for_thumb_top(geometry, drag.thumb_top(y), maximum);
        self.set_scrollback(offset);
        true
    }

    fn end_scroll_drag(&mut self) {
        if self.scroll_drag.take().is_some() {
            let _ = self.window.set_pointer_capture(false);
        }
    }

    fn scrollbar_capture_lost(&mut self) {
        self.scroll_drag = None;
    }

    fn scroll_sidebar(&mut self, wheel_notches: i32) {
        self.invalidate_sidebar_text_click();
        let steps = wheel_notches.unsigned_abs() as usize * WHEEL_ROWS_PER_NOTCH;
        let maximum = self.sidebar_max_offset();
        self.sidebar_scroll_offset = if wheel_notches > 0 {
            self.sidebar_offset().saturating_sub(steps)
        } else {
            self.sidebar_offset().saturating_add(steps).min(maximum)
        };
        self.layout_tab_editor();
    }

    fn click_sidebar_scrollbar(&mut self, x: i32, y: i32) -> bool {
        let Some((geometry, current, maximum)) = self.sidebar_scrollbar_state() else {
            return false;
        };
        if !geometry.track.contains(x, y) {
            return false;
        }
        self.invalidate_sidebar_text_click();
        if maximum == 0 {
            return true;
        }
        if geometry.thumb.contains(x, y) {
            self.sidebar_scroll_drag = Some(ScrollbarThumbDrag::begin(y, geometry.thumb.top));
            let _ = self.window.set_pointer_capture(true);
        } else {
            let page = self.sidebar_row_capacity().max(1);
            self.sidebar_scroll_offset = if y < geometry.thumb.top {
                current.saturating_sub(page)
            } else {
                current.saturating_add(page).min(maximum)
            };
            self.layout_tab_editor();
        }
        true
    }

    fn drag_sidebar_scrollbar(&mut self, y: i32) -> bool {
        let Some(drag) = self.sidebar_scroll_drag else {
            return false;
        };
        let Some((geometry, _, maximum)) = self.sidebar_scrollbar_state() else {
            self.end_sidebar_scroll_drag();
            return false;
        };
        self.invalidate_sidebar_text_click();
        self.sidebar_scroll_offset =
            sidebar_scroll_offset_for_thumb_top(geometry, drag.thumb_top(y), maximum);
        self.layout_tab_editor();
        true
    }

    fn end_sidebar_scroll_drag(&mut self) {
        if self.sidebar_scroll_drag.take().is_some() {
            let _ = self.window.set_pointer_capture(false);
        }
    }

    fn sidebar_scrollbar_capture_lost(&mut self) {
        self.sidebar_scroll_drag = None;
    }

    fn resize_grip_contains(&self, x: i32, y: i32) -> bool {
        self.workspace_geometry()
            .resize_grip
            .is_some_and(|grip| grip.contains(x, y))
    }

    fn begin_tabs_resize(&mut self, x: i32, y: i32) -> bool {
        if !self.resize_grip_contains(x, y) {
            return false;
        }
        self.invalidate_sidebar_text_click();
        self.cancel_terminal_selection();
        self.tabs_resize_dragging = true;
        let _ = self.window.set_pointer_capture(true);
        true
    }

    fn drag_tabs_resize(&mut self, x: i32) {
        if !self.tabs_resize_dragging {
            return;
        }
        let width = self.workspace_geometry().client.width();
        self.config.tabs_width = clamp_tabs_width(tabs_width_from_drag(x, width));
        self.layout();
        self.resize_active_terminal();
    }

    fn finish_tabs_resize(&mut self) {
        if !self.tabs_resize_dragging {
            return;
        }
        self.tabs_resize_dragging = false;
        let _ = self.window.set_pointer_capture(false);
        if let Err(error) = save_config(&self.config) {
            self.last_error = Some(format!("Tabs width save failed: {error:#}"));
        }
    }

    fn reset_tabs_width(&mut self) {
        self.tabs_resize_dragging = false;
        let _ = self.window.set_pointer_capture(false);
        self.config.tabs_width = clamp_tabs_width(reset_tabs_width());
        self.layout();
        self.resize_active_terminal();
        if let Err(error) = save_config(&self.config) {
            self.last_error = Some(format!("Tabs width reset failed: {error:#}"));
        }
    }

    fn tabs_resize_capture_lost(&mut self) {
        if !self.tabs_resize_dragging {
            return;
        }
        self.tabs_resize_dragging = false;
        if let Err(error) = save_config(&self.config) {
            self.last_error = Some(format!("Tabs width save failed: {error:#}"));
        }
    }

    fn handle_left_double_click(&mut self, x: i32, y: i32, clicks: u8) -> bool {
        if self.window_close_dialog.is_open()
            || self.settings_dialog.is_open()
            || self.new_terminal_dialog.is_open()
            || self.close_confirmation.is_open()
        {
            return false;
        }
        let text_tab = self.take_matching_sidebar_text_click(x, y);
        if self.resize_grip_contains(x, y) {
            self.reset_tabs_width();
            true
        } else if let Some(tab_id) = text_tab {
            self.begin_tab_edit_id(&tab_id);
            true
        } else if self.forward_terminal_mouse(x, y, Some(0), true, false) {
            self.cancel_terminal_selection();
            self.mouse_report_button = Some(0);
            self.set_focus_surface_unchecked(RemoteFocusSurface::Terminal);
            self.window.focus();
            true
        } else {
            self.handle_terminal_double_click(x, y, clicks)
        }
    }

    fn handle_left_button_down(&mut self, x: i32, y: i32) -> bool {
        self.recent_sidebar_text_click = None;
        if self.window_close_dialog.is_open()
            || self.settings_dialog.is_open()
            || self.new_terminal_dialog.is_open()
            || self.close_confirmation.is_open()
        {
            return false;
        }
        let clicked_text_tab = self.tab_text_id_at(x, y);
        if self.tabs_recovery_rect().is_some_and(|rect| {
            x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
        }) {
            self.set_tabs_visible(true);
            return true;
        }
        let cwd_status = self.cwd_status_rect();
        let in_cwd_status = x >= cwd_status.left
            && x < cwd_status.right
            && y >= cwd_status.top
            && y < cwd_status.bottom;
        if self.cwd_editor_dialog.is_open() {
            self.finish_cwd_editor(false, ComposerWriteMode::EmptyOnly);
            if in_cwd_status {
                return true;
            }
        } else if in_cwd_status {
            self.open_cwd_editor();
            return true;
        }
        if self.begin_tabs_resize(x, y)
            || self.click_sidebar_scrollbar(x, y)
            || self.click_scrollbar(x, y)
        {
            return true;
        }
        if self.forward_terminal_mouse(x, y, Some(0), true, false) {
            self.cancel_terminal_selection();
            self.mouse_report_button = Some(0);
            self.set_focus_surface_unchecked(RemoteFocusSurface::Terminal);
            self.window.focus();
            return true;
        }
        let sidebar = self.layout_rects().0;
        if self.tabs_visible && x < sidebar.right {
            if !self.toggle_tree_at(x, y) {
                match self.tab_action_at(x, y) {
                    Some(RemoteTabAction::AddChild) => self.new_child_at(y),
                    Some(RemoteTabAction::Close) => self.request_close_tab_at(y),
                    None => {
                        self.finish_tab_edit(false);
                        self.select_tab_at(y);
                        if let Some(tab_id) = clicked_text_tab
                            && self.tab_text_id_at(x, y).as_deref() == Some(tab_id.as_str())
                        {
                            self.record_sidebar_text_click(tab_id);
                        }
                    }
                }
            }
        } else {
            self.finish_tab_edit(false);
            if self.begin_terminal_selection(x, y) {
                return true;
            }
            self.window.focus();
        }
        true
    }

    fn handle_terminal_double_click(&mut self, x: i32, y: i32, clicks: u8) -> bool {
        let Some(point) = self.terminal_point(x, y, false) else {
            return false;
        };
        let Some((tab_id, rows, columns)) = self
            .active_tab()
            .map(|tab| (tab.id.clone(), tab.screen.rows, tab.screen.columns))
        else {
            return false;
        };
        let now = Instant::now();

        if clicks >= 3
            && self.terminal_double_click.as_ref().is_some_and(|click| {
                click.tab_id == tab_id && click.point == point && now <= click.expires_at
            })
        {
            self.terminal_double_click = None;
            self.recent_terminal_click = None;
            if let Some((start, end)) = remote_visible_row_selection(rows, columns, point.row) {
                self.set_completed_terminal_selection(tab_id, rows, columns, start, end);
                let _ = self.copy_terminal_selection();
            }
            self.set_focus_surface_unchecked(RemoteFocusSurface::Terminal);
            self.window.focus();
            return true;
        }

        if self.recent_terminal_click.as_ref().is_some_and(|click| {
            click.tab_id == tab_id
                && click.point == point
                && now.duration_since(click.at) <= Duration::from_millis(DOUBLE_CLICK_MS)
        }) {
            self.recent_terminal_click = None;
            let cells = self.active_tab().map(|tab| screen_cells(&tab.screen));
            let selected = cells
                .as_deref()
                .and_then(|cells| remote_word_selection(cells, point));
            if let Some((start, end)) = selected {
                self.set_completed_terminal_selection(tab_id.clone(), rows, columns, start, end);
                self.terminal_double_click = now
                    .checked_add(Duration::from_millis(DOUBLE_CLICK_MS))
                    .map(|expires_at| RemoteTerminalDoubleClick {
                        tab_id: tab_id.clone(),
                        point,
                        expires_at,
                    });
                let _ = self.copy_terminal_selection();
            }
            self.set_focus_surface_unchecked(RemoteFocusSurface::Terminal);
            self.window.focus();
            return true;
        }

        false
    }

    fn set_completed_terminal_selection(
        &mut self,
        tab_id: String,
        rows: u32,
        columns: u32,
        start: RemotePoint,
        end: RemotePoint,
    ) -> bool {
        self.cancel_terminal_selection();
        if rows == 0 || columns == 0 {
            return false;
        }
        self.terminal_selection = Some(RemoteTerminalSelection {
            tab_id: tab_id.clone(),
            rows,
            columns,
            gesture: RemoteSelectionGesture::completed_unchecked(tab_id, start, end),
            cached_text: None,
        });
        self.terminal_selection_pointer = None;
        self.terminal_selection_autoscroll = None;
        self.cache_terminal_selection_text();
        true
    }

    fn cache_terminal_selection_text(&mut self) {
        let cached_text = {
            let Some(selection) = self.terminal_selection.as_ref() else {
                return;
            };
            let Some(tab) = self.active_tab() else {
                return;
            };
            if !selection.matches_screen(&tab.screen) {
                return;
            }
            screen_selection_text(&tab.screen, selection)
        };
        let cached_text = (!cached_text.is_empty()).then_some(cached_text);
        if let Some(selection) = self.terminal_selection.as_mut() {
            selection.cached_text = cached_text;
        }
    }

    fn handle_pointer_moved(&mut self, x: i32, y: i32) -> bool {
        if self.sidebar_scroll_drag.is_some() {
            self.drag_sidebar_scrollbar(y)
        } else if self.scroll_drag.is_some() {
            self.drag_scrollbar(y)
        } else if self.tabs_resize_dragging {
            self.drag_tabs_resize(x);
            true
        } else if self.forward_terminal_mouse(x, y, None, true, true) {
            true
        } else {
            self.drag_terminal_selection(x, y)
        }
    }

    fn handle_left_button_up(&mut self, x: i32, y: i32) -> bool {
        if let Some(code) = self.mouse_report_button.take() {
            let _ = self.forward_terminal_mouse(x, y, Some(code), false, false);
            self.mouse_report_cell = None;
            false
        } else if self.sidebar_scroll_drag.is_some() {
            self.end_sidebar_scroll_drag();
            false
        } else if self.scroll_drag.is_some() {
            self.end_scroll_drag();
            false
        } else if self.tabs_resize_dragging {
            self.finish_tabs_resize();
            false
        } else {
            self.finish_terminal_selection(x, y)
        }
    }

    fn handle_wheel(&mut self, x: i32, y: i32, delta: i32) {
        if self.window_close_dialog.is_open()
            || self.settings_dialog.is_open()
            || self.new_terminal_dialog.is_open()
            || self.close_confirmation.is_open()
        {
            return;
        }
        let target = route_wheel(
            self.workspace_geometry().sidebar_tree.contains(x, y),
            self.workspace_geometry().terminal.contains(x, y),
        );
        if target == WheelTarget::Ignored {
            return;
        }
        let notches = self.wheel_accumulator.push(delta);
        if notches == 0 {
            return;
        }
        if target == WheelTarget::Sidebar {
            self.scroll_sidebar(notches);
            return;
        }
        if target != WheelTarget::Terminal {
            return;
        }
        let wheel_button = if notches > 0 { 64 } else { 65 };
        let mut reported = false;
        for _ in 0..notches.unsigned_abs().min(40) {
            if self.forward_terminal_mouse(x, y, Some(wheel_button), true, false) {
                reported = true;
            } else {
                break;
            }
        }
        if !reported {
            self.scroll_terminal(notches);
        }
    }

    fn handle_window_keydown(&mut self, key: u32, modifiers: input::ModifierState) -> bool {
        if self.window_close_dialog.is_open() {
            match key {
                0x0d => self.finish_window_close(WindowCloseChoice::KeepServerRunning),
                0x1b => self.finish_window_close(WindowCloseChoice::Cancel),
                _ => {}
            }
            return true;
        }
        if self.close_confirmation.is_open() {
            match key {
                0x0d => self.finish_close_tab(true),
                0x1b => self.finish_close_tab(false),
                _ => {}
            }
            return true;
        }
        if self.settings_dialog.is_open() {
            if key == 0x1b {
                self.finish_settings(false);
            }
            return true;
        }
        if self.new_terminal_dialog.is_open() {
            if key == 0x1b {
                self.finish_new_terminal(false);
            } else if key == 0x0d {
                self.finish_new_terminal(true);
            }
            return true;
        }
        if key == 0x71 && self.current_focus_surface() == RemoteFocusSurface::Tabs {
            if let Some(tab) = self.active_tab().cloned() {
                self.begin_tab_edit(tab);
            }
            return true;
        }
        self.window.focused_target() == FocusTarget::Window && self.terminal_key(key, modifiers)
    }

    fn terminal_char(&mut self, value: u16) {
        // Named terminal keys are fully handled by the WM_KEYDOWN path so it
        // can preserve modifiers. TranslateMessage may still emit their
        // character values independently; drop those echoes to avoid sending
        // Tab/BackTab and Escape twice.
        if terminal_char_is_named_key_echo(value) {
            return;
        }
        if let KeyClassification::TextCommit(text) = self.terminal_text_decoder.push(value) {
            match text.as_str() {
                "\u{8}" => self.terminal_input(b"\x7f"),
                "\r" => self.terminal_input(b"\r"),
                _ => self.terminal_input(text.as_bytes()),
            }
        }
    }

    fn terminal_key(&mut self, key: u32, modifiers: input::ModifierState) -> bool {
        let key = u16::try_from(key).unwrap_or_default();
        let primary = input::is_primary_shortcut(input::modifiers(
            modifiers.control,
            modifiers.meta,
            modifiers.alt,
            modifiers.shift,
        ));
        if primary
            && key == u16::from(b'C')
            && selection_claims_copy_shortcut(self.terminal_selection.as_ref())
        {
            if let Err(error) = self.copy_terminal_selection() {
                self.last_error = Some(format!("Copy failed: {error:#}"));
            }
            return true;
        }
        if primary && key == u16::from(b'V') {
            if let Err(error) = self.paste_terminal_clipboard() {
                self.last_error = Some(format!("Paste failed: {error:#}"));
            }
            return true;
        }
        if primary && (u16::from(b'A')..=u16::from(b'Z')).contains(&key) {
            self.terminal_input(&[(key as u8) - b'A' + 1]);
            return true;
        }
        let Some(name) = windows_terminal_named_key(key) else {
            return false;
        };
        if let Some(bytes) = tmux_key_bytes_with_modifiers(name, modifiers) {
            self.terminal_input(&bytes);
            true
        } else {
            false
        }
    }

    fn dispatch_windows_toolbar_action(&mut self, action_id: &str) {
        if !action::is_toolbar_action_id(action_id) {
            return;
        }
        match action_id {
            action::TOGGLE_TABS => self.toggle_tabs(),
            action::NEW_TAB => self.open_new_terminal(),
            action::OPEN_CONTROL_CENTER => {
                if let Err(error) = crate::control_center::open_control_center(
                    self.no_activate,
                    &crate::client::ipc_address(),
                ) {
                    self.last_error = Some(format!("Control Center unavailable: {error:#}"));
                } else {
                    self.last_message = Some("Control Center opened".to_owned());
                }
            }
            action::OPEN_SETTINGS => {
                self.finish_cwd_editor(false, ComposerWriteMode::EmptyOnly);
                self.open_settings();
            }
            action::TOGGLE_LOCALE => self.toggle_locale(),
            action::FONT_DECREASE => self.adjust_active_terminal_font(-1),
            action::FONT_INCREASE => self.adjust_active_terminal_font(1),
            _ => {}
        }
        if toolbar_action_returns_terminal_focus(action_id) {
            self.set_focus_surface(RemoteFocusSurface::Terminal);
        }
    }

    fn paint_canvas(&self, canvas: &mut dyn ControlCanvas) -> Result<(), ControlWindowError> {
        canvas.set_font(&self.font)?;
        self.paint_frame(canvas);
        Ok(())
    }

    fn paint_frame(&self, device: &mut dyn ControlCanvas) {
        let palette = if self.settings_dialog.is_open() {
            self.settings_dialog.theme_draft().palette()
        } else {
            self.active_terminal_appearance().color_theme.palette()
        };
        let (sidebar, terminal, composer, status) = self.layout_rects();
        fill(device, &sidebar, palette.sidebar.canvas_rgb());
        if let Some(toolbar) = self.workspace_geometry().workspace_toolbar {
            let toolbar = win_rect(toolbar.bounds);
            fill(device, &toolbar, palette.composer.canvas_rgb());
        }
        fill(device, &terminal, palette.terminal_background.canvas_rgb());
        fill(device, &composer, palette.composer.canvas_rgb());
        fill(device, &status, palette.status.canvas_rgb());
        self.paint_tabs(device, sidebar, palette);
        if let Some(tab) = self.active_tab() {
            paint_screen(
                device,
                terminal,
                &tab.screen,
                self.cell_width,
                self.cell_height,
                palette,
            );
            self.paint_terminal_selection(device, terminal, &tab.screen, palette);
            self.paint_terminal_scrollbar(device, palette);
            draw_text(
                device,
                ProductPixelRect {
                    left: composer.left + MARGIN,
                    top: composer.top + 2,
                    right: composer.right - MARGIN,
                    bottom: composer.top + 24,
                },
                &if self.cwd_editor_dialog.target() == Some(tab.id.as_str()) {
                    format!("CWD → {}  Ctrl+Enter prepares · Esc cancels", tab.id)
                } else {
                    format!(
                        "{} → {}  {}",
                        self.config.locale.text(UiText::Input),
                        tab.id,
                        tab.title
                    )
                },
                palette.muted_text.canvas_rgb(),
            );
        }
        let status_text = if let Some(error) = &self.last_error {
            error.clone()
        } else if let Some(client) = &self.client {
            format!(
                "Connected · server PID {} · {}",
                client.server_pid(),
                client.client_id()
            )
        } else {
            "Disconnected · reconnecting".to_owned()
        };
        let cwd_status = self.cwd_status_rect();
        let tabs_recovery = self.tabs_recovery_rect();
        if let Some(recovery) = tabs_recovery {
            frame(device, &recovery, palette.active_border.canvas_rgb());
            draw_text(
                device,
                ProductPixelRect {
                    left: recovery.left + MARGIN,
                    top: recovery.top,
                    right: recovery.right - MARGIN,
                    bottom: recovery.bottom,
                },
                "Tabs",
                palette.muted_text.canvas_rgb(),
            );
        }
        draw_text(
            device,
            ProductPixelRect {
                left: tabs_recovery.map_or(status.left + MARGIN, |rect| rect.right + MARGIN),
                top: status.top,
                right: cwd_status.left - MARGIN,
                bottom: status.bottom,
            },
            &status_text,
            if self.last_error.is_some() {
                palette.danger.canvas_rgb()
            } else {
                palette.muted_text.canvas_rgb()
            },
        );
        frame(device, &cwd_status, palette.active_border.canvas_rgb());
        let cwd = self
            .active_tab()
            .and_then(|tab| tab.working_context.cwd.as_deref())
            .unwrap_or("-");
        draw_text(
            device,
            ProductPixelRect {
                left: cwd_status.left + MARGIN,
                top: cwd_status.top,
                right: cwd_status.right - MARGIN,
                bottom: cwd_status.bottom,
            },
            &format!("CWD: {cwd}"),
            palette.muted_text.canvas_rgb(),
        );
        if !self.window_close_dialog.is_open()
            && !self.settings_dialog.is_open()
            && !self.new_terminal_dialog.is_open()
            && !self.close_confirmation.is_open()
        {
            let focus = match self.current_focus_surface() {
                RemoteFocusSurface::Terminal => terminal,
                RemoteFocusSurface::Composer => composer,
                RemoteFocusSurface::Tabs => sidebar,
            };
            frame(device, &focus, palette.focus_ring.canvas_rgb());
        }
        if self.window_close_dialog.is_open() {
            self.paint_window_close(device, palette);
        } else if self.settings_dialog.is_open() {
            self.paint_settings(device, palette);
        } else if self.new_terminal_dialog.is_open() {
            self.paint_new_terminal(device, palette);
        } else if self.close_confirmation.is_open() {
            self.paint_tab_close(device, palette);
        }
    }

    fn paint_terminal_selection(
        &self,
        device: &mut dyn ControlCanvas,
        terminal: ProductPixelRect,
        screen: &UiScreenSnapshot,
        palette: &ThemePalette,
    ) {
        let Some(selection) = self
            .terminal_selection
            .as_ref()
            .filter(|selection| selection.matches_screen(screen))
        else {
            return;
        };
        let cells = screen_cells(screen);
        let (start, end) = selection.bounds();
        for bounds in terminal_selection_highlight_rects(
            selection,
            screen,
            terminal,
            self.cell_width,
            self.cell_height,
        ) {
            fill(device, &bounds, palette.selection_background.canvas_rgb());
        }
        for row in start.row..=end.row.min(screen.rows.saturating_sub(1)) {
            let first = if row == start.row { start.column } else { 0 };
            let last = if row == end.row {
                end.column
            } else {
                screen.columns.saturating_sub(1)
            }
            .min(screen.columns.saturating_sub(1));
            for column in first..=last {
                let Some(text) = cells
                    .get(usize::try_from(row).unwrap_or(usize::MAX))
                    .and_then(|cells| cells.get(usize::try_from(column).unwrap_or(usize::MAX)))
                    .and_then(Option::as_deref)
                    .filter(|text| !text.is_empty())
                else {
                    continue;
                };
                let span = text
                    .chars()
                    .map(|character| UnicodeWidthChar::width(character).unwrap_or(1))
                    .sum::<usize>()
                    .max(1);
                draw_text(
                    device,
                    ProductPixelRect {
                        left: terminal.left
                            + i32::try_from(column).unwrap_or_default() * self.cell_width,
                        top: terminal.top
                            + i32::try_from(row).unwrap_or_default() * self.cell_height,
                        right: terminal.left
                            + (i32::try_from(column).unwrap_or_default()
                                + i32::try_from(span).unwrap_or(1))
                                * self.cell_width,
                        bottom: terminal.top
                            + i32::try_from(row.saturating_add(1)).unwrap_or_default()
                                * self.cell_height,
                    },
                    text,
                    palette.selection_foreground.canvas_rgb(),
                );
            }
        }
    }

    fn paint_terminal_scrollbar(&self, device: &mut dyn ControlCanvas, palette: &ThemePalette) {
        let Some((geometry, _, _)) = self.scrollbar_state() else {
            return;
        };
        let track = win_rect(geometry.track);
        let thumb = win_rect(geometry.thumb);
        fill(device, &track, palette.scrollbar_track.canvas_rgb());
        fill(
            device,
            &thumb,
            if self.scroll_drag.is_some() {
                palette.scrollbar_thumb_active.canvas_rgb()
            } else {
                palette.scrollbar_thumb.canvas_rgb()
            },
        );
    }

    fn paint_window_close(&self, device: &mut dyn ControlCanvas, palette: &ThemePalette) {
        let (modal, _) = self.close_modal_geometry();
        fill(device, &modal, palette.modal.canvas_rgb());
        frame(device, &modal, palette.accent.canvas_rgb());
        draw_text(
            device,
            ProductPixelRect {
                left: modal.left + 24,
                top: modal.top + 18,
                right: modal.right - 24,
                bottom: modal.top + 50,
            },
            "Close AgenTerm window?",
            palette.text.canvas_rgb(),
        );
        draw_text(
            device,
            ProductPixelRect {
                left: modal.left + 24,
                top: modal.top + 56,
                right: modal.right - 24,
                bottom: modal.top + 86,
            },
            "Keep the server running to preserve live tabs and processes.",
            palette.muted_text.canvas_rgb(),
        );
        draw_text(
            device,
            ProductPixelRect {
                left: modal.left + 24,
                top: modal.top + 90,
                right: modal.right - 24,
                bottom: modal.top + 120,
            },
            "Press Enter to keep it running, or Esc to cancel.",
            palette.muted_text.canvas_rgb(),
        );
    }

    fn paint_settings(&self, device: &mut dyn ControlCanvas, palette: &ThemePalette) {
        let (modal, _) = self.settings_modal_geometry();
        let locale = self.config.locale;
        fill(device, &modal, palette.modal.canvas_rgb());
        frame(device, &modal, palette.accent.canvas_rgb());
        draw_text(
            device,
            ProductPixelRect {
                left: modal.left + 28,
                top: modal.top + 18,
                right: modal.right - 28,
                bottom: modal.top + 50,
            },
            locale.text(UiText::Settings),
            palette.text.canvas_rgb(),
        );
        draw_text(
            device,
            ProductPixelRect {
                left: modal.left + 32,
                top: modal.top + 96,
                right: modal.right - 32,
                bottom: modal.top + 126,
            },
            locale.text(UiText::FontFamily),
            palette.muted_text.canvas_rgb(),
        );
        draw_text(
            device,
            ProductPixelRect {
                left: modal.left + 32,
                top: modal.top + 168,
                right: modal.right - 32,
                bottom: modal.top + 198,
            },
            locale.text(UiText::Size),
            palette.muted_text.canvas_rgb(),
        );
        draw_text(
            device,
            ProductPixelRect {
                left: modal.left + 32,
                top: modal.top + 240,
                right: modal.right - 32,
                bottom: modal.top + 270,
            },
            locale.text(UiText::ColorTheme),
            palette.muted_text.canvas_rgb(),
        );
        if self.settings_dialog.scope() == SettingsScope::CurrentTerminal {
            let target = self.settings_dialog.target_tab_id().unwrap_or("-");
            draw_text(
                device,
                ProductPixelRect {
                    left: modal.left + 454,
                    top: modal.top + 18,
                    right: modal.right - 28,
                    bottom: modal.top + 50,
                },
                target,
                palette.muted_text.canvas_rgb(),
            );
        }
    }

    fn paint_new_terminal(&self, device: &mut dyn ControlCanvas, palette: &ThemePalette) {
        let (modal, _) = self.new_terminal_modal_geometry();
        fill(device, &modal, palette.modal.canvas_rgb());
        frame(device, &modal, palette.accent.canvas_rgb());
        for (top, text, color) in [
            (
                18,
                self.config.locale.text(UiText::NewTerminal),
                palette.text.canvas_rgb(),
            ),
            (
                48,
                self.config.locale.text(UiText::ShellProfile),
                palette.muted_text.canvas_rgb(),
            ),
            (
                116,
                "Initial command · optional; leaves the selected shell open",
                palette.muted_text.canvas_rgb(),
            ),
            (
                192,
                "HTTP proxy · optional, applied only to this terminal",
                palette.muted_text.canvas_rgb(),
            ),
            (
                268,
                "HTTPS proxy · optional, values are never exposed in snapshots",
                palette.muted_text.canvas_rgb(),
            ),
        ] {
            draw_text(
                device,
                ProductPixelRect {
                    left: modal.left + 28,
                    top: modal.top + top,
                    right: modal.right - 28,
                    bottom: modal.top + top + 28,
                },
                text,
                color,
            );
        }
    }

    fn paint_tab_close(&self, device: &mut dyn ControlCanvas, palette: &ThemePalette) {
        let (modal, _) = self.tab_close_modal_geometry();
        fill(device, &modal, palette.modal.canvas_rgb());
        frame(device, &modal, palette.warning.canvas_rgb());
        draw_text(
            device,
            ProductPixelRect {
                left: modal.left + 24,
                top: modal.top + 18,
                right: modal.right - 24,
                bottom: modal.top + 50,
            },
            "Close live tab?",
            palette.text.canvas_rgb(),
        );
        let target = self.close_confirmation.target().unwrap_or("-");
        draw_text(
            device,
            ProductPixelRect {
                left: modal.left + 24,
                top: modal.top + 60,
                right: modal.right - 24,
                bottom: modal.top + 90,
            },
            &format!("{target} is still running."),
            palette.muted_text.canvas_rgb(),
        );
        draw_text(
            device,
            ProductPixelRect {
                left: modal.left + 24,
                top: modal.top + 88,
                right: modal.right - 24,
                bottom: modal.top + 116,
            },
            "Closing it will terminate the PTY process.",
            palette.muted_text.canvas_rgb(),
        );
        draw_text(
            device,
            ProductPixelRect {
                left: modal.left + 24,
                top: modal.top + 116,
                right: modal.right - 24,
                bottom: modal.top + 144,
            },
            "Cancel returns without changing the tab tree.",
            palette.muted_text.canvas_rgb(),
        );
    }

    fn paint_tabs(
        &self,
        device: &mut dyn ControlCanvas,
        sidebar: ProductPixelRect,
        palette: &ThemePalette,
    ) {
        if !self.tabs_visible {
            return;
        }
        let Some(client) = &self.client else {
            return;
        };
        for (viewport_position, tree_row) in remote_tree_rows(&client.snapshot().tabs)
            .into_iter()
            .skip(self.sidebar_offset())
            .take(self.sidebar_row_capacity())
            .enumerate()
        {
            let tab = &client.snapshot().tabs[tree_row.tab_index];
            let geometry =
                self.sidebar_row_geometry(viewport_position, tree_row.depth, TreeRowMode::Normal);
            let row = win_rect(geometry.selection);
            if row.bottom > sidebar.bottom {
                break;
            }
            let active = client.snapshot().active_tab_id.as_deref() == Some(tab.id.as_str());
            if active {
                fill(device, &row, palette.active.canvas_rgb());
                frame(device, &row, palette.active_border.canvas_rgb());
            }
            for segment in tree_connector_segments(
                self.workspace_geometry().sidebar_tree,
                &geometry,
                tree_row.depth,
                &tree_row.guides,
                tree_row.is_last,
                TreeRowMode::Normal,
            ) {
                fill(device, &win_rect(segment), palette.divider.canvas_rgb());
            }
            if tree_row.has_children {
                let expander = win_rect(geometry.expander);
                frame(device, &expander, palette.active_border.canvas_rgb());
                draw_text(
                    device,
                    expander,
                    if tree_row.collapsed { "+" } else { "-" },
                    palette.muted_text.canvas_rgb(),
                );
            }
            if self.tab_editor_dialog.target() == Some(tab.id.as_str()) {
                continue;
            }
            if active {
                let compact = geometry.actions.density == TreeRowActionDensity::Compact;
                let locale = self.config.locale;
                for (bounds, label) in [
                    (
                        geometry
                            .actions
                            .add_child
                            .expect("normal tab rows expose Add"),
                        "+",
                    ),
                    (
                        geometry.actions.secondary,
                        if compact {
                            "X"
                        } else {
                            locale.text(UiText::Close)
                        },
                    ),
                ] {
                    let bounds = win_rect(bounds);
                    frame(device, &bounds, palette.active_border.canvas_rgb());
                    draw_text(device, bounds, label, palette.muted_text.canvas_rgb());
                }
            }
            draw_text(
                device,
                win_rect(geometry.name),
                &format!(
                    "{} {}{}",
                    tab.id,
                    tab.title,
                    if tab.dead { " [dead]" } else { "" }
                ),
                if tab.dead {
                    palette.muted_text.canvas_rgb()
                } else {
                    palette.text.canvas_rgb()
                },
            );
            if !tab.note.is_empty() {
                draw_text(
                    device,
                    win_rect(geometry.note),
                    &tab.note,
                    palette.muted_text.canvas_rgb(),
                );
            }
        }
        if let Some((geometry, _, _)) = self.sidebar_scrollbar_state() {
            fill(
                device,
                &win_rect(geometry.track),
                palette.scrollbar_track.canvas_rgb(),
            );
            fill(
                device,
                &win_rect(geometry.thumb),
                if self.sidebar_scroll_drag.is_some() {
                    palette.scrollbar_thumb_active.canvas_rgb()
                } else {
                    palette.scrollbar_thumb.canvas_rgb()
                },
            );
        }
    }
}

fn terminal_char_is_named_key_echo(value: u16) -> bool {
    matches!(value, 0x09 | 0x1b)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RemoteTerminalPasteRequest {
    server_epoch: String,
    tab_id: String,
    bracketed: bool,
}

struct RemoteTerminalPasteTask {
    requested: RemoteTerminalPasteRequest,
    read: Box<dyn FnOnce() -> std::result::Result<String, String> + Send>,
}

struct RemoteTerminalPasteResult {
    requested: RemoteTerminalPasteRequest,
    result: std::result::Result<String, String>,
}

struct RemoteTerminalPasteWorker {
    tasks: Sender<RemoteTerminalPasteTask>,
    results: Receiver<RemoteTerminalPasteResult>,
    _thread: JoinHandle<()>,
}

impl RemoteTerminalPasteWorker {
    fn spawn() -> Result<Self> {
        let (task_sender, tasks) = mpsc::channel::<RemoteTerminalPasteTask>();
        let (result_sender, results) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("agenterm-terminal-paste".to_owned())
            .spawn(move || {
                for task in tasks {
                    let requested = task.requested;
                    let result = (task.read)();
                    if result_sender
                        .send(RemoteTerminalPasteResult { requested, result })
                        .is_err()
                    {
                        return;
                    }
                }
            })
            .context("could not start terminal paste worker")?;
        Ok(Self {
            tasks: task_sender,
            results,
            _thread: thread,
        })
    }

    fn queue(&self, task: RemoteTerminalPasteTask) -> std::result::Result<(), String> {
        self.tasks
            .send(task)
            .map_err(|_| "terminal paste worker is unavailable".to_owned())
    }

    fn try_result(&self) -> std::result::Result<RemoteTerminalPasteResult, mpsc::TryRecvError> {
        self.results.try_recv()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RemoteTerminalResize {
    server_epoch: String,
    tab_id: String,
    rows: u16,
    columns: u16,
}

struct RemoteTerminalResizeTask {
    requested: RemoteTerminalResize,
    execute: Box<dyn FnOnce() -> Result<()> + Send>,
}

struct RemoteTerminalResizeWorkerState {
    pending: Option<RemoteTerminalResizeTask>,
    shutdown: bool,
}

struct RemoteTerminalResizeWorkerShared {
    state: Mutex<RemoteTerminalResizeWorkerState>,
    wake: Condvar,
}

struct RemoteTerminalResizeResult {
    requested: RemoteTerminalResize,
    result: std::result::Result<(), String>,
}

struct RemoteTerminalResizeWorker {
    shared: Arc<RemoteTerminalResizeWorkerShared>,
    results: Receiver<RemoteTerminalResizeResult>,
    thread: Option<JoinHandle<()>>,
}

impl RemoteTerminalResizeWorker {
    fn spawn() -> Result<Self> {
        let shared = Arc::new(RemoteTerminalResizeWorkerShared {
            state: Mutex::new(RemoteTerminalResizeWorkerState {
                pending: None,
                shutdown: false,
            }),
            wake: Condvar::new(),
        });
        let worker_shared = Arc::clone(&shared);
        let (result_sender, results) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("agenterm-terminal-resize".to_owned())
            .spawn(move || {
                loop {
                    let task = {
                        let mut state = worker_shared
                            .state
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        while state.pending.is_none() && !state.shutdown {
                            state = worker_shared
                                .wake
                                .wait(state)
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                        }
                        if state.shutdown {
                            return;
                        }
                        state.pending.take().expect("pending resize task")
                    };
                    let requested = task.requested;
                    let result = (task.execute)().map_err(|error| format!("{error:#}"));
                    if result_sender
                        .send(RemoteTerminalResizeResult { requested, result })
                        .is_err()
                    {
                        return;
                    }
                }
            })
            .context("could not start terminal resize worker")?;
        Ok(Self {
            shared,
            results,
            thread: Some(thread),
        })
    }

    fn queue(&self, task: RemoteTerminalResizeTask) {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.pending = Some(task);
        self.shared.wake.notify_one();
    }

    fn try_result(&self) -> std::result::Result<RemoteTerminalResizeResult, mpsc::TryRecvError> {
        self.results.try_recv()
    }
}

impl Drop for RemoteTerminalResizeWorker {
    fn drop(&mut self) {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.pending = None;
        state.shutdown = true;
        self.shared.wake.notify_one();
        drop(state);
        // A request already inside bounded IPC may take up to its transport
        // deadline. Dropping the handle detaches only that bounded tail while
        // the shutdown flag prevents any queued resize from starting.
        let _ = self.thread.take();
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RemoteTerminalResizeDecision {
    Current,
    InFlight,
    Send,
}

fn terminal_resize_decision(
    current_rows: u32,
    current_columns: u32,
    last_requested: Option<&RemoteTerminalResize>,
    requested: &RemoteTerminalResize,
) -> RemoteTerminalResizeDecision {
    if current_rows == u32::from(requested.rows) && current_columns == u32::from(requested.columns)
    {
        return RemoteTerminalResizeDecision::Current;
    }
    if last_requested == Some(requested) {
        RemoteTerminalResizeDecision::InFlight
    } else {
        RemoteTerminalResizeDecision::Send
    }
}

fn bounded_frame_dimensions(rect: ProductPixelRect) -> Option<(i32, i32)> {
    let width = rect.right.checked_sub(rect.left)?;
    let height = rect.bottom.checked_sub(rect.top)?;
    let pixels = i64::from(width).checked_mul(i64::from(height))?;
    (width > 0 && height > 0 && pixels <= MAX_FRAME_PIXELS).then_some((width, height))
}

fn windows_terminal_named_key(key: u16) -> Option<&'static str> {
    match key {
        KEY_TAB => Some("Tab"),
        KEY_UP => Some("Up"),
        KEY_DOWN => Some("Down"),
        KEY_LEFT => Some("Left"),
        KEY_RIGHT => Some("Right"),
        KEY_HOME => Some("Home"),
        KEY_END => Some("End"),
        KEY_INSERT => Some("Insert"),
        KEY_DELETE => Some("Delete"),
        KEY_PAGE_UP => Some("PageUp"),
        KEY_PAGE_DOWN => Some("PageDown"),
        KEY_ESCAPE => Some("Escape"),
        KEY_F1 => Some("F1"),
        KEY_F2 => Some("F2"),
        KEY_F3 => Some("F3"),
        KEY_F4 => Some("F4"),
        KEY_F5 => Some("F5"),
        KEY_F6 => Some("F6"),
        KEY_F7 => Some("F7"),
        KEY_F8 => Some("F8"),
        KEY_F9 => Some("F9"),
        KEY_F10 => Some("F10"),
        KEY_F11 => Some("F11"),
        KEY_F12 => Some("F12"),
        _ => None,
    }
}

fn normalized_virtual_key(key: &input::LogicalKey) -> Option<u32> {
    match key {
        input::LogicalKey::Named(named) => Some(match named {
            input::NamedKey::Backspace => 0x08,
            input::NamedKey::Tab => 0x09,
            input::NamedKey::Enter => 0x0d,
            input::NamedKey::Escape => 0x1b,
            input::NamedKey::Space => 0x20,
            input::NamedKey::PageUp => 0x21,
            input::NamedKey::PageDown => 0x22,
            input::NamedKey::End => 0x23,
            input::NamedKey::Home => 0x24,
            input::NamedKey::ArrowLeft => 0x25,
            input::NamedKey::ArrowUp => 0x26,
            input::NamedKey::ArrowRight => 0x27,
            input::NamedKey::ArrowDown => 0x28,
            input::NamedKey::Insert => 0x2d,
            input::NamedKey::Delete => 0x2e,
            input::NamedKey::F1 => 0x70,
            input::NamedKey::F2 => 0x71,
            input::NamedKey::F3 => 0x72,
            input::NamedKey::F4 => 0x73,
            input::NamedKey::F5 => 0x74,
            input::NamedKey::F6 => 0x75,
            input::NamedKey::F7 => 0x76,
            input::NamedKey::F8 => 0x77,
            input::NamedKey::F9 => 0x78,
            input::NamedKey::F10 => 0x79,
            input::NamedKey::F11 => 0x7a,
            input::NamedKey::F12 => 0x7b,
            _ => return None,
        }),
        input::LogicalKey::Character(value) if value.chars().count() == 1 => value
            .chars()
            .next()
            .map(|value| value.to_ascii_uppercase() as u32),
        _ => None,
    }
}

impl Drop for RemoteWindowState {
    fn drop(&mut self) {
        if let Some(client) = self.client.as_mut() {
            let _ = client.detach();
        }
    }
}

struct RemoteWindowApplication {
    state: Option<RemoteWindowState>,
    client_id: String,
    client: Option<UiClientModel>,
    no_activate: bool,
}

impl RemoteWindowApplication {
    fn state_mut(&mut self) -> Result<&mut RemoteWindowState, ControlWindowError> {
        self.state.as_mut().ok_or_else(|| {
            ControlWindowError::failed("control_window_not_open", "window is not initialized")
        })
    }

    fn command(state: &mut RemoteWindowState, control_id: ControlId) {
        if control_id == state.new_initial_command
            || control_id == state.new_http_proxy
            || control_id == state.new_https_proxy
        {
            state.sync_new_terminal_drafts();
        }
        if control_id == state.tab_title_edit || control_id == state.tab_note_edit {
            state.sync_tab_editor_drafts();
        }
        if control_id == state.settings_font || control_id == state.settings_size {
            state.sync_settings_drafts();
        }
        if let Some(hit) = windows_toolbar_hit(control_id) {
            state.dispatch_windows_toolbar_action(hit.action_id());
            return;
        }
        match control_id {
            SEND_ID => state.send_composer(),
            TAB_SAVE_ID => state.finish_tab_edit(true),
            TAB_CANCEL_ID => state.finish_tab_edit(false),
            CLOSE_KEEP_ID => state.finish_window_close(WindowCloseChoice::KeepServerRunning),
            CLOSE_STOP_ID => state.finish_window_close(WindowCloseChoice::StopServerAndExit),
            CLOSE_CANCEL_ID => state.finish_window_close(WindowCloseChoice::Cancel),
            SETTINGS_DARK_ID => state.preview_settings_theme(ThemeId::Dark),
            SETTINGS_LIGHT_ID => state.preview_settings_theme(ThemeId::Light),
            SETTINGS_DEFAULT_SCOPE_ID => state.switch_settings_scope(SettingsScope::Defaults),
            SETTINGS_CURRENT_SCOPE_ID => {
                state.switch_settings_scope(SettingsScope::CurrentTerminal)
            }
            SETTINGS_FONT_INHERIT_ID => {
                state.toggle_settings_inheritance(AppearanceField::FontFamily)
            }
            SETTINGS_SIZE_INHERIT_ID => {
                state.toggle_settings_inheritance(AppearanceField::FontSize)
            }
            SETTINGS_THEME_INHERIT_ID => state.toggle_settings_inheritance(AppearanceField::Theme),
            SETTINGS_RESET_OVERRIDES_ID => state.reset_settings_overrides(),
            SETTINGS_APPLY_ID => state.finish_settings(true),
            SETTINGS_CANCEL_ID => state.finish_settings(false),
            TAB_CLOSE_CONFIRM_ID => state.finish_close_tab(true),
            TAB_CLOSE_CANCEL_ID => state.finish_close_tab(false),
            NEW_DEFAULT_SHELL_ID => state.choose_new_shell(NewShellChoice::Default),
            NEW_CMD_SHELL_ID => state.choose_new_shell(NewShellChoice::Primary),
            NEW_POWERSHELL_ID => state.choose_new_shell(NewShellChoice::Alternate),
            NEW_CREATE_ID => state.finish_new_terminal(true),
            NEW_CANCEL_ID => state.finish_new_terminal(false),
            _ => {}
        }
    }
}

impl ControlWindowApplication for RemoteWindowApplication {
    fn opened(
        &mut self,
        window: &ControlWindow,
    ) -> Result<ControlWindowDirective, ControlWindowError> {
        let client = self.client.take().ok_or_else(|| {
            ControlWindowError::failed("control_window_client_missing", "UI client is unavailable")
        })?;
        let state = RemoteWindowState::new(
            window.clone(),
            RemoteControls::stable(),
            std::mem::take(&mut self.client_id),
            client,
            self.no_activate,
        )
        .map_err(|error| ControlWindowError::failed("control_window_open_failed", error))?;
        self.state = Some(state);
        let state = self.state_mut()?;
        state.apply_locale();
        state.layout();
        state.load_composer();
        state.resize_active_terminal();
        Ok(ControlWindowDirective::Redraw)
    }

    fn event(
        &mut self,
        _window: &ControlWindow,
        event: ControlWindowEvent,
    ) -> Result<ControlWindowDirective, ControlWindowError> {
        let state = self.state_mut()?;
        let mut redraw = false;
        let mut consumed = false;
        match event {
            ControlWindowEvent::Poll { .. } => redraw = state.tick(),
            ControlWindowEvent::Resized { minimized, .. } => {
                state.layout();
                if !minimized {
                    state.resize_active_terminal();
                }
                redraw = true;
            }
            ControlWindowEvent::CloseRequested => {
                state.request_window_close();
                redraw = true;
                consumed = true;
            }
            ControlWindowEvent::FocusChanged(true) => {
                if state.window.focused_target() != FocusTarget::Control(EDIT_ID) {
                    state.window.focus();
                }
            }
            ControlWindowEvent::KeyPreview { event, .. }
                if matches!(event.state, input::KeyPressState::Pressed) =>
            {
                if let Some(key) = normalized_virtual_key(&event.logical) {
                    consumed = state.handle_tab_editor_keydown(key, event.modifiers)
                        || state.handle_cwd_editor_keydown(key, event.modifiers)
                        || state.handle_keyboard_navigation_with_modifiers(key, event.modifiers)
                        || state.handle_window_keydown(key, event.modifiers);
                    redraw = consumed;
                }
            }
            ControlWindowEvent::TextInput(text) => {
                if state.window.focused_target() == FocusTarget::Window {
                    for value in text.encode_utf16() {
                        state.terminal_char(value);
                    }
                    consumed = true;
                }
            }
            ControlWindowEvent::PointerMoved {
                position,
                modifiers,
            } => {
                state.pointer_modifiers = modifiers;
                redraw = state.handle_pointer_moved(position.x, position.y);
            }
            ControlWindowEvent::PointerButton {
                button: PointerButton::Left,
                state: ButtonState::Pressed,
                position,
                clicks,
                modifiers,
            } => {
                state.pointer_modifiers = modifiers;
                redraw = if clicks > 1 {
                    state.handle_left_double_click(position.x, position.y, clicks)
                } else {
                    state.handle_left_button_down(position.x, position.y)
                };
                consumed = true;
            }
            ControlWindowEvent::PointerButton {
                button: PointerButton::Left,
                state: ButtonState::Released,
                position,
                modifiers,
                clicks: _,
            } => {
                state.pointer_modifiers = modifiers;
                redraw = state.handle_left_button_up(position.x, position.y);
                consumed = true;
            }
            ControlWindowEvent::PointerButton {
                button: button @ (PointerButton::Right | PointerButton::Middle),
                state: button_state,
                position,
                modifiers,
                ..
            } => {
                state.pointer_modifiers = modifiers;
                let code = if button == PointerButton::Right { 2 } else { 1 };
                match button_state {
                    ButtonState::Pressed => {
                        if state.forward_terminal_mouse(
                            position.x,
                            position.y,
                            Some(code),
                            true,
                            false,
                        ) {
                            state.mouse_report_button = Some(code);
                            consumed = true;
                            redraw = true;
                        }
                    }
                    ButtonState::Released if state.mouse_report_button == Some(code) => {
                        state.mouse_report_button = None;
                        let _ = state.forward_terminal_mouse(
                            position.x,
                            position.y,
                            Some(code),
                            false,
                            false,
                        );
                        state.mouse_report_cell = None;
                        consumed = true;
                        redraw = true;
                    }
                    ButtonState::Released => {}
                    _ => {}
                }
            }
            ControlWindowEvent::Wheel {
                delta: ControlWheelDelta::Lines(lines),
                position,
                modifiers,
            } => {
                state.pointer_modifiers = modifiers;
                state.handle_wheel(
                    position.x,
                    position.y,
                    wheel_delta_units(f64::from(lines), true),
                );
                redraw = true;
                consumed = true;
            }
            ControlWindowEvent::CaptureChanged(false) => {
                state.mouse_report_button = None;
                state.mouse_report_cell = None;
                state.tabs_resize_capture_lost();
                state.sidebar_scrollbar_capture_lost();
                state.scrollbar_capture_lost();
                state.terminal_selection_capture_lost();
            }
            ControlWindowEvent::Command(control_id) => {
                Self::command(state, control_id);
                redraw = true;
                consumed = true;
            }
            ControlWindowEvent::SystemMenuOpening => state.refresh_system_menu(),
            ControlWindowEvent::SystemMenu(command) => {
                match command {
                    SYSTEM_MENU_TOGGLE_TABS_ID => state.toggle_tabs(),
                    SYSTEM_MENU_COPY_ID => state.system_menu_copy(),
                    SYSTEM_MENU_PASTE_ID => state.system_menu_paste(),
                    _ => {}
                }
                redraw = true;
                consumed = true;
            }
            ControlWindowEvent::AutomationShortcut { key, modifiers } => {
                consumed = state.handle_surface_navigation(
                    key,
                    modifiers.control,
                    modifiers.shift,
                    modifiers.alt,
                );
                if !consumed {
                    consumed = state.handle_window_keydown(key, modifiers);
                }
                redraw = consumed;
            }
            ControlWindowEvent::RenderActivitySample(activity) => {
                state.render_activity_sample = Some(activity);
                state.render_activity_sample_sequence =
                    state.render_activity_sample_sequence.saturating_add(1);
                consumed = true;
            }
            _ => {}
        }
        Ok(match (consumed, redraw) {
            (true, true) => ControlWindowDirective::ConsumedAndRedraw,
            (true, false) => ControlWindowDirective::Consumed,
            (false, true) => ControlWindowDirective::Redraw,
            (false, false) => ControlWindowDirective::Continue,
        })
    }

    fn paint(
        &mut self,
        _window: &ControlWindow,
        canvas: &mut dyn ControlCanvas,
    ) -> Result<ControlWindowDirective, ControlWindowError> {
        self.state_mut()?.paint_canvas(canvas)?;
        Ok(ControlWindowDirective::Continue)
    }

    fn query(&self, _window: &ControlWindow, query: ControlWindowQuery) -> isize {
        match query {
            ControlWindowQuery::AutomationFocusSurface => self
                .state
                .as_ref()
                .map_or(0, RemoteWindowState::native_focus_surface_code),
            _ => 0,
        }
    }
}

fn screen_cells(screen: &UiScreenSnapshot) -> Vec<Vec<Option<String>>> {
    let rows = usize::try_from(screen.rows).unwrap_or_default();
    let columns = usize::try_from(screen.columns).unwrap_or_default();
    let mut cells: Vec<Vec<Option<String>>> = vec![vec![None; columns]; rows];
    for run in &screen.runs {
        let Some(row) = cells.get_mut(usize::try_from(run.row).unwrap_or(usize::MAX)) else {
            continue;
        };
        let mut column = usize::try_from(run.column).unwrap_or(usize::MAX);
        for character in run.text.chars() {
            let width = UnicodeWidthChar::width(character).unwrap_or(1);
            if width == 0 {
                if let Some(Some(previous)) = column
                    .checked_sub(1)
                    .and_then(|previous| row.get_mut(previous))
                {
                    previous.push(character);
                }
                continue;
            }
            if column >= row.len() {
                break;
            }
            row[column] = Some(character.to_string());
            for continuation in 1..width {
                if let Some(cell) = row.get_mut(column + continuation) {
                    *cell = Some(String::new());
                }
            }
            column = column.saturating_add(width);
        }
    }
    cells
}

fn screen_selection_text(screen: &UiScreenSnapshot, selection: &RemoteTerminalSelection) -> String {
    let cells = screen_cells(screen);
    let (start, end) = selection.bounds();
    let mut lines = Vec::new();
    for row in start.row..=end.row.min(screen.rows.saturating_sub(1)) {
        let first = if row == start.row { start.column } else { 0 };
        let last = if row == end.row {
            end.column
        } else {
            screen.columns.saturating_sub(1)
        }
        .min(screen.columns.saturating_sub(1));
        let mut line = String::new();
        for column in first..=last {
            match cells
                .get(usize::try_from(row).unwrap_or(usize::MAX))
                .and_then(|row| row.get(usize::try_from(column).unwrap_or(usize::MAX)))
            {
                Some(Some(text)) => line.push_str(text),
                Some(None) | None => line.push(' '),
            }
        }
        lines.push(line.trim_end_matches(' ').to_owned());
    }
    lines.join("\r\n")
}

fn create_terminal_font(
    window: &ControlWindow,
    family: &str,
    size: u16,
) -> Result<(agenterm_platform::font::NativeFont, i32, i32)> {
    let font = window
        .create_font(agenterm_platform::font::FontRequest {
            family,
            point_size: size,
        })
        .map_err(|error| anyhow::anyhow!(error))?;
    let metrics = font.metrics();
    Ok((
        font,
        metrics.cell_width.round().max(1.0) as i32,
        metrics.cell_height.round().max(1.0) as i32,
    ))
}

fn paint_screen(
    device: &mut dyn ControlCanvas,
    terminal: ProductPixelRect,
    screen: &UiScreenSnapshot,
    cell_width: i32,
    cell_height: i32,
    palette: &ThemePalette,
) {
    for run in &screen.runs {
        let left = terminal.left + i32::try_from(run.column).unwrap_or(0) * cell_width;
        let top = terminal.top + i32::try_from(run.row).unwrap_or(0) * cell_height;
        let right = left
            + i32::try_from(run.columns)
                .unwrap_or(0)
                .saturating_mul(cell_width);
        let rect = ProductPixelRect {
            left,
            top,
            right: right.min(terminal.right),
            bottom: (top + cell_height).min(terminal.bottom),
        };
        if rect.left >= terminal.right || rect.top >= terminal.bottom {
            continue;
        }
        let (foreground, background) = style_colors(&run.style, palette);
        if background != palette.terminal_background.canvas_rgb() {
            fill(device, &rect, background);
        }
        if !run.text.is_empty() {
            device.text(PixelPoint::new(left, top), &run.text, foreground);
        }
    }
    if screen.cursor.visible {
        let cursor = ProductPixelRect {
            left: terminal.left + i32::try_from(screen.cursor.column).unwrap_or(0) * cell_width,
            top: terminal.top + i32::try_from(screen.cursor.row).unwrap_or(0) * cell_height,
            right: terminal.left
                + (i32::try_from(screen.cursor.column).unwrap_or(0) + 1) * cell_width,
            bottom: terminal.top
                + (i32::try_from(screen.cursor.row).unwrap_or(0) + 1) * cell_height,
        };
        frame(device, &cursor, palette.accent.canvas_rgb());
    }
}

fn style_colors(style: &UiCellStyle, palette: &ThemePalette) -> (Rgb8, Rgb8) {
    let mut foreground = terminal_color(style.foreground, palette, true);
    let mut background = terminal_color(style.background, palette, false);
    if style.inverse {
        std::mem::swap(&mut foreground, &mut background);
    }
    (foreground, background)
}

fn terminal_color(color: UiColor, palette: &ThemePalette, foreground: bool) -> Rgb8 {
    match color {
        UiColor::Default if foreground => palette.terminal_foreground.canvas_rgb(),
        UiColor::Default => palette.terminal_background.canvas_rgb(),
        UiColor::Indexed { index } => {
            if index < 16 {
                palette.ansi[usize::from(index)].canvas_rgb()
            } else {
                indexed_rgb(index).canvas_rgb()
            }
        }
        UiColor::Rgb { red, green, blue } => Rgb::new(red, green, blue).canvas_rgb(),
    }
}

fn indexed_rgb(index: u8) -> Rgb {
    if index >= 232 {
        let value = 8_u8.saturating_add(index.saturating_sub(232).saturating_mul(10));
        return Rgb::new(value, value, value);
    }
    let cube = index.saturating_sub(16);
    let channel = |value: u8| {
        let value = value % 6;
        if value == 0 { 0 } else { 55 + value * 40 }
    };
    Rgb::new(channel(cube / 36), channel(cube / 6), channel(cube))
}

fn stable_tab_number(id: &str) -> Option<u64> {
    id.strip_prefix('@')?.parse().ok()
}

fn remote_tree_rows(tabs: &[UiTabBootstrap]) -> Vec<RemoteTreeRow> {
    let nodes = tabs
        .iter()
        .filter_map(|tab| {
            Some(TabTreeNode {
                id: stable_tab_number(&tab.id)?,
                parent_id: tab.parent_id.as_deref().and_then(stable_tab_number),
                sort_key: tab.index,
            })
        })
        .collect::<Vec<_>>();
    tree_rows(&nodes)
        .into_iter()
        .filter(|row| {
            !row.ancestors.iter().any(|ancestor| {
                tabs.iter()
                    .any(|tab| stable_tab_number(&tab.id) == Some(*ancestor) && tab.collapsed)
            })
        })
        .filter_map(|row| {
            let tab_index = tabs
                .iter()
                .position(|tab| stable_tab_number(&tab.id) == Some(row.id))?;
            Some(RemoteTreeRow {
                tab_index,
                depth: row.depth,
                is_last: row.is_last,
                guides: row.guides,
                has_children: nodes.iter().any(|node| node.parent_id == Some(row.id)),
                collapsed: tabs[tab_index].collapsed,
            })
        })
        .collect()
}

fn remote_tab_depth(tabs: &[UiTabBootstrap], tab: &UiTabBootstrap) -> usize {
    let mut depth = 0_usize;
    let mut parent = tab.parent_id.as_deref();
    while let Some(parent_id) = parent {
        if depth >= tabs.len() {
            break;
        }
        let Some(parent_tab) = tabs.iter().find(|candidate| candidate.id == parent_id) else {
            break;
        };
        depth += 1;
        parent = parent_tab.parent_id.as_deref();
    }
    depth
}

fn tree_action_density_name(density: TreeRowActionDensity) -> &'static str {
    match density {
        TreeRowActionDensity::Full => "full",
        TreeRowActionDensity::Compact => "compact",
    }
}

fn close_button_snapshot(action: &str, label: &str) -> serde_json::Value {
    serde_json::json!({
        "action": action,
        "label": label,
        "text_alignment": {
            "horizontal": "center",
            "vertical": "center",
            "win32_draw_text_format": WINDOW_CLOSE_BUTTON_TEXT_FORMAT,
        },
    })
}

fn fill(device: &mut dyn ControlCanvas, rect: &ProductPixelRect, color: Rgb8) {
    device.fill_rect(control_rect(*rect), color);
}

fn frame(device: &mut dyn ControlCanvas, rect: &ProductPixelRect, color: Rgb8) {
    device.stroke_rect(control_rect(*rect), color, 1);
}

fn draw_text(device: &mut dyn ControlCanvas, rect: ProductPixelRect, text: &str, color: Rgb8) {
    device.text_rect(
        control_rect(rect),
        text,
        color,
        TextOptions {
            horizontal: TextHorizontalAlignment::Left,
            vertical_center: true,
            single_line: true,
            end_ellipsis: true,
        },
    );
}

fn win_rect(rect: ProductPixelRect) -> ProductPixelRect {
    ProductPixelRect {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_bridge::{UI_SCREEN_SCHEMA_VERSION, UiCellRun, UiCursorSnapshot};

    fn plain_style() -> UiCellStyle {
        UiCellStyle {
            foreground: UiColor::Default,
            background: UiColor::Default,
            bold: false,
            italic: false,
            underline: false,
            inverse: false,
        }
    }

    #[test]
    fn selection_text_preserves_wide_cells_and_multiline_bounds() {
        let screen = UiScreenSnapshot {
            schema_version: UI_SCREEN_SCHEMA_VERSION,
            tab_id: "@1".to_owned(),
            generation: 7,
            terminal_title: "terminal".to_owned(),
            rows: 2,
            columns: 8,
            alternate_screen: false,
            application_cursor: false,
            bracketed_paste: false,
            mouse_protocol_mode: "none".to_owned(),
            mouse_protocol_encoding: "default".to_owned(),
            scrollback_offset: 0,
            max_scrollback: 0,
            cursor: UiCursorSnapshot {
                row: 1,
                column: 4,
                visible: true,
            },
            runs: vec![
                UiCellRun {
                    row: 0,
                    column: 0,
                    columns: 4,
                    text: "A界B".to_owned(),
                    style: plain_style(),
                },
                UiCellRun {
                    row: 1,
                    column: 0,
                    columns: 4,
                    text: "tail".to_owned(),
                    style: plain_style(),
                },
            ],
            complete: true,
            truncated: false,
        };
        let mut selection = RemoteTerminalSelection {
            tab_id: "@1".to_owned(),
            rows: 2,
            columns: 8,
            gesture: RemoteSelectionGesture::begin(
                "@1".to_owned(),
                RemotePoint { row: 0, column: 1 },
            ),
            cached_text: Some("cached".to_owned()),
        };
        selection.drag_to(RemotePoint { row: 1, column: 1 });
        assert!(selection.complete());
        assert_eq!(screen_selection_text(&screen, &selection), "界B\r\nta");
    }

    #[test]
    fn selection_highlight_rects_match_multiline_terminal_cells() {
        let screen = UiScreenSnapshot {
            schema_version: UI_SCREEN_SCHEMA_VERSION,
            tab_id: "@1".to_owned(),
            generation: 1,
            terminal_title: String::new(),
            rows: 3,
            columns: 8,
            alternate_screen: false,
            application_cursor: false,
            bracketed_paste: false,
            mouse_protocol_mode: "none".to_owned(),
            mouse_protocol_encoding: "default".to_owned(),
            scrollback_offset: 0,
            max_scrollback: 0,
            cursor: UiCursorSnapshot {
                row: 0,
                column: 0,
                visible: true,
            },
            runs: Vec::new(),
            complete: true,
            truncated: false,
        };
        let mut selection = RemoteTerminalSelection {
            tab_id: "@1".to_owned(),
            rows: 3,
            columns: 8,
            gesture: RemoteSelectionGesture::begin(
                "@1".to_owned(),
                RemotePoint { row: 0, column: 2 },
            ),
            cached_text: None,
        };
        selection.drag_to(RemotePoint { row: 2, column: 3 });
        assert!(selection.complete());
        assert_eq!(
            terminal_selection_highlight_rects(
                &selection,
                &screen,
                ProductPixelRect {
                    left: 10,
                    top: 20,
                    right: 90,
                    bottom: 50,
                },
                10,
                10,
            ),
            vec![
                ProductPixelRect {
                    left: 30,
                    top: 20,
                    right: 90,
                    bottom: 30,
                },
                ProductPixelRect {
                    left: 10,
                    top: 30,
                    right: 90,
                    bottom: 40,
                },
                ProductPixelRect {
                    left: 10,
                    top: 40,
                    right: 50,
                    bottom: 50,
                },
            ]
        );
    }

    #[test]
    fn selection_survives_content_generations_but_not_geometry_changes() {
        let mut selection = RemoteTerminalSelection {
            tab_id: "@1".to_owned(),
            rows: 2,
            columns: 8,
            gesture: RemoteSelectionGesture::begin(
                "@1".to_owned(),
                RemotePoint { row: 0, column: 1 },
            ),
            cached_text: None,
        };
        selection.drag_to(RemotePoint { row: 1, column: 1 });
        assert_eq!(selection.phase(), SelectionGesturePhase::Dragging);
        assert!(selection.complete());
        selection.cached_text = Some("selected".to_owned());
        assert!(selection.can_copy());

        let mut screen = UiScreenSnapshot {
            schema_version: UI_SCREEN_SCHEMA_VERSION,
            tab_id: "@1".to_owned(),
            generation: 8,
            terminal_title: String::new(),
            rows: 2,
            columns: 8,
            alternate_screen: false,
            application_cursor: false,
            bracketed_paste: false,
            mouse_protocol_mode: "none".to_owned(),
            mouse_protocol_encoding: "default".to_owned(),
            scrollback_offset: 0,
            max_scrollback: 0,
            cursor: UiCursorSnapshot {
                row: 0,
                column: 0,
                visible: true,
            },
            runs: Vec::new(),
            complete: true,
            truncated: false,
        };
        assert!(selection.matches_screen(&screen));
        screen.generation += 1;
        assert!(selection.matches_screen(&screen));
        screen.rows += 1;
        assert!(!selection.matches_screen(&screen));
    }

    #[test]
    fn prepared_selection_cancels_instead_of_becoming_completed() {
        let mut selection = RemoteTerminalSelection {
            tab_id: "@1".to_owned(),
            rows: 24,
            columns: 80,
            gesture: RemoteSelectionGesture::begin(
                "@1".to_owned(),
                RemotePoint { row: 2, column: 3 },
            ),
            cached_text: None,
        };
        assert!(!selection.complete());
        assert_eq!(selection.phase(), SelectionGesturePhase::Cancelled);
        assert!(!selection.can_copy());
    }

    #[test]
    fn only_copyable_selection_claims_ctrl_c_from_the_pty() {
        let mut selection = RemoteTerminalSelection {
            tab_id: "@1".to_owned(),
            rows: 24,
            columns: 80,
            gesture: RemoteSelectionGesture::begin(
                "@1".to_owned(),
                RemotePoint { row: 2, column: 3 },
            ),
            cached_text: None,
        };
        assert!(!selection_claims_copy_shortcut(None));
        assert!(!selection_claims_copy_shortcut(Some(&selection)));

        selection.drag_to(RemotePoint { row: 2, column: 8 });
        assert!(selection.complete());
        assert!(!selection_claims_copy_shortcut(Some(&selection)));
        selection.cached_text = Some("selected".to_owned());
        assert!(selection_claims_copy_shortcut(Some(&selection)));
    }

    #[test]
    fn terminal_paste_normalizes_lines_and_filters_controls() {
        assert_eq!(
            normalize_terminal_paste("one\r\ntwo\nthree\rfour\t\u{1b}[31m\0"),
            "one\rtwo\rthree\rfour\t[31m"
        );
    }

    #[test]
    fn terminal_named_key_path_owns_modifier_sensitive_and_char_echo_keys() {
        assert_eq!(windows_terminal_named_key(KEY_TAB), Some("Tab"));
        assert_eq!(windows_terminal_named_key(KEY_INSERT), Some("Insert"));
        assert_eq!(windows_terminal_named_key(KEY_DELETE), Some("Delete"));
        assert_eq!(windows_terminal_named_key(KEY_F12), Some("F12"));
        assert_eq!(windows_terminal_named_key(u16::from(b'A')), None);
        assert!(terminal_char_is_named_key_echo(0x09));
        assert!(terminal_char_is_named_key_echo(0x1b));
        assert!(!terminal_char_is_named_key_echo(u16::from(b'A')));
    }

    #[test]
    fn paint_back_buffer_dimensions_are_positive_and_bounded() {
        assert_eq!(
            bounded_frame_dimensions(ProductPixelRect {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1080,
            }),
            Some((1920, 1080))
        );
        assert_eq!(
            bounded_frame_dimensions(ProductPixelRect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 1080,
            }),
            None
        );
        assert_eq!(
            bounded_frame_dimensions(ProductPixelRect {
                left: 0,
                top: 0,
                right: 10_000,
                bottom: 10_000,
            }),
            None
        );
    }

    #[test]
    fn terminal_resize_suppresses_current_and_in_flight_grid_requests() {
        let requested = RemoteTerminalResize {
            server_epoch: "epoch-a".to_owned(),
            tab_id: "@1".to_owned(),
            rows: 24,
            columns: 80,
        };
        assert_eq!(
            terminal_resize_decision(24, 80, None, &requested),
            RemoteTerminalResizeDecision::Current
        );
        assert_eq!(
            terminal_resize_decision(30, 100, Some(&requested), &requested),
            RemoteTerminalResizeDecision::InFlight
        );
        assert_eq!(
            terminal_resize_decision(30, 100, None, &requested),
            RemoteTerminalResizeDecision::Send
        );

        let replacement_epoch = RemoteTerminalResize {
            server_epoch: "epoch-b".to_owned(),
            ..requested.clone()
        };
        assert_eq!(
            terminal_resize_decision(30, 100, Some(&requested), &replacement_epoch),
            RemoteTerminalResizeDecision::Send
        );
        let replacement_tab = RemoteTerminalResize {
            tab_id: "@2".to_owned(),
            ..requested.clone()
        };
        assert_eq!(
            terminal_resize_decision(30, 100, Some(&requested), &replacement_tab),
            RemoteTerminalResizeDecision::Send
        );
    }

    #[test]
    fn terminal_resize_worker_keeps_only_the_latest_queued_request() {
        fn requested(rows: u16) -> RemoteTerminalResize {
            RemoteTerminalResize {
                server_epoch: "epoch-a".to_owned(),
                tab_id: "@1".to_owned(),
                rows,
                columns: 80,
            }
        }

        let worker = RemoteTerminalResizeWorker::spawn().expect("spawn resize worker");
        let executed = Arc::new(Mutex::new(Vec::new()));
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let (started_sender, started_receiver) = mpsc::channel();

        let first_executed = Arc::clone(&executed);
        let first_release = Arc::clone(&release);
        worker.queue(RemoteTerminalResizeTask {
            requested: requested(21),
            execute: Box::new(move || {
                first_executed.lock().expect("record first resize").push(21);
                started_sender.send(()).expect("signal first resize");
                let (lock, wake) = &*first_release;
                let mut released = lock.lock().expect("lock resize release");
                while !*released {
                    released = wake.wait(released).expect("wait resize release");
                }
                Ok(())
            }),
        });
        started_receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("first resize started off-thread");

        let middle_executed = Arc::clone(&executed);
        worker.queue(RemoteTerminalResizeTask {
            requested: requested(22),
            execute: Box::new(move || {
                middle_executed
                    .lock()
                    .expect("record middle resize")
                    .push(22);
                Ok(())
            }),
        });
        let latest_executed = Arc::clone(&executed);
        worker.queue(RemoteTerminalResizeTask {
            requested: requested(23),
            execute: Box::new(move || {
                latest_executed
                    .lock()
                    .expect("record latest resize")
                    .push(23);
                Ok(())
            }),
        });

        let (lock, wake) = &*release;
        *lock.lock().expect("release first resize") = true;
        wake.notify_one();

        let first = worker
            .results
            .recv_timeout(Duration::from_secs(2))
            .expect("receive first resize result");
        let latest = worker
            .results
            .recv_timeout(Duration::from_secs(2))
            .expect("receive latest resize result");
        assert_eq!(first.requested.rows, 21);
        assert!(first.result.is_ok());
        assert_eq!(latest.requested.rows, 23);
        assert!(latest.result.is_ok());
        assert_eq!(
            *executed.lock().expect("read executed resizes"),
            vec![21, 23]
        );
        assert!(matches!(
            worker.try_result(),
            Err(mpsc::TryRecvError::Empty)
        ));
    }

    #[test]
    fn terminal_paste_worker_never_blocks_the_ui_request_path() {
        let worker = RemoteTerminalPasteWorker::spawn().expect("spawn paste worker");
        let (started_sender, started_receiver) = mpsc::channel();
        let (release_sender, release_receiver) = mpsc::channel();
        let requested = RemoteTerminalPasteRequest {
            server_epoch: "epoch-a".to_owned(),
            tab_id: "@1".to_owned(),
            bracketed: true,
        };
        worker
            .queue(RemoteTerminalPasteTask {
                requested: requested.clone(),
                read: Box::new(move || {
                    started_sender.send(()).expect("signal paste read");
                    release_receiver
                        .recv_timeout(Duration::from_secs(2))
                        .expect("release paste read");
                    Ok("payload".to_owned())
                }),
            })
            .expect("queue paste read");
        started_receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("paste read started off-thread");
        assert!(matches!(
            worker.try_result(),
            Err(mpsc::TryRecvError::Empty)
        ));
        release_sender.send(()).expect("release paste read");
        let completed = worker
            .results
            .recv_timeout(Duration::from_secs(2))
            .expect("paste read result");
        assert_eq!(completed.requested, requested);
        assert_eq!(completed.result.as_deref(), Ok("payload"));
    }

    #[test]
    fn native_toolbar_ids_resolve_to_stable_product_actions() {
        let cases = [
            (TABS_ID, action::TOGGLE_TABS),
            (NEW_ID, action::NEW_TAB),
            (CONTROL_CENTER_ID, action::OPEN_CONTROL_CENTER),
            (SETTINGS_ID, action::OPEN_SETTINGS),
            (LOCALE_ID, action::TOGGLE_LOCALE),
            (FONT_DECREASE_ID, action::FONT_DECREASE),
            (FONT_INCREASE_ID, action::FONT_INCREASE),
        ];
        for (control_id, action_id) in cases {
            assert_eq!(
                windows_toolbar_hit(control_id).map(WindowsToolbarHit::action_id),
                Some(action_id)
            );
        }
        assert_eq!(windows_toolbar_hit(SEND_ID), None);
    }

    #[test]
    fn immediate_toolbar_actions_return_keyboard_focus_to_terminal() {
        for action_id in [
            action::TOGGLE_TABS,
            action::TOGGLE_LOCALE,
            action::FONT_DECREASE,
            action::FONT_INCREASE,
        ] {
            assert!(toolbar_action_returns_terminal_focus(action_id));
        }
        for action_id in [
            action::NEW_TAB,
            action::OPEN_CONTROL_CENTER,
            action::OPEN_SETTINGS,
        ] {
            assert!(!toolbar_action_returns_terminal_focus(action_id));
        }
    }

    #[test]
    fn surface_navigation_is_directional_and_modifier_exact() {
        fn navigate(
            source: RemoteFocusSurface,
            control: bool,
            shift: bool,
            alt: bool,
            key: u32,
        ) -> Option<RemoteFocusSurface> {
            let state = FocusState::new(source.to_shared(), FocusTransitionGate::default());
            let target = state.navigate(
                FocusDirection::from_virtual_key_code(key)?,
                control,
                shift,
                alt,
            )?;
            Some(RemoteFocusSurface::from_shared(target))
        }
        assert_eq!(
            navigate(RemoteFocusSurface::Terminal, true, false, false, 0x28),
            Some(RemoteFocusSurface::Composer)
        );
        assert_eq!(
            navigate(RemoteFocusSurface::Composer, true, false, false, 0x26),
            Some(RemoteFocusSurface::Terminal)
        );
        assert_eq!(
            navigate(RemoteFocusSurface::Terminal, true, false, false, 0x25),
            Some(RemoteFocusSurface::Tabs)
        );
        assert_eq!(
            navigate(RemoteFocusSurface::Tabs, true, false, false, 0x27),
            Some(RemoteFocusSurface::Terminal)
        );
        assert_eq!(
            navigate(RemoteFocusSurface::Composer, true, false, false, 0x25),
            None
        );
        assert_eq!(
            navigate(RemoteFocusSurface::Terminal, true, true, false, 0x28),
            None
        );
        assert_eq!(
            navigate(RemoteFocusSurface::Terminal, true, false, true, 0x28),
            None
        );
    }

    #[test]
    fn sidebar_rows_are_positioned_to_the_right_of_the_left_scrollbar() {
        let sidebar = ProductPixelRect {
            left: 0,
            top: 0,
            right: 244,
            bottom: 700,
        };
        let geometry = sidebar_tree_row_geometry(sidebar, 0, 2, TreeRowMode::Editing);
        let track = sidebar_scrollbar_track(sidebar);

        assert_eq!(track.left, sidebar.left);
        assert_eq!(track.right, TERMINAL_SCROLLBAR_WIDTH);
        assert_eq!(geometry.row.left, TERMINAL_SCROLLBAR_WIDTH + 2);
        assert!(geometry.disclosure_hit.left >= track.right);
        assert!(geometry.actions.bounds.right <= sidebar.right);
        let editors = geometry.editors.expect("editing geometry has editors");
        assert!(editors.name.left >= track.right);
    }

    #[test]
    fn sidebar_double_click_candidate_requires_stable_tab_and_geometry() {
        let click = RemoteSidebarTextClick {
            tab_id: "@7".to_owned(),
            geometry_generation: 11,
        };
        assert!(click.matches("@7", 11));
        assert!(!click.matches("@8", 11));
        assert!(!click.matches("@7", 12));
    }
}
