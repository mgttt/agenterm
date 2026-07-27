pub(crate) const TAB_TOP: i32 = 8;
pub(crate) const TAB_HEIGHT: i32 = 44;
pub(crate) const TAB_LEFT: i32 = 5;
pub(crate) const TAB_RIGHT_MARGIN: i32 = 5;
pub(crate) const TREE_INDENT: i32 = 16;
pub(crate) const TREE_ANCHOR_LEFT: i32 = 17;
pub(crate) const TERMINAL_SCROLLBAR_WIDTH: i32 = 12;
pub(crate) const TABS_MIN_WIDTH: i32 = 180;
pub(crate) const TABS_DEFAULT_WIDTH: i32 = 250;
pub(crate) const TABS_MAX_WIDTH: i32 = 480;
pub(crate) const TERMINAL_MIN_WIDTH: i32 = 320;
pub(crate) const TABS_RESIZE_GRIP_WIDTH: i32 = 6;

const MAX_TREE_DEPTH: usize = 10;
const NODE_Y_OFFSET: i32 = 13;
const MIN_SCROLLBAR_THUMB_HEIGHT: i32 = 24;
const STATUS_TABS_WIDTH: i32 = 72;
const STATUS_CWD_WIDTH: i32 = 260;
const STATUS_CWD_MIN_WIDTH: i32 = 80;
const STATUS_PROXY_WIDTH: i32 = 112;
const STATUS_PROXY_MIN_WIDTH: i32 = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PixelRect {
    pub(crate) left: i32,
    pub(crate) top: i32,
    pub(crate) right: i32,
    pub(crate) bottom: i32,
}

impl PixelRect {
    pub(crate) fn width(self) -> i32 {
        self.right - self.left
    }

    pub(crate) fn height(self) -> i32 {
        self.bottom - self.top
    }

    pub(crate) fn contains_x(self, x: i32) -> bool {
        (self.left..self.right).contains(&x)
    }

