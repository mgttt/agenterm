pub(crate) const TAB_TOP: i32 = 8;
pub(crate) const TAB_HEIGHT: i32 = 44;
pub(crate) const TAB_LEFT: i32 = 5;
pub(crate) const TAB_RIGHT_MARGIN: i32 = 5;
pub(crate) const TREE_INDENT: i32 = 16;
pub(crate) const TREE_ANCHOR_LEFT: i32 = 17;

const MAX_TREE_DEPTH: usize = 10;
const NODE_Y_OFFSET: i32 = 13;

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
}
