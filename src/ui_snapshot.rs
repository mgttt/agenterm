//! Shared `ui-snapshot` / UI client state JSON fragments for GUI hosts.
use crate::{
    locale::{LocaleId, UiText},
    settings::AppConfig,
    theme::AppearancePreset,
    ui_bridge::UI_CLIENT_STATE_SCHEMA_VERSION,
    ui_geometry::{
        PixelRect, TERMINAL_SCROLLBAR_WIDTH, TerminalScrollbarGeometry, TreeRowActionDensity,
        TreeRowMode, WorkspaceLayout, WorkspaceLayoutInput, WorkspaceToolbarLayout,
        composer_geometry, pixel_rect_json, sidebar_row_capacity, sidebar_tree_row_geometry,
        terminal_scrollbar_geometry, workspace_layout,
    },
    working_context::{CwdTracker, ProxyState, ShellKind},
};

pub(crate) const PROJECTION_EMBEDDED_GUI: &str = "embedded_gui";
pub(crate) const PROJECTION_REPLACEABLE_UI_CLIENT: &str = "replaceable_ui_client";

/// Bounds in this snapshot were measured from a window that a compositor
/// actually laid out and painted.
pub(crate) const GEOMETRY_SOURCE_RENDERED: &str = "rendered";
/// Bounds in this snapshot were computed by the shared `ui_geometry` layout
/// functions from a **nominal** viewport. No window exists.
///
/// A consumer must treat these as a model of where chrome *would* be, good
/// enough to exercise hit/route/dispatch logic, and never as evidence that
/// anything was rendered. Pixel fidelity still needs a live-window smoke.
pub(crate) const GEOMETRY_SOURCE_SYNTHETIC: &str = "synthetic";

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
        "appearance_preset_draft": settings_open.then_some(draft),
        "theme_draft": settings_open.then_some(draft),
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

