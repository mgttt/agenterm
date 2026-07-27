pub(crate) const TAB_TOP: i32 = 8;
pub(crate) const TAB_HEIGHT: i32 = 44;
pub(crate) const TAB_LEFT: i32 = 5;
pub(crate) const TAB_RIGHT_MARGIN: i32 = 5;
pub(crate) const TREE_INDENT: i32 = 16;
pub(crate) const TREE_ANCHOR_LEFT: i32 = 17;
pub(crate) const TERMINAL_SCROLLBAR_WIDTH: i32 = 12;

const MAX_TREE_DEPTH: usize = 10;
const NODE_Y_OFFSET: i32 = 13;
const MIN_SCROLLBAR_THUMB_HEIGHT: i32 = 24;

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
        right: sidebar_width - TAB_RIGHT_MARGIN,
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
