use crate::{
    SCROLLBACK_LINES,
    settings::AppConfig,
    ui_geometry::{
        PixelRect, TERMINAL_SCROLLBAR_WIDTH, WorkspaceLayout, WorkspaceLayoutInput,
        terminal_scrollbar_geometry, workspace_layout,
    },
};

pub(super) const UNIX_COMPOSER_HEIGHT: i32 = 48;
pub(super) const UNIX_STATUS_HEIGHT: i32 = 0;
pub(super) const SCROLLBAR_WIDTH: u32 = TERMINAL_SCROLLBAR_WIDTH as u32;

pub(super) fn workspace_layout_for(
    client_width: u32,
    client_height: u32,
    config: &AppConfig,
) -> WorkspaceLayout {
    workspace_layout(WorkspaceLayoutInput {
        client_width: i32::try_from(client_width).unwrap_or(i32::MAX),
        client_height: i32::try_from(client_height).unwrap_or(i32::MAX),
        tabs_visible: config.tabs_visible,
        configured_tabs_width: i32::from(config.tabs_width),
        composer_height: UNIX_COMPOSER_HEIGHT,
        status_height: UNIX_STATUS_HEIGHT,
    })
}

pub(super) fn sidebar_width_u32(layout: &WorkspaceLayout) -> u32 {
    layout.sidebar.width().max(0) as u32
}

pub(super) fn terminal_pixel_rect(layout: &WorkspaceLayout) -> PixelRect {
    layout.terminal
}

pub(super) fn composer_pixel_rect(layout: &WorkspaceLayout) -> PixelRect {
    layout.composer
}

pub(super) fn scrollbar_geometry(
    layout: &WorkspaceLayout,
    visible_rows: usize,
    scrollback_offset: usize,
) -> crate::ui_geometry::TerminalScrollbarGeometry {
    terminal_scrollbar_geometry(
        layout.terminal,
        visible_rows,
        scrollback_offset,
        SCROLLBACK_LINES,
    )
}

pub(super) fn pixel_rect_json(rect: PixelRect) -> serde_json::Value {
    serde_json::json!({
        "left": rect.left,
        "top": rect.top,
        "right": rect.right,
        "bottom": rect.bottom,
        "width": rect.width(),
        "height": rect.height(),
    })
}

pub(super) fn u32_rect(rect: PixelRect) -> (u32, u32, u32, u32) {
    (
        rect.left.max(0) as u32,
        rect.top.max(0) as u32,
        rect.width().max(0) as u32,
        rect.height().max(0) as u32,
    )
}