    pub(crate) fn contains(self, x: i32, y: i32) -> bool {
        self.contains_x(x) && (self.top..self.bottom).contains(&y)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WorkspaceLayoutInput {
    pub(crate) client_width: i32,
    pub(crate) client_height: i32,
    pub(crate) tabs_visible: bool,
    /// The persisted preference. Window-size constraints are applied only to
    /// `WorkspaceLayout::effective_tabs_width`.
    pub(crate) configured_tabs_width: i32,
    pub(crate) composer_height: i32,
    pub(crate) status_height: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StatusSegmentLayout {
    /// Host-owned recovery control. It is present only while Tabs are hidden.
    pub(crate) tabs_recovery: Option<PixelRect>,
    pub(crate) cwd: PixelRect,
    /// Flexible space reserved for future bounded status providers.
    pub(crate) provider: PixelRect,
    pub(crate) proxy: PixelRect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WorkspaceLayout {
    pub(crate) client: PixelRect,
    pub(crate) tabs_visible: bool,
    pub(crate) configured_tabs_width: i32,
    pub(crate) effective_tabs_width: i32,
    pub(crate) sidebar: PixelRect,
    pub(crate) resize_grip: Option<PixelRect>,
    pub(crate) terminal: PixelRect,
    pub(crate) composer: PixelRect,
    pub(crate) status: PixelRect,
    pub(crate) status_segments: StatusSegmentLayout,
}

/// Clamp a persisted Tabs preference independently of the current window.
pub(crate) fn clamp_configured_tabs_width(width: i32) -> i32 {
    width.clamp(TABS_MIN_WIDTH, TABS_MAX_WIDTH)
}

/// Convert a resize-grip pointer position into a valid persisted preference.
///
/// On windows narrower than the proposed minimum plus the terminal floor, the
/// minimum preference is retained while `workspace_layout` temporarily reduces
/// only the effective width.
pub(crate) fn tabs_width_from_drag(pointer_x: i32, client_width: i32) -> i32 {
    let available = client_width
        .max(0)
        .saturating_sub(TERMINAL_MIN_WIDTH)
        .clamp(0, TABS_MAX_WIDTH);
    let upper = available.max(TABS_MIN_WIDTH);
    pointer_x.clamp(TABS_MIN_WIDTH, upper)
}

pub(crate) fn reset_tabs_width() -> i32 {
    TABS_DEFAULT_WIDTH
}

pub(crate) fn workspace_layout(input: WorkspaceLayoutInput) -> WorkspaceLayout {
    let width = input.client_width.max(0);
    let height = input.client_height.max(0);
    let configured_tabs_width = clamp_configured_tabs_width(input.configured_tabs_width);
    let effective_tabs_width = if input.tabs_visible {
        configured_tabs_width.min(width.saturating_sub(TERMINAL_MIN_WIDTH).max(0))
    } else {
        0
    };

    let status_height = input.status_height.max(0).min(height);
    let status_top = height - status_height;
    let composer_height = input.composer_height.max(0).min(status_top);
    let composer_top = status_top - composer_height;
    let content_left = effective_tabs_width;

    let client = rect(0, 0, width, height);
    let sidebar = rect(0, 0, effective_tabs_width, status_top);
    let terminal = rect(content_left, 0, width, composer_top);
    let composer = rect(content_left, composer_top, width, status_top);
    let status = rect(0, status_top, width, height);
    let resize_grip = input
        .tabs_visible
        .then(|| {
            let left = (effective_tabs_width - TABS_RESIZE_GRIP_WIDTH).clamp(0, width);
            rect(left, 0, effective_tabs_width.min(width), status_top)
        })
        .filter(|grip| grip.width() > 0);

    WorkspaceLayout {
        client,
        tabs_visible: input.tabs_visible,
        configured_tabs_width,
        effective_tabs_width,
        sidebar,
        resize_grip,
        terminal,
        composer,
        status,
        status_segments: status_segment_layout(status, input.tabs_visible),
    }
}

fn status_segment_layout(status: PixelRect, tabs_visible: bool) -> StatusSegmentLayout {
    let mut left = status.left;
    let right = status.right;
    let tabs_recovery = (!tabs_visible).then(|| {
        let segment_right = (left + STATUS_TABS_WIDTH).min(right);
        let segment = rect(left, status.top, segment_right, status.bottom);
        left = segment_right;
        segment
    });

    let remaining = (right - left).max(0);
    // Provider space disappears first. Proxy then compacts to its minimum,
    // followed by CWD truncation. At extremely small widths the host-owned Tabs
    // recovery (when present) and Proxy remain the highest-priority controls.
    let proxy_width = if remaining >= STATUS_CWD_MIN_WIDTH + STATUS_PROXY_MIN_WIDTH {
        STATUS_PROXY_WIDTH
            .min(remaining - STATUS_CWD_MIN_WIDTH)
            .max(STATUS_PROXY_MIN_WIDTH)
    } else {
        STATUS_PROXY_MIN_WIDTH.min(remaining)
    };
    let proxy_left = right - proxy_width;
    let cwd_width = STATUS_CWD_WIDTH.min((proxy_left - left).max(0));
    let cwd_right = left + cwd_width;

    StatusSegmentLayout {
        tabs_recovery,
        cwd: rect(left, status.top, cwd_right, status.bottom),
        provider: rect(cwd_right, status.top, proxy_left, status.bottom),
        proxy: rect(proxy_left, status.top, right, status.bottom),
    }
}

fn rect(left: i32, top: i32, right: i32, bottom: i32) -> PixelRect {
    PixelRect {
        left,
        top,
        right: right.max(left),
        bottom: bottom.max(top),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TerminalScrollbarGeometry {
    pub(crate) track: PixelRect,
    pub(crate) thumb: PixelRect,
}

pub(crate) fn terminal_scrollbar_geometry(
    terminal: PixelRect,
    visible_rows: usize,
    scrollback_offset: usize,
    max_scrollback: usize,
) -> TerminalScrollbarGeometry {
    let track = PixelRect {
        left: (terminal.right - TERMINAL_SCROLLBAR_WIDTH).max(terminal.left),
        top: terminal.top,
        right: terminal.right,
        bottom: terminal.bottom,
    };
    let track_height = track.height().max(0);
    let total_rows = visible_rows.saturating_add(max_scrollback).max(1);
    let proportional_height =
        (i64::from(track_height) * visible_rows.max(1) as i64 / total_rows as i64) as i32;
    let thumb_height = if max_scrollback == 0 {
        track_height
    } else {
        proportional_height
            .max(MIN_SCROLLBAR_THUMB_HEIGHT)
            .min(track_height)
    };
    let travel = (track_height - thumb_height).max(0);
    let offset = scrollback_offset.min(max_scrollback);
    let distance_from_bottom = if max_scrollback == 0 {
        0
    } else {
        (offset as i64 * i64::from(travel) / max_scrollback as i64) as i32
    };
    let thumb_top = track.bottom - thumb_height - distance_from_bottom;
    TerminalScrollbarGeometry {
        track,
        thumb: PixelRect {
            left: track.left + 2,
            top: thumb_top,
            right: (track.right - 2).max(track.left + 2),
            bottom: thumb_top + thumb_height,
        },
    }
}

pub(crate) fn scrollback_for_thumb_top(
    geometry: TerminalScrollbarGeometry,
    thumb_top: i32,
    max_scrollback: usize,
) -> usize {
    let travel = geometry.track.height() - geometry.thumb.height();
    if max_scrollback == 0 || travel <= 0 {
        return 0;
    }
    let clamped_top = thumb_top.clamp(
        geometry.track.top,
        geometry.track.bottom - geometry.thumb.height(),
    );
    let distance_from_bottom = geometry.track.bottom - geometry.thumb.height() - clamped_top;
    ((i64::from(distance_from_bottom) * max_scrollback as i64 + i64::from(travel) / 2)
        / i64::from(travel)) as usize
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TreeRowGeometry {
    pub(crate) row: PixelRect,
    pub(crate) selection: PixelRect,
    pub(crate) node_x: i32,
    pub(crate) node_y: i32,
    pub(crate) expander: PixelRect,
    pub(crate) status: PixelRect,
    pub(crate) disclosure_hit: PixelRect,
}

pub(crate) fn tree_anchor_x(depth: usize) -> i32 {
    TREE_ANCHOR_LEFT + depth as i32 * TREE_INDENT
}

pub(crate) fn tree_row_at_y(y: i32) -> Option<usize> {
    (y >= TAB_TOP).then_some(((y - TAB_TOP) / TAB_HEIGHT) as usize)
}

pub(crate) fn tree_row_geometry(
    visual_position: usize,
    depth: usize,
    sidebar_width: i32,
) -> TreeRowGeometry {
    let top = TAB_TOP + visual_position as i32 * TAB_HEIGHT;
    let node_x = tree_anchor_x(depth.min(MAX_TREE_DEPTH));
    let node_y = top + NODE_Y_OFFSET;
    let selection = PixelRect {
        left: TAB_LEFT,
        top,
        right: (sidebar_width - TAB_RIGHT_MARGIN).max(TAB_LEFT),
        bottom: top + TAB_HEIGHT - 1,
    };
    TreeRowGeometry {
        row: selection,
        selection,
        node_x,
        node_y,
        expander: PixelRect {
            left: node_x - 5,
            top: node_y - 5,
            right: node_x + 6,
            bottom: node_y + 6,
        },
        status: PixelRect {
            left: node_x + 10,
            top: node_y - 4,
            right: node_x + 19,
            bottom: node_y + 5,
        },
        // Preserve the deliberately forgiving disclosure target used by the
        // original tree: it is wider than the visible 11x11 expander.
        disclosure_hit: PixelRect {
            left: node_x - 6,
            top,
            right: node_x + 12,
            bottom: top + TAB_HEIGHT,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(width: i32, height: i32, tabs_visible: bool, tabs_width: i32) -> WorkspaceLayout {
        workspace_layout(WorkspaceLayoutInput {
            client_width: width,
            client_height: height,
            tabs_visible,
            configured_tabs_width: tabs_width,
            composer_height: 78,
            status_height: 26,
        })
    }

    fn assert_valid_rect(rect: PixelRect, client: PixelRect) {
        assert!(rect.width() >= 0, "{rect:?}");
        assert!(rect.height() >= 0, "{rect:?}");
        assert!(rect.left >= client.left, "{rect:?}");
        assert!(rect.top >= client.top, "{rect:?}");
        assert!(rect.right <= client.right, "{rect:?}");
        assert!(rect.bottom <= client.bottom, "{rect:?}");
    }

    #[test]
    fn configured_tabs_width_has_stable_proposed_bounds_and_reset() {
        assert_eq!(clamp_configured_tabs_width(-1), TABS_MIN_WIDTH);
        assert_eq!(clamp_configured_tabs_width(180), 180);
        assert_eq!(clamp_configured_tabs_width(250), 250);
        assert_eq!(clamp_configured_tabs_width(480), 480);
        assert_eq!(clamp_configured_tabs_width(i32::MAX), TABS_MAX_WIDTH);
        assert_eq!(reset_tabs_width(), 250);
    }

    #[test]
    fn normal_workspace_partitions_sidebar_terminal_composer_and_status() {
        let geometry = layout(1000, 700, true, 250);

        assert_eq!(geometry.configured_tabs_width, 250);
        assert_eq!(geometry.effective_tabs_width, 250);
        assert_eq!(geometry.sidebar, rect(0, 0, 250, 674));
        assert_eq!(geometry.terminal, rect(250, 0, 1000, 596));
        assert_eq!(geometry.composer, rect(250, 596, 1000, 674));
        assert_eq!(geometry.status, rect(0, 674, 1000, 700));
        assert_eq!(geometry.resize_grip, Some(rect(244, 0, 250, 674)));
        assert_eq!(
            geometry.resize_grip.unwrap().right,
            geometry.terminal.left,
            "the full six-pixel grip stays outside the terminal viewport"
        );
        assert_eq!(geometry.status_segments.tabs_recovery, None);
        assert_eq!(geometry.status_segments.cwd.width(), STATUS_CWD_WIDTH);
        assert_eq!(geometry.status_segments.proxy.width(), STATUS_PROXY_WIDTH);
        assert_eq!(geometry.status_segments.provider.width(), 628);
    }

    #[test]
    fn hidden_tabs_release_width_without_discarding_configured_width() {
        let geometry = layout(1000, 700, false, 414);

        assert!(!geometry.tabs_visible);
        assert_eq!(geometry.configured_tabs_width, 414);
        assert_eq!(geometry.effective_tabs_width, 0);
        assert_eq!(geometry.sidebar.width(), 0);
        assert_eq!(geometry.terminal.left, 0);
        assert_eq!(geometry.composer.left, 0);
        assert_eq!(geometry.resize_grip, None);
        assert_eq!(
            geometry.status_segments.tabs_recovery,
            Some(rect(0, 674, STATUS_TABS_WIDTH, 700))
        );
        assert_eq!(geometry.status_segments.cwd.left, STATUS_TABS_WIDTH);
    }

    #[test]
    fn narrow_window_reduces_only_effective_width_and_preserves_terminal_floor() {
        let geometry = layout(500, 300, true, 400);

        assert_eq!(geometry.configured_tabs_width, 400);
        assert_eq!(geometry.effective_tabs_width, 180);
        assert_eq!(geometry.terminal.width(), TERMINAL_MIN_WIDTH);
        assert_eq!(geometry.composer.width(), TERMINAL_MIN_WIDTH);

        let very_narrow = layout(200, 80, true, 250);
        assert_eq!(very_narrow.configured_tabs_width, 250);
        assert_eq!(very_narrow.effective_tabs_width, 0);
        assert_eq!(very_narrow.terminal.width(), 200);
        assert_eq!(very_narrow.composer.height(), 54);
        assert_eq!(very_narrow.terminal.height(), 0);
    }

    #[test]
    fn status_segments_degrade_without_overlap_and_keep_hidden_tabs_recovery() {
        let hidden = layout(210, 100, false, 250);
        let segments = hidden.status_segments;

        assert_eq!(segments.tabs_recovery.unwrap().width(), STATUS_TABS_WIDTH);
        assert_eq!(segments.provider.width(), 0);
        assert_eq!(segments.proxy.width(), STATUS_PROXY_MIN_WIDTH);
        assert_eq!(segments.cwd.width(), 74);
        assert_eq!(segments.cwd.right, segments.provider.left);
        assert_eq!(segments.provider.right, segments.proxy.left);

        let tiny = layout(40, 100, false, 250);
        assert_eq!(tiny.status_segments.tabs_recovery.unwrap().width(), 40);
        assert_eq!(tiny.status_segments.cwd.width(), 0);
        assert_eq!(tiny.status_segments.provider.width(), 0);
        assert_eq!(tiny.status_segments.proxy.width(), 0);
    }

    #[test]
    fn drag_clamps_to_terminal_floor_and_narrow_windows_keep_minimum_preference() {
        assert_eq!(tabs_width_from_drag(100, 1000), TABS_MIN_WIDTH);
        assert_eq!(tabs_width_from_drag(350, 1000), 350);
        assert_eq!(tabs_width_from_drag(900, 1000), TABS_MAX_WIDTH);
        assert_eq!(tabs_width_from_drag(400, 600), 280);
        assert_eq!(tabs_width_from_drag(20, 400), TABS_MIN_WIDTH);
        assert_eq!(tabs_width_from_drag(i32::MAX, -1), TABS_MIN_WIDTH);
    }

    #[test]
    fn maximized_and_degenerate_layouts_keep_all_rectangles_bounded() {
        for (width, height, visible, configured) in [
            (2560, 1440, true, 480),
            (640, 480, true, 250),
            (319, 120, true, 250),
            (100, 20, false, 250),
            (0, 0, true, -500),
            (-10, -20, false, i32::MAX),
        ] {
            let geometry = layout(width, height, visible, configured);
            for candidate in [
                geometry.sidebar,
                geometry.terminal,
                geometry.composer,
                geometry.status,
                geometry.status_segments.cwd,
                geometry.status_segments.provider,
                geometry.status_segments.proxy,
            ] {
                assert_valid_rect(candidate, geometry.client);
            }
            if let Some(candidate) = geometry.resize_grip {
                assert_valid_rect(candidate, geometry.client);
            }
            if let Some(candidate) = geometry.status_segments.tabs_recovery {
                assert_valid_rect(candidate, geometry.client);
            }
            assert_eq!(geometry.sidebar.right, geometry.terminal.left);
            assert_eq!(geometry.terminal.bottom, geometry.composer.top);
            assert_eq!(geometry.composer.bottom, geometry.status.top);
        }
    }

    #[test]
    fn tree_grid_is_stable_across_depths_and_rows() {
        let rows = [
            tree_row_geometry(0, 0, 250),
            tree_row_geometry(1, 1, 250),
            tree_row_geometry(2, 2, 250),
        ];

        assert_eq!(rows[0].node_x, 17);
        assert_eq!(rows[1].node_x, 17 + 16);
        assert_eq!(rows[2].node_x, 17 + 16 * 2);
        assert_eq!(rows[1].row.top - rows[0].row.top, 44);
        assert_eq!(rows[2].row.top - rows[1].row.top, 44);
        assert_eq!(rows[0].expander.width(), 11);
        assert_eq!(rows[0].expander.height(), 11);
    }

    #[test]
    fn selection_matches_the_visible_row_bounds() {
        let geometry = tree_row_geometry(2, 1, 250);

        assert_eq!(geometry.row, geometry.selection);
        assert_eq!(
            geometry.selection,
            PixelRect {
                left: 5,
                top: 96,
                right: 245,
                bottom: 139,
            }
        );
        assert_eq!(geometry.selection.width(), 240);
        assert_eq!(geometry.selection.height(), 43);
    }

    #[test]
    fn selection_safely_collapses_for_an_extremely_narrow_sidebar() {
        for sidebar_width in [0, 5, TAB_LEFT + TAB_RIGHT_MARGIN - 1] {
            let geometry = tree_row_geometry(0, 0, sidebar_width);

            assert_eq!(geometry.selection.left, TAB_LEFT);
            assert_eq!(geometry.selection.right, TAB_LEFT);
            assert_eq!(geometry.selection.width(), 0);
            assert!(geometry.selection.height() >= 0);
        }
    }

    #[test]
    fn disclosure_hit_test_uses_the_shared_node_position() {
        let geometry = tree_row_geometry(0, 2, 250);

        assert!(geometry.disclosure_hit.contains_x(geometry.node_x));
        assert!(geometry.disclosure_hit.contains_x(geometry.node_x - 6));
        assert!(!geometry.disclosure_hit.contains_x(geometry.node_x + 12));
        assert_eq!(tree_row_at_y(7), None);
        assert_eq!(tree_row_at_y(8), Some(0));
        assert_eq!(tree_row_at_y(52), Some(1));
    }

    #[test]
    fn terminal_scrollbar_maps_bottom_history_and_drag_positions() {
        let terminal = PixelRect {
            left: 250,
            top: 0,
            right: 1000,
            bottom: 600,
        };
        let bottom = terminal_scrollbar_geometry(terminal, 30, 0, 90);
        let middle = terminal_scrollbar_geometry(terminal, 30, 45, 90);
        let top = terminal_scrollbar_geometry(terminal, 30, 90, 90);

        assert_eq!(bottom.track.left, 1000 - TERMINAL_SCROLLBAR_WIDTH);
        assert_eq!(bottom.thumb.bottom, bottom.track.bottom);
        assert!(middle.thumb.top < bottom.thumb.top);
        assert_eq!(top.thumb.top, top.track.top);
        assert_eq!(scrollback_for_thumb_top(middle, middle.thumb.top, 90), 45);
    }

    #[test]
    fn terminal_scrollbar_fills_track_without_history() {
        let geometry = terminal_scrollbar_geometry(
            PixelRect {
                left: 250,
                top: 0,
                right: 1000,
                bottom: 600,
            },
            30,
            0,
            0,
        );

        assert_eq!(geometry.thumb.top, geometry.track.top);
        assert_eq!(geometry.thumb.bottom, geometry.track.bottom);
        assert_eq!(scrollback_for_thumb_top(geometry, 100, 0), 0);
    }
}
