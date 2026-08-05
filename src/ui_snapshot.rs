//! Shared `ui-snapshot` / UI client state JSON fragments for GUI hosts.
use crate::{
    locale::{LocaleId, UiText},
    settings::AppConfig,
    theme::AppearancePreset,
    ui_bridge::UI_CLIENT_STATE_SCHEMA_VERSION,
    ui_geometry::{PixelRect, TerminalScrollbarGeometry, pixel_rect_json},
    working_context::{CwdTracker, ProxyState, ShellKind},
};

pub(crate) const PROJECTION_EMBEDDED_GUI: &str = "embedded_gui";
pub(crate) const PROJECTION_REPLACEABLE_UI_CLIENT: &str = "replaceable_ui_client";

pub(crate) const SYSTEM_MENU_COPY_ID: u32 = 0x1f00;
pub(crate) const SYSTEM_MENU_PASTE_ID: u32 = 0x1f10;
pub(crate) const SYSTEM_MENU_TOGGLE_TABS_ID: u32 = 0x1f20;
pub(crate) const SYSTEM_MENU_OPEN_INSTANCE_ID: u32 = 0x1f30;

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

pub(crate) fn locale_json(locale: LocaleId) -> serde_json::Value {
    serde_json::json!({
        "id": locale.as_str(),
        "toolbar_label": locale.toolbar_label(),
        "controls": {
            "send": locale.text(UiText::Send),
            "settings": locale.text(UiText::Settings),
            "new": locale.text(UiText::New),
            "apply": locale.text(UiText::Apply),
            "save": locale.text(UiText::Save),
        },
    })
}