/// Shared status-bar shape for the active input method.
///
/// Hosts own discovery of the current input source; snapshots own the stable
/// JSON contract and geometry. A host that cannot observe a source still
/// publishes the segment as disabled so clients never need projection-specific
/// parsing.
pub(crate) fn ime_status_snapshot_json(
    bounds: PixelRect,
    status: Option<&agenterm_platform::ime::ImeStatus>,
) -> serde_json::Value {
    serde_json::json!({
        "bounds": pixel_rect_json(bounds),
        "label": status
            .map(agenterm_platform::ime::ImeStatus::label)
            .unwrap_or_else(|| "IME: off".to_owned()),
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

pub(crate) fn workspace_toolbar_snapshot_json(
    toolbar: WorkspaceToolbarLayout,
) -> serde_json::Value {
    serde_json::json!({
        "bounds": pixel_rect_json(toolbar.bounds),
        "new": pixel_rect_json(toolbar.new_tab),
        "tabs": pixel_rect_json(toolbar.tabs),
        "control_center": pixel_rect_json(toolbar.control_center),
        "settings": pixel_rect_json(toolbar.settings),
        "locale": pixel_rect_json(toolbar.locale),
        "font_decrease": pixel_rect_json(toolbar.font_decrease),
        "font_increase": pixel_rect_json(toolbar.font_increase),
    })
}

// ---------------------------------------------------------------------------
// Synthetic (headless) geometry
// ---------------------------------------------------------------------------
//
// The workspace layout is a **pure function** of a client rectangle plus the
// chrome heights (`src/ui_geometry.rs`). A headless server therefore does not
// need a window to say where the toolbar buttons, tab rows, composer input and
// scrollbars would sit -- it only needs a viewport to feed the same function
// both GUI hosts feed. Publishing that lets an agent exercise perceive->act
// routing in CI, which is impossible while the only geometry lives behind a
// live window.
//
// Everything produced here is tagged `GEOMETRY_SOURCE_SYNTHETIC`.

/// Overrides the nominal headless viewport, as `"<width>x<height>"`.
pub(crate) const HEADLESS_VIEWPORT_ENV: &str = "AGENTERM_HEADLESS_VIEWPORT";

/// Built-in nominal client area.
///
/// A fixed default is deliberate. Deriving the viewport from a tab's rows and
/// columns would require cell metrics, and cell metrics require a font and a
/// DPI that a headless server does not have; inventing them would make the
/// bounds *look* measured while being no more real, and would make them jitter
/// whenever a pane is resized. A constant keeps synthetic geometry
/// reproducible, which is the property CI needs, and the environment override
/// lets a caller replay a specific real window size before checking the same
/// gesture against a live one.
const HEADLESS_DEFAULT_WIDTH: i32 = 1280;
const HEADLESS_DEFAULT_HEIGHT: i32 = 800;
const HEADLESS_MIN_WIDTH: i32 = 320;
const HEADLESS_MIN_HEIGHT: i32 = 240;
const HEADLESS_MAX_EDGE: i32 = 16_384;

/// Chrome heights match the GUI host shipped on *this* platform, so synthetic
/// geometry and the window an agent may attach later agree. The two hosts
/// currently disagree (Windows composer 104, Unix 64), so the value is also
/// published rather than left for a caller to guess. Routed through the
/// platform-kind policy fact instead of a `cfg` split (boundary policy).
fn headless_composer_height() -> i32 {
    if crate::platform::is_windows_host() {
        104
    } else {
        64
    }
}
const HEADLESS_STATUS_HEIGHT: i32 = 26;
const HEADLESS_SERVER_STRIP_HEIGHT: i32 = 34;

/// The nominal client area a headless projection lays chrome out in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HeadlessViewport {
    pub(crate) width: i32,
    pub(crate) height: i32,
    /// `"default"` or `"environment"`.
    pub(crate) source: &'static str,
    pub(crate) composer_height: i32,
    pub(crate) status_height: i32,
    pub(crate) server_strip_height: i32,
}

impl HeadlessViewport {
    pub(crate) fn resolve() -> Self {
        let (width, height, source) = std::env::var(HEADLESS_VIEWPORT_ENV)
            .ok()
            .as_deref()
            .and_then(parse_headless_viewport)
            .map(|(width, height)| (width, height, "environment"))
            .unwrap_or((HEADLESS_DEFAULT_WIDTH, HEADLESS_DEFAULT_HEIGHT, "default"));
        Self {
            width,
            height,
            source,
            composer_height: headless_composer_height(),
            status_height: HEADLESS_STATUS_HEIGHT,
            server_strip_height: HEADLESS_SERVER_STRIP_HEIGHT,
        }
    }

    pub(crate) fn layout(self, tabs_visible: bool, configured_tabs_width: i32) -> WorkspaceLayout {
        workspace_layout(WorkspaceLayoutInput {
            client_width: self.width,
            client_height: self.height,
            tabs_visible,
            configured_tabs_width,
            composer_height: self.composer_height,
            status_height: self.status_height,
            server_strip_height: self.server_strip_height,
        })
    }

    pub(crate) fn json(self) -> serde_json::Value {
        serde_json::json!({
            "width": self.width,
            "height": self.height,
            "source": self.source,
            "environment_variable": HEADLESS_VIEWPORT_ENV,
            "composer_height": self.composer_height,
            "status_height": self.status_height,
            "server_strip_height": self.server_strip_height,
        })
    }
}

/// Parse `"<width>x<height>"`, rejecting anything outside a plausible window.
///
/// An out-of-range request is refused rather than clamped: silently laying out
/// a 40x40 "window" would publish bounds a caller could not act on while
/// looking exactly like a request that was honoured.
fn parse_headless_viewport(raw: &str) -> Option<(i32, i32)> {
    let (width, height) = raw.trim().split_once(['x', 'X'])?;
    let width = width.trim().parse::<i32>().ok()?;
    let height = height.trim().parse::<i32>().ok()?;
    ((HEADLESS_MIN_WIDTH..=HEADLESS_MAX_EDGE).contains(&width)
        && (HEADLESS_MIN_HEIGHT..=HEADLESS_MAX_EDGE).contains(&height))
    .then_some((width, height))
}

/// Terminal facts a headless projection needs to place the scrollbar.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct SyntheticTerminalInput {
    pub(crate) rows: u16,
    pub(crate) cols: u16,
    pub(crate) scrollback_offset: usize,
    pub(crate) max_scrollback: usize,
}

/// The `layout` block of a headless snapshot, in the same shape the embedded
/// GUI publishes so an agent's parser does not fork per projection.
///
/// `composer.visible` and friends stay `false`: visibility describes the real
/// client, of which there is none. The bounds next to them describe where the
/// composer *would* be. That pairing is the honest one -- nothing is on screen,
/// and here is the layout a window would get.
pub(crate) fn synthetic_layout_json(
    viewport: HeadlessViewport,
    layout: &WorkspaceLayout,
    terminal: Option<SyntheticTerminalInput>,
) -> serde_json::Value {
    let composer = composer_geometry(layout.composer);
    serde_json::json!({
        "geometry_source": GEOMETRY_SOURCE_SYNTHETIC,
        "viewport": viewport.json(),
        "sidebar": {
            "x": layout.sidebar.left,
            "y": layout.sidebar.top,
            "visible": layout.tabs_visible,
            "configured_width": layout.configured_tabs_width,
            "effective_width": layout.effective_tabs_width,
            "width": layout.sidebar.width(),
            "height": layout.sidebar.height(),
            "bounds": pixel_rect_json(layout.sidebar),
            "tree": pixel_rect_json(layout.sidebar_tree),
            "resize_grip": layout.resize_grip.map(pixel_rect_json),
            "resizing": false,
            "row_capacity": sidebar_row_capacity(layout.sidebar_tree.height()),
        },
        "toolbar": layout.workspace_toolbar.map(workspace_toolbar_snapshot_json),
        "server_strip": layout.server_strip.map(|strip| serde_json::json!({
            "bounds": pixel_rect_json(strip),
            // Chips enumerate live instances, which is a fleet query rather
            // than layout maths; a headless projection publishes the strip it
            // would draw them into and leaves the roster to `list-instances`.
            "tabs": serde_json::Value::Null,
        })),
        "sidebar_clock": layout.sidebar_clock.map(pixel_rect_json),
        "terminal": {
            "x": layout.terminal.left,
            "y": layout.terminal.top,
            "width": layout.terminal.width(),
            "viewport_width": (layout.terminal.width() - TERMINAL_SCROLLBAR_WIDTH).max(0),
            "height": layout.terminal.height(),
            "bounds": pixel_rect_json(layout.terminal),
            "rows": terminal.map(|terminal| terminal.rows),
            "cols": terminal.map(|terminal| terminal.cols),
            "scrollbar": terminal.map(|terminal| {
                let geometry = terminal_scrollbar_geometry(
                    layout.terminal,
                    usize::from(terminal.rows),
                    terminal.scrollback_offset,
                    terminal.max_scrollback,
                );
                scrollbar_state_json(
                    &geometry,
                    terminal.scrollback_offset,
                    terminal.max_scrollback,
                )
            }),
        },
        "composer": {
            "visible": false,
            "input_visible": false,
            "send_visible": false,
            "x": layout.composer.left,
            "y": layout.composer.top,
            "width": layout.composer.width(),
            "height": layout.composer.height(),
            "bounds": pixel_rect_json(layout.composer),
            "input": {
                "bounds": pixel_rect_json(composer.input),
                "target_rows": 3,
                "vertical_scrollbar": true,
            },
            "send": {"bounds": pixel_rect_json(composer.send)},
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
            "ime": ime_status_snapshot_json(layout.status_segments.ime, None),
            "proxy": archived_proxy_status_json(layout.status_segments.proxy),
            "provider": "placeholder",
        },
    })
}

const fn tree_action_density_name(density: TreeRowActionDensity) -> &'static str {
    match density {
        TreeRowActionDensity::Full => "full",
        TreeRowActionDensity::Compact => "compact",
    }
}

