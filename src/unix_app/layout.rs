use crate::{
    settings::AppConfig,
    ui_geometry::{
        PixelRect, TERMINAL_SCROLLBAR_WIDTH, TerminalScrollbarGeometry, WorkspaceLayout,
        WorkspaceLayoutInput, terminal_scrollbar_geometry, workspace_layout,
    },
};

pub(super) const UNIX_COMPOSER_HEIGHT: i32 = 64;
pub(super) const UNIX_STATUS_HEIGHT: i32 = 26;
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
    max_scrollback: usize,
) -> TerminalScrollbarGeometry {
    terminal_scrollbar_geometry(
        layout.terminal,
        visible_rows,
        scrollback_offset,
        max_scrollback,
    )
}

pub(super) fn u32_rect(rect: PixelRect) -> (u32, u32, u32, u32) {
    (
        rect.left.max(0) as u32,
        rect.top.max(0) as u32,
        rect.width().max(0) as u32,
        rect.height().max(0) as u32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::AppConfig;

    #[test]
    fn hidden_tabs_zero_sidebar_width() {
        let config = AppConfig {
            tabs_visible: false,
            ..AppConfig::default()
        };
        let layout = workspace_layout_for(800, 600, &config);
        assert_eq!(sidebar_width_u32(&layout), 0);
        assert_eq!(layout.effective_tabs_width, 0);
    }

    #[test]
    fn composer_layout_reserves_two_input_lines() {
        let layout = workspace_layout_for(800, 600, &AppConfig::default());
        assert_eq!(layout.composer.height(), UNIX_COMPOSER_HEIGHT);
        assert_eq!(UNIX_COMPOSER_HEIGHT, 64);
    }

    #[test]
    fn scrollbar_geometry_fits_terminal_column() {
        let config = AppConfig::default();
        let layout = workspace_layout_for(800, 600, &config);
        let geometry = scrollbar_geometry(&layout, 24, 0, 90);
        assert!(geometry.track.right <= layout.terminal.right);
        assert!(geometry.thumb.bottom <= geometry.track.bottom);
    }
}
