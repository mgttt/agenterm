pub(crate) const TAB_TOP: i32 = 8;
pub(crate) const TAB_HEIGHT: i32 = 44;
pub(crate) const TAB_LEFT: i32 = 5;
pub(crate) const TAB_RIGHT_MARGIN: i32 = 5;
pub(crate) const TREE_INDENT: i32 = 12;
pub(crate) const TREE_ANCHOR_LEFT: i32 = 12;
pub(crate) const TERMINAL_SCROLLBAR_WIDTH: i32 = 12;
pub(crate) const COMPOSER_HEIGHT: i32 = 104;
pub(crate) const TABS_MIN_WIDTH: i32 = 180;
pub(crate) const TABS_DEFAULT_WIDTH: i32 = 250;
pub(crate) const TABS_MAX_WIDTH: i32 = 480;
pub(crate) const TERMINAL_MIN_WIDTH: i32 = 320;
pub(crate) const TABS_RESIZE_GRIP_WIDTH: i32 = 6;
pub(crate) const SIDEBAR_TOOLBAR_HEIGHT: i32 = 46;

const MAX_TREE_DEPTH: usize = 10;
const TREE_MIN_RESPONSIVE_INDENT: i32 = 3;
const TREE_TEXT_GAP: i32 = 4;
const TREE_MIN_TEXT_WIDTH: i32 = 28;
const TREE_ACTION_GAP: i32 = 2;
const TREE_ACTION_INSET: i32 = 4;
const TREE_ACTION_TOP_INSET: i32 = 6;
const TREE_COMPACT_ACTION_WIDTH: i32 = 24;
const TREE_ADD_ACTION_WIDTH: i32 = 24;
const TREE_EDIT_ACTION_WIDTH: i32 = 38;
const TREE_CLOSE_ACTION_WIDTH: i32 = 42;
const TREE_SAVE_ACTION_WIDTH: i32 = 42;
const TREE_CANCEL_ACTION_WIDTH: i32 = 48;
const TREE_COMPACT_SAVE_ACTION_WIDTH: i32 = 34;
const TREE_COMPACT_CANCEL_ACTION_WIDTH: i32 = 40;
const TREE_COMPACT_ACTION_THRESHOLD: i32 = 220;
const NODE_Y_OFFSET: i32 = 13;
const MIN_SCROLLBAR_THUMB_HEIGHT: i32 = 24;
const SIDEBAR_TOOLBAR_DIVIDER_HEIGHT: i32 = 1;
const SIDEBAR_TOOLBAR_HORIZONTAL_PADDING: i32 = 8;
const SIDEBAR_TOOLBAR_BUTTON_GAP: i32 = 6;
const SIDEBAR_TOOLBAR_BUTTON_HEIGHT: i32 = 34;
const SIDEBAR_NEW_BUTTON_WIDTH: i32 = 66;
const SIDEBAR_TABS_BUTTON_WIDTH: i32 = 52;
const SIDEBAR_SETTINGS_BUTTON_WIDTH: i32 = 78;
const SIDEBAR_COMPACT_NEW_BUTTON_WIDTH: i32 = 64;
const SIDEBAR_COMPACT_ACTION_BUTTON_WIDTH: i32 = 36;
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
pub(crate) enum SidebarToolbarMode {
    Full,
    Compact,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SidebarToolbarLayout {
    pub(crate) bounds: PixelRect,
    pub(crate) divider: PixelRect,
    pub(crate) mode: SidebarToolbarMode,
    pub(crate) new_tab: PixelRect,
    pub(crate) tabs: PixelRect,
    pub(crate) settings: PixelRect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WorkspaceLayout {
    pub(crate) client: PixelRect,
    pub(crate) tabs_visible: bool,
    pub(crate) configured_tabs_width: i32,
    pub(crate) effective_tabs_width: i32,
    pub(crate) sidebar: PixelRect,
    /// Tree-owned sidebar surface. It never extends underneath the resize grip
    /// or into the host-owned action toolbar.
    pub(crate) sidebar_tree: PixelRect,
    /// Host-owned New/Tabs/Settings action surface. It is absent when Tabs are
    /// hidden or the constrained sidebar cannot provide its minimum width.
    pub(crate) sidebar_toolbar: Option<SidebarToolbarLayout>,
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
    let (sidebar_tree, sidebar_toolbar) = sidebar_regions(sidebar, resize_grip, input.tabs_visible);

    WorkspaceLayout {
        client,
        tabs_visible: input.tabs_visible,
        configured_tabs_width,
        effective_tabs_width,
        sidebar,
        sidebar_tree,
        sidebar_toolbar,
        resize_grip,
        terminal,
        composer,
        status,
        status_segments: status_segment_layout(status, input.tabs_visible),
    }
}

fn sidebar_regions(
    sidebar: PixelRect,
    resize_grip: Option<PixelRect>,
    tabs_visible: bool,
) -> (PixelRect, Option<SidebarToolbarLayout>) {
    let content_right = resize_grip
        .map(|grip| grip.left)
        .unwrap_or(sidebar.right)
        .clamp(sidebar.left, sidebar.right);
    let content = rect(sidebar.left, sidebar.top, content_right, sidebar.bottom);
    if !tabs_visible
        || sidebar.width() < TABS_MIN_WIDTH
        || content.width() < compact_toolbar_required_width()
        || content.height() < SIDEBAR_TOOLBAR_HEIGHT
    {
        return (content, None);
    }

    let toolbar = rect(
        content.left,
        content.bottom - SIDEBAR_TOOLBAR_HEIGHT,
        content.right,
        content.bottom,
    );
    let tree = rect(content.left, content.top, content.right, toolbar.top);
    let divider = rect(
        toolbar.left,
        toolbar.top,
        toolbar.right,
        toolbar.top + SIDEBAR_TOOLBAR_DIVIDER_HEIGHT,
    );
    let mode = if toolbar.width() >= full_toolbar_required_width() {
        SidebarToolbarMode::Full
    } else {
        SidebarToolbarMode::Compact
    };
    let (new_width, tabs_width, settings_width) = match mode {
        SidebarToolbarMode::Full => (
            SIDEBAR_NEW_BUTTON_WIDTH,
            SIDEBAR_TABS_BUTTON_WIDTH,
            SIDEBAR_SETTINGS_BUTTON_WIDTH,
        ),
        SidebarToolbarMode::Compact => (
            SIDEBAR_COMPACT_NEW_BUTTON_WIDTH,
            SIDEBAR_COMPACT_ACTION_BUTTON_WIDTH,
            SIDEBAR_COMPACT_ACTION_BUTTON_WIDTH,
        ),
    };
    let button_top = toolbar.top + (toolbar.height() - SIDEBAR_TOOLBAR_BUTTON_HEIGHT) / 2;
    let button_bottom = button_top + SIDEBAR_TOOLBAR_BUTTON_HEIGHT;
    let new_tab = rect(
        toolbar.left + SIDEBAR_TOOLBAR_HORIZONTAL_PADDING,
        button_top,
        toolbar.left + SIDEBAR_TOOLBAR_HORIZONTAL_PADDING + new_width,
        button_bottom,
    );
    let settings = rect(
        toolbar.right - SIDEBAR_TOOLBAR_HORIZONTAL_PADDING - settings_width,
        button_top,
        toolbar.right - SIDEBAR_TOOLBAR_HORIZONTAL_PADDING,
        button_bottom,
    );
    let tabs = rect(
        settings.left - SIDEBAR_TOOLBAR_BUTTON_GAP - tabs_width,
        button_top,
        settings.left - SIDEBAR_TOOLBAR_BUTTON_GAP,
        button_bottom,
    );

    (
        tree,
        Some(SidebarToolbarLayout {
            bounds: toolbar,
            divider,
            mode,
            new_tab,
            tabs,
            settings,
        }),
    )
}

const fn full_toolbar_required_width() -> i32 {
    SIDEBAR_TOOLBAR_HORIZONTAL_PADDING * 2
        + SIDEBAR_NEW_BUTTON_WIDTH
        + SIDEBAR_TABS_BUTTON_WIDTH
        + SIDEBAR_SETTINGS_BUTTON_WIDTH
        + SIDEBAR_TOOLBAR_BUTTON_GAP * 2
}

const fn compact_toolbar_required_width() -> i32 {
    SIDEBAR_TOOLBAR_HORIZONTAL_PADDING * 2
        + SIDEBAR_COMPACT_NEW_BUTTON_WIDTH
        + SIDEBAR_COMPACT_ACTION_BUTTON_WIDTH * 2
        + SIDEBAR_TOOLBAR_BUTTON_GAP * 2
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
pub(crate) enum TreeRowMode {
    Normal,
    Editing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TreeRowActionDensity {
    Full,
    Compact,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TreeRowEditorGeometry {
    pub(crate) name: PixelRect,
    pub(crate) note: PixelRect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TreeRowActionGeometry {
    pub(crate) bounds: PixelRect,
    pub(crate) density: TreeRowActionDensity,
    /// The add-child action in normal mode. Editing mode omits it.
    pub(crate) add_child: Option<PixelRect>,
    /// Edit in normal mode, Save in editing mode.
    pub(crate) primary: PixelRect,
    /// Close in normal mode, Cancel in editing mode.
    pub(crate) secondary: PixelRect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TreeRowGeometry {
    pub(crate) mode: TreeRowMode,
    pub(crate) row: PixelRect,
    pub(crate) selection: PixelRect,
    pub(crate) node_x: i32,
    pub(crate) node_y: i32,
    pub(crate) expander: PixelRect,
    pub(crate) status: PixelRect,
    pub(crate) disclosure_hit: PixelRect,
    /// Complete two-line label surface, excluding the action cluster.
    pub(crate) text: PixelRect,
    pub(crate) name: PixelRect,
    pub(crate) note: PixelRect,
    /// Two native single-line edit overlays in editing mode.
    pub(crate) editors: Option<TreeRowEditorGeometry>,
    pub(crate) actions: TreeRowActionGeometry,
}

#[cfg(test)]
pub(crate) fn tree_anchor_x(depth: usize) -> i32 {
    TREE_ANCHOR_LEFT + depth as i32 * TREE_INDENT
}

/// Responsive tree anchor shared by row content and connector painting.
///
/// At the 180 px Tabs minimum width this keeps every supported depth distinct
/// while reserving one CJK glyph plus ellipsis and the compact action cluster.
pub(crate) fn tree_connector_x(depth: usize, sidebar_width: i32, mode: TreeRowMode) -> i32 {
    let depth = depth.min(MAX_TREE_DEPTH);
    TREE_ANCHOR_LEFT + depth as i32 * responsive_tree_indent(sidebar_width, mode)
}

pub(crate) fn tree_row_at_y(y: i32) -> Option<usize> {
    (y >= TAB_TOP).then_some(((y - TAB_TOP) / TAB_HEIGHT) as usize)
}

/// Compatibility geometry for the current host. New host code should use
/// `tree_row_geometry_for_mode` and switch connectors, painting, hit-testing,
/// native edit placement, and snapshots in one change.
#[cfg(test)]
pub(crate) fn tree_row_geometry(
    visual_position: usize,
    depth: usize,
    sidebar_width: i32,
) -> TreeRowGeometry {
    tree_row_geometry_impl(
        visual_position,
        tree_anchor_x(depth.min(MAX_TREE_DEPTH)),
        sidebar_width,
        TreeRowMode::Normal,
    )
}

pub(crate) fn tree_row_geometry_for_mode(
    visual_position: usize,
    depth: usize,
    sidebar_width: i32,
    mode: TreeRowMode,
) -> TreeRowGeometry {
    tree_row_geometry_impl(
        visual_position,
        tree_connector_x(depth, sidebar_width, mode),
        sidebar_width,
        mode,
    )
}

fn tree_row_geometry_impl(
    visual_position: usize,
    node_x: i32,
    sidebar_width: i32,
    mode: TreeRowMode,
) -> TreeRowGeometry {
    let top = TAB_TOP + visual_position as i32 * TAB_HEIGHT;
    let node_y = top + NODE_Y_OFFSET;
    let selection = PixelRect {
        left: TAB_LEFT,
        top,
        right: (sidebar_width - TAB_RIGHT_MARGIN).max(TAB_LEFT),
        bottom: top + TAB_HEIGHT - 1,
    };
    let actions = tree_row_actions(selection, mode);
    let text_left = (node_x + 20).clamp(selection.left, selection.right);
    let text_right = (actions.bounds.left - TREE_TEXT_GAP).clamp(text_left, selection.right);
    let text = rect(text_left, top + 3, text_right, selection.bottom - 3);
    let name = rect(text.left, text.top, text.right, (top + 21).min(text.bottom));
    let note = rect(
        text.left,
        (top + 22).min(text.bottom),
        text.right,
        text.bottom,
    );
    let editors = (mode == TreeRowMode::Editing).then_some(TreeRowEditorGeometry { name, note });
    TreeRowGeometry {
        mode,
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
        text,
        name,
        note,
        editors,
        actions,
    }
}

fn responsive_tree_indent(sidebar_width: i32, mode: TreeRowMode) -> i32 {
    let action_width = desired_tree_action_width(sidebar_width, mode);
    let available_anchor_span = (sidebar_width
        - TAB_RIGHT_MARGIN
        - TREE_ACTION_INSET
        - action_width
        - TREE_TEXT_GAP
        - TREE_MIN_TEXT_WIDTH
        - 20
        - TREE_ANCHOR_LEFT)
        .max(0);
    (available_anchor_span / MAX_TREE_DEPTH as i32).clamp(TREE_MIN_RESPONSIVE_INDENT, TREE_INDENT)
}

fn desired_tree_action_width(sidebar_width: i32, mode: TreeRowMode) -> i32 {
    let compact = sidebar_width < TREE_COMPACT_ACTION_THRESHOLD;
    match (mode, compact) {
        (TreeRowMode::Normal, true) => TREE_COMPACT_ACTION_WIDTH * 3 + TREE_ACTION_GAP * 2,
        (TreeRowMode::Normal, false) => {
            TREE_ADD_ACTION_WIDTH
                + TREE_EDIT_ACTION_WIDTH
                + TREE_CLOSE_ACTION_WIDTH
                + TREE_ACTION_GAP * 2
        }
        (TreeRowMode::Editing, true) => {
            TREE_COMPACT_SAVE_ACTION_WIDTH + TREE_COMPACT_CANCEL_ACTION_WIDTH + TREE_ACTION_GAP
        }
        (TreeRowMode::Editing, false) => {
            TREE_SAVE_ACTION_WIDTH + TREE_CANCEL_ACTION_WIDTH + TREE_ACTION_GAP
        }
    }
}

fn tree_row_actions(row: PixelRect, mode: TreeRowMode) -> TreeRowActionGeometry {
    let density = if row.right + TAB_RIGHT_MARGIN < TREE_COMPACT_ACTION_THRESHOLD {
        TreeRowActionDensity::Compact
    } else {
        TreeRowActionDensity::Full
    };
    let (desired_widths, action_count) = match (mode, density) {
        (TreeRowMode::Normal, TreeRowActionDensity::Full) => (
            [
                TREE_ADD_ACTION_WIDTH,
                TREE_EDIT_ACTION_WIDTH,
                TREE_CLOSE_ACTION_WIDTH,
            ],
            3_usize,
        ),
        (TreeRowMode::Normal, TreeRowActionDensity::Compact) => ([TREE_COMPACT_ACTION_WIDTH; 3], 3),
        (TreeRowMode::Editing, TreeRowActionDensity::Full) => {
            ([TREE_SAVE_ACTION_WIDTH, TREE_CANCEL_ACTION_WIDTH, 0], 2)
        }
        (TreeRowMode::Editing, TreeRowActionDensity::Compact) => (
            [
                TREE_COMPACT_SAVE_ACTION_WIDTH,
                TREE_COMPACT_CANCEL_ACTION_WIDTH,
                0,
            ],
            2,
        ),
    };
    let right = (row.right - TREE_ACTION_INSET).max(row.left);
    let available = (right - row.left).max(0);
    let gap_count = action_count.saturating_sub(1) as i32;
    let gap = TREE_ACTION_GAP.min(available / action_count as i32);
    let width_budget = (available - gap * gap_count).max(0);
    let desired_total: i32 = desired_widths[..action_count].iter().sum();
    let mut widths = [0; 3];
    for index in 0..action_count {
        widths[index] = if desired_total <= width_budget {
            desired_widths[index]
        } else if desired_total == 0 {
            0
        } else {
            (desired_widths[index] * width_budget) / desired_total
        };
    }
    let assigned: i32 = widths[..action_count].iter().sum();
    let mut cursor = right - assigned - gap * gap_count;
    let top = (row.top + TREE_ACTION_TOP_INSET).min(row.bottom);
    let bottom = (row.bottom - TREE_ACTION_TOP_INSET).max(top);
    let empty = rect(right, top, right, bottom);
    let mut rects = [empty; 3];
    for index in 0..action_count {
        let action = rect(cursor, top, cursor + widths[index], bottom);
        rects[index] = action;
        cursor = action.right + gap;
    }
    let bounds = rect(rects[0].left, top, rects[action_count - 1].right, bottom);
    match mode {
        TreeRowMode::Normal => TreeRowActionGeometry {
            bounds,
            density,
            add_child: Some(rects[0]),
            primary: rects[1],
            secondary: rects[2],
        },
        TreeRowMode::Editing => TreeRowActionGeometry {
            bounds,
            density,
            add_child: None,
            primary: rects[0],
            secondary: rects[1],
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
            composer_height: COMPOSER_HEIGHT,
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

    fn assert_toolbar_valid(layout: WorkspaceLayout, expected_mode: SidebarToolbarMode) {
        let toolbar = layout
            .sidebar_toolbar
            .expect("sidebar toolbar should be present");
        let grip = layout.resize_grip.expect("resize grip should be present");

        assert_eq!(toolbar.mode, expected_mode);
        assert_eq!(layout.sidebar_tree.bottom, toolbar.bounds.top);
        assert_eq!(toolbar.bounds.right, grip.left);
        assert_eq!(toolbar.divider.top, toolbar.bounds.top);
        assert_eq!(
            toolbar.divider.bottom,
            toolbar.bounds.top + SIDEBAR_TOOLBAR_DIVIDER_HEIGHT
        );
        for action in [toolbar.new_tab, toolbar.tabs, toolbar.settings] {
            assert_valid_rect(action, toolbar.bounds);
            assert_eq!(action.height(), SIDEBAR_TOOLBAR_BUTTON_HEIGHT);
            assert!(action.width() > 0);
            assert!(action.right <= grip.left);
        }
        assert!(toolbar.new_tab.right <= toolbar.tabs.left);
        assert_eq!(
            toolbar.tabs.right + SIDEBAR_TOOLBAR_BUTTON_GAP,
            toolbar.settings.left
        );
        assert!(toolbar.new_tab.bottom <= layout.status.top);
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
        assert_eq!(geometry.terminal, rect(250, 0, 1000, 570));
        assert_eq!(geometry.composer, rect(250, 570, 1000, 674));
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
        assert_eq!(geometry.sidebar_tree, rect(0, 0, 244, 628));
        assert_toolbar_valid(geometry, SidebarToolbarMode::Full);
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
        assert_eq!(geometry.sidebar_tree, geometry.sidebar);
        assert_eq!(geometry.sidebar_toolbar, None);
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
    fn sidebar_toolbar_uses_compact_and_full_modes_without_moving_workspace_surfaces() {
        let compact = layout(500, 300, true, 250);
        assert_eq!(compact.effective_tabs_width, 180);
        assert_eq!(compact.terminal, rect(180, 0, 500, 170));
        assert_eq!(compact.composer, rect(180, 170, 500, 274));
        assert_eq!(compact.status, rect(0, 274, 500, 300));
        assert_toolbar_valid(compact, SidebarToolbarMode::Compact);

        let default = layout(1000, 700, true, 250);
        assert_eq!(default.effective_tabs_width, 250);
        assert_toolbar_valid(default, SidebarToolbarMode::Full);

        let wide = layout(1000, 700, true, 480);
        assert_eq!(wide.effective_tabs_width, 480);
        assert_eq!(wide.terminal, rect(480, 0, 1000, 570));
        assert_eq!(wide.composer, rect(480, 570, 1000, 674));
        assert_eq!(wide.status, rect(0, 674, 1000, 700));
        assert_toolbar_valid(wide, SidebarToolbarMode::Full);
    }

    #[test]
    fn constrained_sidebar_keeps_tree_bounded_and_omits_unusable_toolbar() {
        let narrow = layout(400, 300, true, 250);

        assert_eq!(narrow.effective_tabs_width, 80);
        assert_eq!(narrow.resize_grip, Some(rect(74, 0, 80, 274)));
        assert_eq!(narrow.sidebar_tree, rect(0, 0, 74, 274));
        assert_eq!(narrow.sidebar_toolbar, None);
        assert_eq!(narrow.terminal, rect(80, 0, 400, 170));
        assert_eq!(narrow.composer, rect(80, 170, 400, 274));
        assert_eq!(narrow.status, rect(0, 274, 400, 300));
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
                geometry.sidebar_tree,
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
            if let Some(toolbar) = geometry.sidebar_toolbar {
                for candidate in [
                    toolbar.bounds,
                    toolbar.divider,
                    toolbar.new_tab,
                    toolbar.tabs,
                    toolbar.settings,
                ] {
                    assert_valid_rect(candidate, geometry.client);
                }
                let grip = geometry.resize_grip.expect("toolbar requires resize grip");
                assert!(toolbar.bounds.right <= grip.left);
                assert!(geometry.sidebar_tree.bottom <= toolbar.bounds.top);
                assert!(toolbar.new_tab.right <= toolbar.tabs.left);
                assert!(toolbar.tabs.right <= toolbar.settings.left);
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

        assert_eq!(rows[0].node_x, 12);
        assert_eq!(rows[1].node_x, 12 + 12);
        assert_eq!(rows[2].node_x, 12 + 12 * 2);
        assert_eq!(rows[1].row.top - rows[0].row.top, 44);
        assert_eq!(rows[2].row.top - rows[1].row.top, 44);
        assert_eq!(rows[0].expander.width(), 11);
        assert_eq!(rows[0].expander.height(), 11);
        assert_eq!(rows[0].mode, TreeRowMode::Normal);
        assert!(rows[0].editors.is_none());
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
    fn normal_row_partitions_text_and_three_actions_without_overlap() {
        let geometry = tree_row_geometry_for_mode(0, 1, 250, TreeRowMode::Normal);
        let add = geometry
            .actions
            .add_child
            .expect("normal mode has add-child");

        assert_eq!(geometry.actions.density, TreeRowActionDensity::Full);
        assert!(geometry.text.right <= geometry.actions.bounds.left);
        assert!(add.right <= geometry.actions.primary.left);
        assert!(geometry.actions.primary.right <= geometry.actions.secondary.left);
        assert_eq!(add.width(), TREE_ADD_ACTION_WIDTH);
        assert_eq!(geometry.actions.primary.width(), TREE_EDIT_ACTION_WIDTH);
        assert_eq!(geometry.actions.secondary.width(), TREE_CLOSE_ACTION_WIDTH);
        assert_eq!(geometry.name.left, geometry.note.left);
        assert_eq!(geometry.name.right, geometry.note.right);
        assert!(geometry.name.bottom <= geometry.note.top);
        assert!(geometry.editors.is_none());
    }

    #[test]
    fn editing_row_replaces_actions_and_exposes_two_inline_editors() {
        let geometry = tree_row_geometry_for_mode(3, 2, 250, TreeRowMode::Editing);
        let editors = geometry.editors.expect("editing mode has two editors");

        assert_eq!(geometry.mode, TreeRowMode::Editing);
        assert_eq!(geometry.actions.density, TreeRowActionDensity::Full);
        assert_eq!(geometry.actions.add_child, None);
        assert_eq!(geometry.actions.primary.width(), TREE_SAVE_ACTION_WIDTH);
        assert_eq!(geometry.actions.secondary.width(), TREE_CANCEL_ACTION_WIDTH);
        assert!(geometry.actions.primary.right <= geometry.actions.secondary.left);
        assert_eq!(editors.name, geometry.name);
        assert_eq!(editors.note, geometry.note);
        assert_eq!(geometry.text.left, geometry.name.left);
        assert_eq!(geometry.text.right, geometry.note.right);
        assert!(geometry.text.right <= geometry.actions.bounds.left);
    }

    #[test]
    fn minimum_tabs_width_keeps_deep_cjk_text_and_compact_actions_bounded() {
        for mode in [TreeRowMode::Normal, TreeRowMode::Editing] {
            let geometry = tree_row_geometry_for_mode(0, MAX_TREE_DEPTH, 180, mode);

            assert_eq!(geometry.actions.density, TreeRowActionDensity::Compact);
            assert!(geometry.text.width() >= TREE_MIN_TEXT_WIDTH);
            assert!(geometry.text.right <= geometry.actions.bounds.left);
            assert!(geometry.actions.bounds.right <= geometry.selection.right);
            assert!(geometry.node_x < geometry.text.left);
            assert_eq!(geometry.node_x, tree_connector_x(MAX_TREE_DEPTH, 180, mode));
            if let Some(add) = geometry.actions.add_child {
                assert!(add.width() >= 20);
                assert!(add.right <= geometry.actions.primary.left);
            }
            assert!(geometry.actions.primary.width() >= 20);
            assert!(geometry.actions.secondary.width() >= 20);
            assert!(geometry.actions.primary.right <= geometry.actions.secondary.left);
        }
    }

    #[test]
    fn responsive_connector_grid_uses_one_indent_for_every_depth() {
        for (width, mode) in [
            (180, TreeRowMode::Normal),
            (180, TreeRowMode::Editing),
            (250, TreeRowMode::Normal),
            (480, TreeRowMode::Normal),
        ] {
            let anchors: Vec<i32> = (0..=MAX_TREE_DEPTH)
                .map(|depth| tree_connector_x(depth, width, mode))
                .collect();
            let indent = anchors[1] - anchors[0];

            assert!((TREE_MIN_RESPONSIVE_INDENT..=TREE_INDENT).contains(&indent));
            for pair in anchors.windows(2) {
                assert_eq!(pair[1] - pair[0], indent);
            }
        }
        assert!(
            tree_connector_x(MAX_TREE_DEPTH, 180, TreeRowMode::Normal)
                < tree_connector_x(MAX_TREE_DEPTH, 480, TreeRowMode::Normal)
        );
    }

    #[test]
    fn degenerate_row_geometry_collapses_safely_without_inverted_rectangles() {
        for width in [0, 5, 20, 80] {
            for mode in [TreeRowMode::Normal, TreeRowMode::Editing] {
                let geometry = tree_row_geometry_for_mode(0, 10, width, mode);
                for candidate in [
                    geometry.row,
                    geometry.text,
                    geometry.name,
                    geometry.note,
                    geometry.actions.bounds,
                    geometry.actions.primary,
                    geometry.actions.secondary,
                ] {
                    assert!(candidate.width() >= 0, "{candidate:?}");
                    assert!(candidate.height() >= 0, "{candidate:?}");
                }
                if let Some(add) = geometry.actions.add_child {
                    assert!(add.width() >= 0);
                    assert!(add.right <= geometry.actions.primary.left);
                }
                assert!(geometry.actions.primary.right <= geometry.actions.secondary.left);
            }
        }
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
