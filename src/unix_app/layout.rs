use crate::{
    settings::AppConfig,
    ui_geometry::{
        PixelRect, TERMINAL_SCROLLBAR_WIDTH, TerminalScrollbarGeometry, WorkspaceLayout,
        WorkspaceLayoutInput, terminal_scrollbar_geometry, workspace_layout,
    },
};

pub(super) const UNIX_COMPOSER_HEIGHT: i32 = 48;
pub(super) const UNIX_STATUS_HEIGHT: i32 = 0;
pub(super) const SCROLLBAR_WIDTH: u32 = TERMINAL_SCROLLBAR_WIDTH as u32;
pub(super) const WHEEL_DELTA: i32 = 120;
pub(super) const WHEEL_ROWS_PER_NOTCH: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ScrollbarHit {
    Thumb,
    TrackAbove,
    TrackBelow,
}

pub(super) fn workspace_layout_for(client_width: u32, client_height: u32, config: &AppConfig) -> WorkspaceLayout {
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

pub(super) fn scrollbar_hit_test(
    geometry: &TerminalScrollbarGeometry,
    x: i32,
    y: i32,
) -> Option<ScrollbarHit> {
    if !geometry.track.contains(x, y) {
        return None;
    }
    if geometry.thumb.contains(x, y) {
        return Some(ScrollbarHit::Thumb);
    }
    if y < geometry.thumb.top {
        Some(ScrollbarHit::TrackAbove)
    } else {
        Some(ScrollbarHit::TrackBelow)
    }
}

/// Map a client pixel inside the terminal content (excluding scrollbar) to a cell.
pub(super) fn terminal_cell_at(
    terminal: PixelRect,
    x: i32,
    y: i32,
    rows: u16,
    cols: u16,
    cell_width: i32,
    cell_height: i32,
) -> Option<(u16, u16)> {
    let content_right = (terminal.right - TERMINAL_SCROLLBAR_WIDTH).max(terminal.left);
    if x < terminal.left || x >= content_right || y < terminal.top || y >= terminal.bottom {
        return None;
    }
    if rows == 0 || cols == 0 || cell_width <= 0 || cell_height <= 0 {
        return None;
    }
    let column = ((x - terminal.left) / cell_width).clamp(0, i32::from(cols.saturating_sub(1))) as u16;
    let row = ((y - terminal.top) / cell_height).clamp(0, i32::from(rows.saturating_sub(1))) as u16;
    Some((column, row))
}

/// Convert a winit wheel notch/pixel delta into Win32-style `WHEEL_DELTA` units.
pub(super) fn wheel_delta_units(line_or_pixel_y: f64, is_line_delta: bool) -> i32 {
    if is_line_delta {
        (line_or_pixel_y * f64::from(WHEEL_DELTA)).round() as i32
    } else {
        line_or_pixel_y.round() as i32
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::AppConfig;

    #[test]
    fn hidden_tabs_zero_sidebar_width() {
        let mut config = AppConfig::default();
        config.tabs_visible = false;
        let layout = workspace_layout_for(800, 600, &config);
        assert_eq!(sidebar_width_u32(&layout), 0);
        assert_eq!(layout.effective_tabs_width, 0);
    }

    #[test]
    fn pixel_rect_json_includes_bounds_and_size() {
        let rect = PixelRect {
            left: 10,
            top: 20,
            right: 110,
            bottom: 220,
        };
        let json = pixel_rect_json(rect);
        assert_eq!(json["left"], 10);
        assert_eq!(json["width"], 100);
        assert_eq!(json["height"], 200);
    }

    #[test]
    fn scrollbar_geometry_fits_terminal_column() {
        let config = AppConfig::default();
        let layout = workspace_layout_for(800, 600, &config);
        let geometry = scrollbar_geometry(&layout, 24, 0, 90);
        assert!(geometry.track.right <= layout.terminal.right);
        assert!(geometry.thumb.bottom <= geometry.track.bottom);
    }

    #[test]
    fn scrollbar_hit_test_distinguishes_thumb_and_track() {
        let config = AppConfig::default();
        let layout = workspace_layout_for(800, 600, &config);
        let geometry = scrollbar_geometry(&layout, 24, 45, 90);
        let mid_x = (geometry.track.left + geometry.track.right) / 2;
        assert_eq!(
            scrollbar_hit_test(&geometry, mid_x, geometry.thumb.top + 1),
            Some(ScrollbarHit::Thumb)
        );
        assert_eq!(
            scrollbar_hit_test(&geometry, mid_x, geometry.track.top + 1),
            Some(ScrollbarHit::TrackAbove)
        );
        assert_eq!(
            scrollbar_hit_test(&geometry, mid_x, geometry.track.bottom - 1),
            Some(ScrollbarHit::TrackBelow)
        );
        assert_eq!(scrollbar_hit_test(&geometry, 0, geometry.thumb.top), None);
    }

    #[test]
    fn wheel_delta_units_accumulate_like_win32() {
        assert_eq!(wheel_delta_units(1.0, true), WHEEL_DELTA);
        assert_eq!(wheel_delta_units(-0.5, true), -WHEEL_DELTA / 2);
        assert_eq!(wheel_delta_units(40.0, false), 40);
    }

    #[test]
    fn terminal_cell_at_maps_pixels_inside_content() {
        let terminal = PixelRect {
            left: 200,
            top: 0,
            right: 800,
            bottom: 480,
        };
        assert_eq!(
            terminal_cell_at(terminal, 210, 8, 24, 80, 10, 16),
            Some((1, 0))
        );
        assert_eq!(
            terminal_cell_at(terminal, 790, 8, 24, 80, 10, 16),
            None,
            "scrollbar column is excluded"
        );
        assert_eq!(terminal_cell_at(terminal, 100, 8, 24, 80, 10, 16), None);
    }
}
