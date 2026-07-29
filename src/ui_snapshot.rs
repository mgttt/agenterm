//! Shared `ui-snapshot` / UI client state JSON fragments for GUI hosts.
use crate::{
    settings::AppConfig,
    theme::ThemeId,
    ui_bridge::UI_CLIENT_STATE_SCHEMA_VERSION,
    ui_geometry::{PixelRect, TerminalScrollbarGeometry, pixel_rect_json},
    working_context::{CwdTracker, ProxyState, ShellKind},
};

pub(crate) const PROJECTION_EMBEDDED_GUI: &str = "embedded_gui";
pub(crate) const PROJECTION_REPLACEABLE_UI_CLIENT: &str = "replaceable_ui_client";

pub(crate) const SYSTEM_MENU_COPY_ID: u32 = 0x1f00;
pub(crate) const SYSTEM_MENU_PASTE_ID: u32 = 0x1f10;
pub(crate) const SYSTEM_MENU_TOGGLE_TABS_ID: u32 = 0x1f20;

pub(crate) fn event_position_json(epoch: &str, sequence: u64) -> serde_json::Value {
    serde_json::json!({
        "epoch": epoch,
        "sequence": sequence,
    })
}

pub(crate) fn scrollbar_state_json(
    geometry: &TerminalScrollbarGeometry,
    offset: usize,
    maximum: usize,
) -> serde_json::Value {
    serde_json::json!({
        "visible": true,
        "track": pixel_rect_json(geometry.track),
        "thumb": pixel_rect_json(geometry.thumb),
        "offset": offset,
        "max_offset": maximum,
    })
}

pub(crate) fn locale_json() -> serde_json::Value {
    serde_json::json!({
        "id": "en-US",
        "controls": {
            "send": "Send",
            "settings": "Settings",
            "new": "New",
            "apply": "Apply",
            "save": "Save",
        },
    })
}

pub(crate) fn settings_json(
    config: &AppConfig,
    settings_open: bool,
    theme_draft: Option<&str>,
) -> serde_json::Value {
    serde_json::json!({
        "terminal_font_family": config.terminal_font_family,
        "terminal_font_size": config.terminal_font_size,
        "color_theme": config.color_theme.as_str(),
        "theme_draft": settings_open.then(|| theme_draft.unwrap_or(config.color_theme.as_str())),
        "theme_options": ThemeId::ALL.map(|theme| serde_json::json!({
            "id": theme.as_str(),
            "label": theme.label(),
        })),
        "tabs_visible": config.tabs_visible,
        "tabs_width": config.tabs_width,
    })
}

pub(crate) fn archived_proxy_status_json(bounds: PixelRect) -> serde_json::Value {
    serde_json::json!({
        "bounds": pixel_rect_json(bounds),
        "available": false,
        "archived": true,
        "action": serde_json::Value::Null,
        "eye_action": serde_json::Value::Null,
    })
}

pub(crate) fn system_menu_json(
    toggle_checked: bool,
    copy_enabled: bool,
    paste_enabled: bool,
) -> serde_json::Value {
    serde_json::json!({
        "toggle_tabs": {
            "id": SYSTEM_MENU_TOGGLE_TABS_ID,
            "label": "Toggle Tabs",
            "checked": toggle_checked,
        },
        "copy": {
            "id": SYSTEM_MENU_COPY_ID,
            "label": "Copy",
            "enabled": copy_enabled,
        },
        "paste": {
            "id": SYSTEM_MENU_PASTE_ID,
            "label": "Paste",
            "enabled": paste_enabled,
        },
    })
}

pub(crate) fn working_context_json(
    cwd: &CwdTracker,
    shell: ShellKind,
    proxy: &ProxyState,
) -> serde_json::Value {
    let facts = proxy.facts();
    serde_json::json!({
        "cwd": {
            "path": cwd.path(),
            "confirmed_path": cwd.confirmed_path(),
            "confirmed": cwd.confirmed_path().is_some(),
            "source": cwd.source().as_str(),
            "pending": cwd.pending(),
        },
        "shell": shell.as_str(),
        "proxy": {
            "configured": facts.configured,
            "source": facts.source.as_str(),
            "application_state": facts.application_state.as_str(),
            "request_pending": facts.request_pending,
            "endpoint_visible": false,
            "credential_revealed": false,
        },
    })
}

pub(crate) struct TerminalSelectionSnapshotInput {
    pub tab_id: u64,
    pub start_row: u16,
    pub start_col: u16,
    pub end_row: u16,
    pub end_col: u16,
    pub dragging: bool,
}

pub(crate) fn terminal_interaction_json(
    selection: Option<TerminalSelectionSnapshotInput>,
    gesture_phase: Option<&str>,
) -> serde_json::Value {
    serde_json::json!({
        "selection": selection.map(|selection| serde_json::json!({
            "phase": if selection.dragging { "dragging" } else { "complete" },
            "tab_id": format!("@{}", selection.tab_id),
            "selection": {
                "start": {"row": selection.start_row, "col": selection.start_col},
                "end": {"row": selection.end_row, "col": selection.end_col},
            },
            "autoscroll": {"active": false},
        })),
        "raw_mouse_arbitration": false,
        "rectangular_selection": false,
        "gesture_phase": gesture_phase.unwrap_or("none"),
    })
}

pub(crate) fn embedded_window_json(
    title: &str,
    client_width: u32,
    client_height: u32,
) -> serde_json::Value {
    embedded_window_json_with_state(title, client_width, client_height, true, false, "restored")
}

pub(crate) fn embedded_window_json_with_state(
    title: &str,
    client_width: u32,
    client_height: u32,
    visible: bool,
    minimized: bool,
    state: &str,
) -> serde_json::Value {
    serde_json::json!({
        "title": title,
        "client_width": client_width,
        "client_height": client_height,
        "visible": visible,
        "detached": false,
        "minimized": minimized,
        "state": state,
    })
}

pub(crate) fn schema_version_json() -> u32 {
    UI_CLIENT_STATE_SCHEMA_VERSION
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::AppConfig;

    #[test]
    fn locale_and_settings_blocks_match_win_shape() {
        let config = AppConfig::default();
        let settings = settings_json(&config, true, Some("dark"));
        assert_eq!(
            settings["terminal_font_family"],
            config.terminal_font_family
        );
        assert!(settings["theme_options"].is_array());
        assert_eq!(locale_json()["controls"]["send"], "Send");
    }

    #[test]
    fn scrollbar_state_includes_offset() {
        let geometry = crate::ui_geometry::terminal_scrollbar_geometry(
            PixelRect {
                left: 0,
                top: 0,
                right: 800,
                bottom: 600,
            },
            24,
            12,
            48,
        );
        let json = scrollbar_state_json(&geometry, 12, 48);
        assert_eq!(json["offset"], 12);
        assert_eq!(json["max_offset"], 48);
        assert!(json["track"]["width"].is_number());
    }

    #[test]
    fn system_menu_exposes_copy_paste_enabled_flags() {
        let json = system_menu_json(true, true, false);
        assert_eq!(json["toggle_tabs"]["id"], SYSTEM_MENU_TOGGLE_TABS_ID);
        assert_eq!(json["copy"]["enabled"], true);
        assert_eq!(json["paste"]["enabled"], false);
    }

    #[test]
    fn embedded_window_json_with_state_matches_win_shape() {
        let json = embedded_window_json_with_state("AgenTerm", 800, 600, true, true, "minimized");
        assert_eq!(json["minimized"], true);
        assert_eq!(json["state"], "minimized");
        assert_eq!(json["detached"], false);
    }
}