/// Per-row tab geometry (`bounds` / `render` / `actions`) for a headless
/// projection, matching the embedded GUI's keys.
///
/// `actions` is populated only for the active row because only the active row
/// draws Add/Close, exactly as the GUI hosts do.
pub(crate) fn synthetic_tab_row_json(
    layout: &WorkspaceLayout,
    viewport_position: usize,
    depth: usize,
    active: bool,
) -> serde_json::Value {
    let geometry = sidebar_tree_row_geometry(
        layout.sidebar_tree,
        viewport_position,
        depth,
        TreeRowMode::Normal,
    );
    let action = |id: &str, label: &str, bounds: PixelRect| {
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
    serde_json::json!({
        "bounds": pixel_rect_json(geometry.row),
        "render": {
            "mode": "normal",
            "row": pixel_rect_json(geometry.row),
            "selection": pixel_rect_json(geometry.selection),
            "node": {"x": geometry.node_x, "y": geometry.node_y},
            "expander": pixel_rect_json(geometry.expander),
            "status": pixel_rect_json(geometry.status),
            "disclosure_hit": pixel_rect_json(geometry.disclosure_hit),
            "text": pixel_rect_json(geometry.text),
            "name": pixel_rect_json(geometry.name),
            "note": pixel_rect_json(geometry.note),
            "editors": serde_json::Value::Null,
        },
        "actions": active.then(|| serde_json::json!({
            "mode": "normal",
            "density": tree_action_density_name(geometry.actions.density),
            "new_child": action(
                "new-child",
                "Add",
                geometry.actions.add_child.expect("a normal row has Add"),
            ),
            "close": action("close-tab", "Close", geometry.actions.secondary),
        })),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::AppConfig;

    fn rect(value: &serde_json::Value) -> (i64, i64, i64, i64) {
        (
            value["left"].as_i64().expect("left"),
            value["top"].as_i64().expect("top"),
            value["right"].as_i64().expect("right"),
            value["bottom"].as_i64().expect("bottom"),
        )
    }

    #[test]
    fn a_headless_viewport_request_is_honoured_or_refused_but_never_silently_clamped() {
        assert_eq!(parse_headless_viewport("1024x768"), Some((1024, 768)));
        assert_eq!(parse_headless_viewport(" 1024 X 768 "), Some((1024, 768)));
        // A window this small could not hold the chrome an agent would click,
        // so publishing a clamped layout for it would be a lie about what was
        // asked for.
        assert_eq!(parse_headless_viewport("40x40"), None);
        assert_eq!(parse_headless_viewport("99999x600"), None);
        for malformed in ["1024", "1024*768", "axb", "", "-800x600"] {
            assert_eq!(parse_headless_viewport(malformed), None, "{malformed}");
        }
    }

    /// The point of D-1: bounds an agent can aim `ui-input` at exist without a
    /// window, and are labelled so nobody mistakes them for rendered pixels.
    #[test]
    fn synthetic_layout_publishes_actionable_chrome_and_labels_itself() {
        let viewport = HeadlessViewport::resolve();
        let layout = viewport.layout(true, 250);
        let value = synthetic_layout_json(
            viewport,
            &layout,
            Some(SyntheticTerminalInput {
                rows: 30,
                cols: 100,
                scrollback_offset: 0,
                max_scrollback: 500,
            }),
        );

        assert_eq!(value["geometry_source"], GEOMETRY_SOURCE_SYNTHETIC);
        assert_eq!(value["viewport"]["width"], viewport.width);
        assert_eq!(value["viewport"]["source"], "default");

        let toolbar = &value["toolbar"];
        let client = (
            0_i64,
            0_i64,
            i64::from(viewport.width),
            i64::from(viewport.height),
        );
        for key in [
            "new",
            "tabs",
            "control_center",
            "settings",
            "locale",
            "font_decrease",
            "font_increase",
        ] {
            let (left, top, right, bottom) = rect(&toolbar[key]);
            assert!(right > left && bottom > top, "{key} is degenerate");
            assert!(
                left >= client.0 && top >= client.1 && right <= client.2 && bottom <= client.3,
                "{key} escapes the nominal viewport"
            );
        }

        // Visibility describes the (absent) real client; bounds describe the
        // layout a window would get. Both must be present and must not be
        // confused with each other.
        assert_eq!(value["composer"]["visible"], false);
        let (left, top, right, bottom) = rect(&value["composer"]["input"]["bounds"]);
        assert!(right > left && bottom > top);
        assert!(top >= i64::from(layout.composer.top));
        assert!(bottom <= i64::from(layout.composer.bottom));

        let (track_left, _, track_right, _) = rect(&value["terminal"]["scrollbar"]["track"]);
        assert!(track_right - track_left == i64::from(TERMINAL_SCROLLBAR_WIDTH));
        assert_eq!(value["terminal"]["rows"], 30);
    }

    /// Two tab rows must occupy disjoint rectangles, otherwise a pointer aimed
    /// at the centre of one would route to the other and the whole exercise is
    /// worthless.
    #[test]
    fn synthetic_tab_rows_are_disjoint_and_expose_close_only_on_the_active_row() {
        let viewport = HeadlessViewport::resolve();
        let layout = viewport.layout(true, 250);
        let first = synthetic_tab_row_json(&layout, 0, 0, true);
        let second = synthetic_tab_row_json(&layout, 1, 1, false);

        let (_, first_top, _, first_bottom) = rect(&first["bounds"]);
        let (_, second_top, _, second_bottom) = rect(&second["bounds"]);
        assert!(first_bottom <= second_top, "rows overlap");
        assert!(first_bottom > first_top && second_bottom > second_top);
        assert!(second_bottom <= i64::from(layout.sidebar_tree.bottom));

        // Only the active row draws Add/Close, exactly as the GUI hosts do.
        let close = &first["actions"]["close"];
        assert_eq!(close["id"], "close-tab");
        let (close_left, close_top, close_right, close_bottom) = rect(&close["bounds"]);
        assert!(close_right > close_left && close_bottom > close_top);
        assert!(close_top >= first_top && close_bottom <= first_bottom);
        assert!(second["actions"].is_null());
    }

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