pub(crate) fn settings_json(
    config: &AppConfig,
    locale: LocaleId,
    settings_open: bool,
    preset_draft: Option<&str>,
    address: &str,
    active_tab_id: Option<&str>,
) -> serde_json::Value {
    let terminal_override =
        active_tab_id.and_then(|tab_id| config.terminal_override_entry(address, tab_id).cloned());
    let effective = config.effective_terminal_appearance(address, active_tab_id);
    let draft = preset_draft.unwrap_or(config.appearance_preset.as_str());
    serde_json::json!({
        "terminal_font_family": config.terminal_font_family,
        "terminal_font_size": config.terminal_font_size,
        "appearance_preset": config.appearance_preset.as_str(),
        "skin": config.appearance_preset.skin().as_str(),
        "luminance": config.appearance_preset.luminance().as_str(),
        "color_theme": config.appearance_preset.color_theme().as_str(),
        "current_terminal_override": terminal_override,
        "effective": {
            "terminal_font_family": effective.terminal_font_family,
            "terminal_font_size": effective.terminal_font_size,
            "appearance_preset": effective.appearance_preset.as_str(),
            "skin": effective.appearance_preset.skin().as_str(),
            "luminance": effective.appearance_preset.luminance().as_str(),
            "color_theme": effective.color_theme().as_str(),
        },
        "appearance_preset_draft": settings_open.then(|| draft),
        "theme_draft": settings_open.then(|| draft),
        "theme_options": AppearancePreset::ALL.map(|preset| {
            let metrics = preset.skin_metrics();
            serde_json::json!({
            "id": preset.as_str(),
            "label": preset.label(locale),
            "description": preset.description(locale),
            "skin": preset.skin().as_str(),
            "luminance": preset.luminance().as_str(),
            "color_theme": preset.color_theme().as_str(),
            "accent": format!(
                "#{:02X}{:02X}{:02X}",
                preset.palette().accent.red,
                preset.palette().accent.green,
                preset.palette().accent.blue
            ),
            "metrics": {
                "corner_radius_control_px": metrics.corner_radius_control_px,
                "corner_radius_modal_px": metrics.corner_radius_modal_px,
                "border_width_px": metrics.border_width_px,
                "scrollbar_style": metrics.scrollbar_style,
            },
        })}),
        "appearance_metrics": {
            "corner_radius_control_px": config.appearance_preset.skin_metrics().corner_radius_control_px,
            "corner_radius_modal_px": config.appearance_preset.skin_metrics().corner_radius_modal_px,
            "border_width_px": config.appearance_preset.skin_metrics().border_width_px,
            "scrollbar_style": config.appearance_preset.skin_metrics().scrollbar_style,
        },
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
    autoscroll_active: bool,
) -> serde_json::Value {
    serde_json::json!({
        "selection": selection.map(|selection| serde_json::json!({
            "phase": if selection.dragging { "dragging" } else { "complete" },
            "tab_id": format!("@{}", selection.tab_id),
            "selection": {
                "start": {"row": selection.start_row, "col": selection.start_col},
                "end": {"row": selection.end_row, "col": selection.end_col},
            },
            "autoscroll": {"active": autoscroll_active},
        })),
        "raw_mouse_arbitration": true,
        "rectangular_selection": false,
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

pub(crate) fn is_replaceable_ui_client_snapshot_visible(snapshot_json: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(snapshot_json) else {
        return false;
    };
    let Some(window) = value.get("window").and_then(serde_json::Value::as_object) else {
        return false;
    };
    window.get("detached").and_then(serde_json::Value::as_bool) != Some(true)
        && window.get("visible").and_then(serde_json::Value::as_bool) == Some(true)
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
        let settings = settings_json(
            &config,
            LocaleId::English,
            true,
            Some("classic-night"),
            "local",
            Some("@1"),
        );
        assert_eq!(
            settings["terminal_font_family"],
            config.terminal_font_family
        );
        assert_eq!(settings["appearance_preset"], "classic-night");
        assert_eq!(settings["theme_options"].as_array().unwrap().len(), 4);
        assert_eq!(locale_json(LocaleId::English)["controls"]["send"], "Send");
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

    #[test]
    fn replaceable_ui_client_snapshot_visible_recognizes_detached_or_hidden_clients_as_invisible() {
        let visible = serde_json::json!({
            "projection": PROJECTION_REPLACEABLE_UI_CLIENT,
            "window": {"visible": true, "detached": false},
        });
        let detached = serde_json::json!({
            "projection": PROJECTION_REPLACEABLE_UI_CLIENT,
            "window": {"visible": true, "detached": true},
        });
        let hidden = serde_json::json!({
            "projection": PROJECTION_REPLACEABLE_UI_CLIENT,
            "window": {"visible": false, "detached": false},
        });
        assert!(is_replaceable_ui_client_snapshot_visible(
            &visible.to_string()
        ));
        assert!(!is_replaceable_ui_client_snapshot_visible(
            &detached.to_string()
        ));
        assert!(!is_replaceable_ui_client_snapshot_visible(
            &hidden.to_string()
        ));
    }

    #[test]
    fn terminal_interaction_json_without_selection_matches_win_shape() {
        let json = terminal_interaction_json(None, false);
        assert_eq!(
            json,
            serde_json::json!({
                "selection": null,
                "raw_mouse_arbitration": true,
                "rectangular_selection": false,
            })
        );
        assert!(json.get("gesture_phase").is_none());
    }

    #[test]
    fn terminal_interaction_json_with_selection_matches_win_shape() {
        let json = terminal_interaction_json(
            Some(TerminalSelectionSnapshotInput {
                tab_id: 1,
                start_row: 0,
                start_col: 2,
                end_row: 1,
                end_col: 5,
                dragging: true,
            }),
            false,
        );
        assert_eq!(
            json,
            serde_json::json!({
                "selection": {
                    "phase": "dragging",
                    "tab_id": "@1",
                    "selection": {
                        "start": {"row": 0, "col": 2},
                        "end": {"row": 1, "col": 5},
                    },
                    "autoscroll": {"active": false},
                },
                "raw_mouse_arbitration": true,
                "rectangular_selection": false,
            })
        );
        assert!(json.get("gesture_phase").is_none());
    }

    #[test]
    fn terminal_interaction_json_reports_autoscroll_active() {
        let json = terminal_interaction_json(
            Some(TerminalSelectionSnapshotInput {
                tab_id: 2,
                start_row: 3,
                start_col: 0,
                end_row: 3,
                end_col: 4,
                dragging: true,
            }),
            true,
        );
        assert_eq!(json["selection"]["autoscroll"]["active"], true);
    }

    #[test]
    fn per_tab_selection_json_matches_win_shape() {
        let selection = serde_json::json!({
            "start": {"row": 0, "col": 1},
            "end": {"row": 1, "col": 1},
            "dragging": false,
        });
        assert!(selection.get("phase").is_none());
        assert!(selection.get("tab_id").is_none());
        assert_eq!(selection["start"]["row"], 0);
        assert_eq!(selection["start"]["col"], 1);
        assert_eq!(selection["end"]["row"], 1);
        assert_eq!(selection["end"]["col"], 1);
        assert_eq!(selection["dragging"], false);
    }
}
