pub(crate) const TAB_TOP: i32 = 3;
pub(crate) const TAB_HEIGHT: i32 = 36;
pub(crate) const TAB_LEFT: i32 = 2;
pub(crate) const TAB_RIGHT_MARGIN: i32 = 2;
pub(crate) const TREE_INDENT: i32 = 10;
pub(crate) const TREE_ANCHOR_LEFT: i32 = 8;
pub(crate) const TERMINAL_SCROLLBAR_WIDTH: i32 = 12;
#[cfg(test)]
pub(crate) const COMPOSER_HEIGHT: i32 = 104;
pub(crate) const TABS_MIN_WIDTH: i32 = 180;
pub(crate) const TABS_DEFAULT_WIDTH: i32 = 250;
pub(crate) const TABS_MAX_WIDTH: i32 = 480;
pub(crate) const TERMINAL_MIN_WIDTH: i32 = 320;
pub(crate) const TABS_RESIZE_GRIP_WIDTH: i32 = 6;
pub(crate) const WORKSPACE_TOOLBAR_HEIGHT: i32 = 46;

const MAX_TREE_DEPTH: usize = 10;
const TREE_MIN_RESPONSIVE_INDENT: i32 = 3;
const TREE_TEXT_GAP: i32 = 2;
const TREE_MIN_TEXT_WIDTH: i32 = 28;
const TREE_ACTION_GAP: i32 = 2;
const TREE_ACTION_INSET: i32 = 2;
const TREE_ACTION_TOP_INSET: i32 = 3;
const TREE_COMPACT_ADD_ACTION_WIDTH: i32 = 24;
const TREE_COMPACT_CLOSE_ACTION_WIDTH: i32 = 24;
const TREE_ADD_ACTION_WIDTH: i32 = 24;
const TREE_CLOSE_ACTION_WIDTH: i32 = 62;
const TREE_SAVE_ACTION_WIDTH: i32 = 42;
const TREE_CANCEL_ACTION_WIDTH: i32 = 48;
const TREE_COMPACT_SAVE_ACTION_WIDTH: i32 = 34;
const TREE_COMPACT_CANCEL_ACTION_WIDTH: i32 = 40;
const TREE_COMPACT_ACTION_THRESHOLD: i32 = 300;
const NODE_Y_OFFSET: i32 = 11;
const MIN_SCROLLBAR_THUMB_HEIGHT: i32 = 24;
const WORKSPACE_TOOLBAR_DIVIDER_HEIGHT: i32 = 1;
const WORKSPACE_TOOLBAR_HORIZONTAL_PADDING: i32 = 4;
const WORKSPACE_TOOLBAR_BUTTON_GAP: i32 = 4;
const WORKSPACE_TOOLBAR_BUTTON_HEIGHT: i32 = 34;
const WORKSPACE_NEW_BUTTON_WIDTH: i32 = 66;
const WORKSPACE_TABS_BUTTON_WIDTH: i32 = 52;
const WORKSPACE_CONTROL_CENTER_BUTTON_WIDTH: i32 = 120;
const WORKSPACE_SETTINGS_BUTTON_WIDTH: i32 = 78;
const WORKSPACE_LOCALE_BUTTON_WIDTH: i32 = 58;
const WORKSPACE_FONT_BUTTON_WIDTH: i32 = 34;
const WORKSPACE_COMPACT_NEW_BUTTON_WIDTH: i32 = 32;
const WORKSPACE_COMPACT_ACTION_BUTTON_WIDTH: i32 = 32;
const WORKSPACE_COMPACT_CONTROL_CENTER_BUTTON_WIDTH: i32 = 40;
const WORKSPACE_COMPACT_LOCALE_BUTTON_WIDTH: i32 = 36;
const WORKSPACE_COMPACT_FONT_BUTTON_WIDTH: i32 = 22;
const STATUS_TABS_WIDTH: i32 = 72;
const STATUS_CWD_WIDTH: i32 = 260;

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
pub(crate) enum WorkspaceToolbarMode {
    Full,
    Compact,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WorkspaceToolbarLayout {
    pub(crate) bounds: PixelRect,
    pub(crate) divider: PixelRect,
    pub(crate) mode: WorkspaceToolbarMode,
    pub(crate) new_tab: PixelRect,
    pub(crate) tabs: PixelRect,
    /// Visually centered on the terminal workbench column, independent of the
    /// left and right action groups.
    pub(crate) control_center: PixelRect,
    pub(crate) settings: PixelRect,
    pub(crate) locale: PixelRect,
    pub(crate) font_decrease: PixelRect,
    pub(crate) font_increase: PixelRect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WorkspaceLayout {
    pub(crate) client: PixelRect,
    pub(crate) tabs_visible: bool,
    pub(crate) configured_tabs_width: i32,
    pub(crate) effective_tabs_width: i32,
    pub(crate) sidebar: PixelRect,
    /// Tree-owned sidebar surface. It spans the full client height and never
    /// extends underneath the resize grip.
    pub(crate) sidebar_tree: PixelRect,
    /// Host-owned New/Tabs/Settings action surface above the terminal. It
    /// remains available when Tabs are hidden so the sidebar can be restored.
    pub(crate) workspace_toolbar: Option<WorkspaceToolbarLayout>,
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

    let toolbar_height = WORKSPACE_TOOLBAR_HEIGHT.min(height);
    let status_height = input
        .status_height
        .max(0)
        .min(height.saturating_sub(toolbar_height));
    let status_top = height - status_height;
    let composer_height = input
        .composer_height
        .max(0)
        .min(status_top.saturating_sub(toolbar_height));
    let composer_top = status_top - composer_height;
    let content_left = effective_tabs_width;

    let client = rect(0, 0, width, height);
    let sidebar = rect(0, 0, effective_tabs_width, height);
    let terminal = rect(content_left, toolbar_height, width, composer_top);
    let composer = rect(content_left, composer_top, width, status_top);
    let status = rect(content_left, status_top, width, height);
    let resize_grip = input
        .tabs_visible
        .then(|| {
            let left = (effective_tabs_width - TABS_RESIZE_GRIP_WIDTH).clamp(0, width);
            rect(left, 0, effective_tabs_width.min(width), height)
        })
        .filter(|grip| grip.width() > 0);
    let sidebar_tree = rect(
        sidebar.left,
        sidebar.top,
        resize_grip.map(|grip| grip.left).unwrap_or(sidebar.right),
        sidebar.bottom,
    );
    let workspace_toolbar = workspace_toolbar_region(
        rect(content_left, 0, width, toolbar_height),
        toolbar_height > 0,
    );

    WorkspaceLayout {
        client,
        tabs_visible: input.tabs_visible,
        configured_tabs_width,
        effective_tabs_width,
        sidebar,
        sidebar_tree,
        workspace_toolbar,
        resize_grip,
        terminal,
        composer,
        status,
        status_segments: status_segment_layout(status, input.tabs_visible),
    }
}

fn workspace_toolbar_region(toolbar: PixelRect, visible: bool) -> Option<WorkspaceToolbarLayout> {
    if !visible
        || toolbar.width() < compact_toolbar_required_width()
        || toolbar.height() < WORKSPACE_TOOLBAR_HEIGHT
    {
        return None;
    }

    let divider = rect(
        toolbar.left,
        toolbar.bottom - WORKSPACE_TOOLBAR_DIVIDER_HEIGHT,
        toolbar.right,
        toolbar.bottom,
    );
    let mode = if toolbar.width() >= full_toolbar_required_width() {
        WorkspaceToolbarMode::Full
    } else {
        WorkspaceToolbarMode::Compact
    };
    let (new_width, tabs_width, control_center_width, settings_width, locale_width, font_width) =
        match mode {
            WorkspaceToolbarMode::Full => (
                WORKSPACE_NEW_BUTTON_WIDTH,
                WORKSPACE_TABS_BUTTON_WIDTH,
                WORKSPACE_CONTROL_CENTER_BUTTON_WIDTH,
                WORKSPACE_SETTINGS_BUTTON_WIDTH,
                WORKSPACE_LOCALE_BUTTON_WIDTH,
                WORKSPACE_FONT_BUTTON_WIDTH,
            ),
            WorkspaceToolbarMode::Compact => (
                WORKSPACE_COMPACT_NEW_BUTTON_WIDTH,
                WORKSPACE_COMPACT_ACTION_BUTTON_WIDTH,
                WORKSPACE_COMPACT_CONTROL_CENTER_BUTTON_WIDTH,
                WORKSPACE_COMPACT_ACTION_BUTTON_WIDTH,
                WORKSPACE_COMPACT_LOCALE_BUTTON_WIDTH,
                WORKSPACE_COMPACT_FONT_BUTTON_WIDTH,
            ),
        };
    let button_top = toolbar.top + (toolbar.height() - WORKSPACE_TOOLBAR_BUTTON_HEIGHT) / 2;
    let button_bottom = button_top + WORKSPACE_TOOLBAR_BUTTON_HEIGHT;
    let tabs = rect(
        toolbar.left + WORKSPACE_TOOLBAR_HORIZONTAL_PADDING,
        button_top,
        toolbar.left + WORKSPACE_TOOLBAR_HORIZONTAL_PADDING + tabs_width,
        button_bottom,
    );
    let new_tab = rect(
        tabs.right + WORKSPACE_TOOLBAR_BUTTON_GAP,
        button_top,
        tabs.right + WORKSPACE_TOOLBAR_BUTTON_GAP + new_width,
        button_bottom,
    );
    let toolbar_center = toolbar.left + toolbar.width() / 2;
    let control_center = rect(
        toolbar_center - control_center_width / 2,
        button_top,
        toolbar_center - control_center_width / 2 + control_center_width,
        button_bottom,
    );
    let font_increase = rect(
        toolbar.right - WORKSPACE_TOOLBAR_HORIZONTAL_PADDING - font_width,
        button_top,
        toolbar.right - WORKSPACE_TOOLBAR_HORIZONTAL_PADDING,
        button_bottom,
    );
    let font_decrease = rect(
        font_increase.left - font_width,
        button_top,
        font_increase.left,
        button_bottom,
    );
    let locale = rect(
        font_decrease.left - WORKSPACE_TOOLBAR_BUTTON_GAP - locale_width,
        button_top,
        font_decrease.left - WORKSPACE_TOOLBAR_BUTTON_GAP,
        button_bottom,
    );
    let settings = rect(
        locale.left - WORKSPACE_TOOLBAR_BUTTON_GAP - settings_width,
        button_top,
        locale.left - WORKSPACE_TOOLBAR_BUTTON_GAP,
        button_bottom,
    );

    Some(WorkspaceToolbarLayout {
        bounds: toolbar,
        divider,
        mode,
        new_tab,
        tabs,
        control_center,
        settings,
        locale,
        font_decrease,
        font_increase,
    })
}

const fn full_toolbar_required_width() -> i32 {
    // The center control is centered on the entire workbench column, while
    // Settings/locale/font remain right anchored. This is the first width at
    // which those independent groups retain one full inter-button gap.
    560
}

const fn compact_toolbar_required_width() -> i32 {
    296
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

    // The bottom-bar Proxy surface is archived. Users configure proxy state in
    // their shell; keep a zero-width geometry slot so old structured snapshot
    // readers degrade cleanly while the implementation remains easy to revive.
    //
    // Archived allocation:
    // let proxy_width = STATUS_PROXY_WIDTH.min(remaining);
    // let proxy_left = right - proxy_width;
    let cwd_width = STATUS_CWD_WIDTH.min((right - left).max(0));
    let cwd_right = left + cwd_width;

    StatusSegmentLayout {
        tabs_recovery,
        cwd: rect(left, status.top, cwd_right, status.bottom),
        provider: rect(cwd_right, status.top, right, status.bottom),
        proxy: rect(right, status.top, right, status.bottom),
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
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) enum ScrollbarHit {
    Thumb,
    TrackAbove,
    TrackBelow,
}

#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) const WHEEL_DELTA: i32 = 120;
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) const WHEEL_ROWS_PER_NOTCH: usize = 3;

#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) fn scrollbar_hit_test(
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
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) fn terminal_cell_at(
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
    let column =
        ((x - terminal.left) / cell_width).clamp(0, i32::from(cols.saturating_sub(1))) as u16;
    let row = ((y - terminal.top) / cell_height).clamp(0, i32::from(rows.saturating_sub(1))) as u16;
    Some((column, row))
}

/// Convert a wheel notch/pixel delta into Win32-style `WHEEL_DELTA` units.
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) fn wheel_delta_units(line_or_pixel_y: f64, is_line_delta: bool) -> i32 {
    if is_line_delta {
        (line_or_pixel_y * f64::from(WHEEL_DELTA)).round() as i32
    } else {
        line_or_pixel_y.round() as i32
    }
}

pub(crate) fn pixel_rect_json(rect: PixelRect) -> serde_json::Value {
    serde_json::json!({
        "left": rect.left,
        "top": rect.top,
        "right": rect.right,
        "bottom": rect.bottom,
        "width": rect.width(),
        "height": rect.height(),
    })
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
    /// Empty in normal mode, Save in editing mode.
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
    let text_left = (node_x + 17).clamp(selection.left, selection.right);
    let text_right = (actions.bounds.left - TREE_TEXT_GAP).clamp(text_left, selection.right);
    let text = rect(text_left, top + 1, text_right, selection.bottom - 1);
    let name = rect(text.left, text.top, text.right, (top + 18).min(text.bottom));
    let note = rect(
        text.left,
        (top + 18).min(text.bottom),
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
            left: node_x + 8,
            top: node_y - 4,
            right: node_x + 16,
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

#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) fn sidebar_scrollbar_track(sidebar_tree: PixelRect) -> PixelRect {
    PixelRect {
        left: sidebar_tree.left,
        top: sidebar_tree.top,
        right: (sidebar_tree.left + TERMINAL_SCROLLBAR_WIDTH).min(sidebar_tree.right),
        bottom: sidebar_tree.bottom,
    }
}

#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) fn sidebar_row_capacity(sidebar_tree_height: i32) -> usize {
    usize::try_from((sidebar_tree_height - TAB_TOP).max(0) / TAB_HEIGHT)
        .unwrap_or_default()
        .max(1)
}

#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) fn translate_tree_row_geometry(
    mut geometry: TreeRowGeometry,
    delta_x: i32,
) -> TreeRowGeometry {
    fn translate(mut rect: PixelRect, delta: i32) -> PixelRect {
        rect.left += delta;
        rect.right += delta;
        rect
    }

    geometry.row = translate(geometry.row, delta_x);
    geometry.selection = translate(geometry.selection, delta_x);
    geometry.node_x += delta_x;
    geometry.expander = translate(geometry.expander, delta_x);
    geometry.status = translate(geometry.status, delta_x);
    geometry.disclosure_hit = translate(geometry.disclosure_hit, delta_x);
    geometry.text = translate(geometry.text, delta_x);
    geometry.name = translate(geometry.name, delta_x);
    geometry.note = translate(geometry.note, delta_x);
    geometry.editors = geometry.editors.map(|mut editors| {
        editors.name = translate(editors.name, delta_x);
        editors.note = translate(editors.note, delta_x);
        editors
    });
    geometry.actions.bounds = translate(geometry.actions.bounds, delta_x);
    geometry.actions.add_child = geometry
        .actions
        .add_child
        .map(|bounds| translate(bounds, delta_x));
    geometry.actions.primary = translate(geometry.actions.primary, delta_x);
    geometry.actions.secondary = translate(geometry.actions.secondary, delta_x);
    geometry
}

#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) fn sidebar_tree_row_geometry(
    sidebar_tree: PixelRect,
    viewport_position: usize,
    depth: usize,
    mode: TreeRowMode,
) -> TreeRowGeometry {
    let content_left = (sidebar_tree.left + TERMINAL_SCROLLBAR_WIDTH).min(sidebar_tree.right);
    let content_width = (sidebar_tree.right - content_left).max(0);
    translate_tree_row_geometry(
        tree_row_geometry_for_mode(viewport_position, depth, content_width, mode),
        content_left,
    )
}

/// Return the one-pixel branch segments for one visible tree row.
///
/// Keeping this geometry host-independent prevents the Win32 and Unix
/// renderers from drifting: both hosts paint the same ancestor guides and
/// child elbow around the same responsive node anchors.
pub(crate) fn tree_connector_segments(
    sidebar_tree: PixelRect,
    geometry: &TreeRowGeometry,
    depth: usize,
    guides: &[bool],
    is_last: bool,
    mode: TreeRowMode,
) -> Vec<PixelRect> {
    let content_left = (sidebar_tree.left + TERMINAL_SCROLLBAR_WIDTH).min(sidebar_tree.right);
    let content_width = (sidebar_tree.right - content_left).max(0);
    let row_top = geometry.row.top.max(sidebar_tree.top);
    let row_bottom = geometry.row.bottom.min(sidebar_tree.bottom);
    if content_width <= 0 || row_bottom <= row_top {
        return Vec::new();
    }

    let mut segments = Vec::with_capacity(guides.len() + 2);
    for (guide_depth, &continues) in guides.iter().enumerate() {
        if continues {
            let x = content_left + tree_connector_x(guide_depth, content_width, mode);
            segments.push(PixelRect {
                left: x,
                top: row_top,
                right: x + 1,
                bottom: row_bottom,
            });
        }
    }

    if depth == 0 {
        return segments;
    }

    let parent_x = content_left + tree_connector_x(depth - 1, content_width, mode);
    let node_y = geometry.node_y.clamp(row_top, row_bottom - 1);
    let vertical_bottom = if is_last { node_y + 1 } else { row_bottom };
    if vertical_bottom > row_top {
        segments.push(PixelRect {
            left: parent_x,
            top: row_top,
            right: parent_x + 1,
            bottom: vertical_bottom,
        });
    }
    if geometry.node_x > parent_x + 1 {
        segments.push(PixelRect {
            left: parent_x,
            top: node_y,
            right: geometry.node_x,
            bottom: node_y + 1,
        });
    }
    segments
}

#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) fn sidebar_scrollbar_geometry(
    track: PixelRect,
    offset: usize,
    maximum: usize,
    row_capacity: usize,
    row_count: usize,
) -> TerminalScrollbarGeometry {
    let track_height = track.height().max(0);
    let total = row_count.max(1);
    let visible = row_capacity.min(total).max(1);
    let proportional = (i64::from(track_height) * visible as i64 / total as i64) as i32;
    let thumb_height = if maximum == 0 {
        track_height
    } else {
        proportional.max(24).min(track_height)
    };
    let travel = (track_height - thumb_height).max(0);
    let thumb_top = if maximum == 0 {
        track.top
    } else {
        track.top + (offset as i64 * i64::from(travel) / maximum as i64) as i32
    };
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
        (TreeRowMode::Normal, true) => {
            TREE_COMPACT_ADD_ACTION_WIDTH + TREE_COMPACT_CLOSE_ACTION_WIDTH + TREE_ACTION_GAP
        }
        (TreeRowMode::Normal, false) => {
            TREE_ADD_ACTION_WIDTH + TREE_CLOSE_ACTION_WIDTH + TREE_ACTION_GAP
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
        (TreeRowMode::Normal, TreeRowActionDensity::Full) => {
            ([TREE_ADD_ACTION_WIDTH, TREE_CLOSE_ACTION_WIDTH, 0], 2_usize)
        }
        (TreeRowMode::Normal, TreeRowActionDensity::Compact) => (
            [
                TREE_COMPACT_ADD_ACTION_WIDTH,
                TREE_COMPACT_CLOSE_ACTION_WIDTH,
                0,
            ],
            2,
        ),
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
            primary: rect(rects[0].right, top, rects[0].right, bottom),
            secondary: rects[1],
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

    fn assert_toolbar_valid(layout: WorkspaceLayout, expected_mode: WorkspaceToolbarMode) {
        let toolbar = layout
            .workspace_toolbar
            .expect("workspace toolbar should be present");

        assert_eq!(toolbar.mode, expected_mode);
        assert_eq!(toolbar.bounds.left, layout.terminal.left);
        assert_eq!(toolbar.bounds.right, layout.terminal.right);
        assert_eq!(toolbar.bounds.bottom, layout.terminal.top);
        assert_eq!(
            toolbar.divider.top,
            toolbar.bounds.bottom - WORKSPACE_TOOLBAR_DIVIDER_HEIGHT
        );
        assert_eq!(toolbar.divider.bottom, toolbar.bounds.bottom);
        for action in [
            toolbar.new_tab,
            toolbar.tabs,
            toolbar.control_center,
            toolbar.settings,
            toolbar.locale,
            toolbar.font_decrease,
            toolbar.font_increase,
        ] {
            assert_valid_rect(action, toolbar.bounds);
            assert_eq!(action.height(), WORKSPACE_TOOLBAR_BUTTON_HEIGHT);
            assert!(action.width() > 0);
        }
        assert!(toolbar.tabs.right <= toolbar.new_tab.left);
        assert!(toolbar.new_tab.right <= toolbar.control_center.left);
        assert!(toolbar.control_center.right <= toolbar.settings.left);
        assert!(toolbar.settings.right <= toolbar.locale.left);
        assert!(toolbar.locale.right <= toolbar.font_decrease.left);
        assert_eq!(toolbar.font_decrease.right, toolbar.font_increase.left);
        assert!(toolbar.font_increase.right <= toolbar.bounds.right);
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
        assert_eq!(geometry.sidebar, rect(0, 0, 250, 700));
        assert_eq!(geometry.terminal, rect(250, 46, 1000, 570));
        assert_eq!(geometry.composer, rect(250, 570, 1000, 674));
        assert_eq!(geometry.status, rect(250, 674, 1000, 700));
        assert_eq!(geometry.resize_grip, Some(rect(244, 0, 250, 700)));
        assert_eq!(
            geometry.resize_grip.unwrap().right,
            geometry.terminal.left,
            "the full six-pixel grip stays outside the terminal viewport"
        );
        assert_eq!(geometry.status_segments.tabs_recovery, None);
        assert_eq!(geometry.status_segments.cwd.width(), STATUS_CWD_WIDTH);
        assert_eq!(geometry.status_segments.proxy.width(), 0);
        assert_eq!(geometry.status_segments.provider.width(), 490);
        assert_eq!(geometry.sidebar_tree, rect(0, 0, 244, 700));
        assert_toolbar_valid(geometry, WorkspaceToolbarMode::Full);
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
        assert!(geometry.workspace_toolbar.is_some());
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
        assert_eq!(very_narrow.composer.height(), 8);
        assert_eq!(very_narrow.terminal.height(), 0);
    }

    #[test]
    fn workspace_toolbar_stays_above_terminal_independently_of_tabs_width() {
        let compact = layout(500, 300, true, 250);
        assert_eq!(compact.effective_tabs_width, 180);
        assert_eq!(compact.terminal, rect(180, 46, 500, 170));
        assert_eq!(compact.composer, rect(180, 170, 500, 274));
        assert_eq!(compact.status, rect(180, 274, 500, 300));
        assert_toolbar_valid(compact, WorkspaceToolbarMode::Compact);

        let default = layout(1000, 700, true, 250);
        assert_eq!(default.effective_tabs_width, 250);
        assert_toolbar_valid(default, WorkspaceToolbarMode::Full);

        let wide = layout(1000, 700, true, 480);
        assert_eq!(wide.effective_tabs_width, 480);
        assert_eq!(wide.terminal, rect(480, 46, 1000, 570));
        assert_eq!(wide.composer, rect(480, 570, 1000, 674));
        assert_eq!(wide.status, rect(480, 674, 1000, 700));
        assert_toolbar_valid(wide, WorkspaceToolbarMode::Compact);

        let very_narrow = layout(200, 300, false, 250);
        assert!(
            very_narrow.workspace_toolbar.is_none(),
            "a too-narrow workbench must omit unusably overlapping controls"
        );
    }

    #[test]
    fn constrained_sidebar_keeps_full_height_tree_and_right_workspace_surfaces() {
        let narrow = layout(400, 300, true, 250);

        assert_eq!(narrow.effective_tabs_width, 80);
        assert_eq!(narrow.resize_grip, Some(rect(74, 0, 80, 300)));
        assert_eq!(narrow.sidebar_tree, rect(0, 0, 74, 300));
        assert!(narrow.workspace_toolbar.is_some());
        assert_eq!(narrow.terminal, rect(80, 46, 400, 170));
        assert_eq!(narrow.composer, rect(80, 170, 400, 274));
        assert_eq!(narrow.status, rect(80, 274, 400, 300));
    }

    #[test]
    fn archived_proxy_releases_status_space_and_keeps_hidden_tabs_recovery() {
        let hidden = layout(210, 100, false, 250);
        let segments = hidden.status_segments;

        assert_eq!(segments.tabs_recovery.unwrap().width(), STATUS_TABS_WIDTH);
        assert_eq!(segments.provider.width(), 0);
        assert_eq!(segments.proxy.width(), 0);
        assert_eq!(segments.cwd.width(), 138);
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
            if let Some(toolbar) = geometry.workspace_toolbar {
                for candidate in [
                    toolbar.bounds,
                    toolbar.divider,
                    toolbar.new_tab,
                    toolbar.tabs,
                    toolbar.control_center,
                    toolbar.settings,
                    toolbar.locale,
                    toolbar.font_decrease,
                    toolbar.font_increase,
                ] {
                    assert_valid_rect(candidate, geometry.client);
                }
                assert!(toolbar.tabs.right <= toolbar.new_tab.left);
                assert!(toolbar.new_tab.right <= toolbar.control_center.left);
                assert!(toolbar.control_center.right <= toolbar.settings.left);
                assert!(toolbar.settings.right <= toolbar.locale.left);
                assert!(toolbar.locale.right <= toolbar.font_decrease.left);
                assert_eq!(toolbar.font_decrease.right, toolbar.font_increase.left);
            }
            if let Some(candidate) = geometry.status_segments.tabs_recovery {
                assert_valid_rect(candidate, geometry.client);
            }
            assert_eq!(geometry.sidebar.right, geometry.terminal.left);
            if let Some(toolbar) = geometry.workspace_toolbar {
                assert_eq!(toolbar.bounds.bottom, geometry.terminal.top);
            }
            assert_eq!(geometry.terminal.bottom, geometry.composer.top);
            assert_eq!(geometry.composer.bottom, geometry.status.top);
            assert_eq!(geometry.status.left, geometry.terminal.left);
        }
    }

    #[test]
    fn tree_grid_is_stable_across_depths_and_rows() {
        let rows = [
            tree_row_geometry(0, 0, 250),
            tree_row_geometry(1, 1, 250),
            tree_row_geometry(2, 2, 250),
        ];

        assert_eq!(rows[0].node_x, TREE_ANCHOR_LEFT);
        assert_eq!(rows[1].node_x, TREE_ANCHOR_LEFT + TREE_INDENT);
        assert_eq!(rows[2].node_x, TREE_ANCHOR_LEFT + TREE_INDENT * 2);
        assert_eq!(rows[1].row.top - rows[0].row.top, TAB_HEIGHT);
        assert_eq!(rows[2].row.top - rows[1].row.top, TAB_HEIGHT);
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
                left: TAB_LEFT,
                top: TAB_TOP + TAB_HEIGHT * 2,
                right: 250 - TAB_RIGHT_MARGIN,
                bottom: TAB_TOP + TAB_HEIGHT * 3 - 1,
            }
        );
        assert_eq!(
            geometry.selection.width(),
            250 - TAB_LEFT - TAB_RIGHT_MARGIN
        );
        assert_eq!(geometry.selection.height(), TAB_HEIGHT - 1);
    }

    #[test]
    fn selection_safely_collapses_for_an_extremely_narrow_sidebar() {
        for sidebar_width in [0, TAB_LEFT, TAB_LEFT + TAB_RIGHT_MARGIN - 1] {
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
        assert_eq!(tree_row_at_y(TAB_TOP - 1), None);
        assert_eq!(tree_row_at_y(TAB_TOP), Some(0));
        assert_eq!(tree_row_at_y(TAB_TOP + TAB_HEIGHT), Some(1));
    }

    #[test]
    fn normal_row_partitions_text_and_two_actions_without_overlap() {
        let geometry = tree_row_geometry_for_mode(0, 1, 360, TreeRowMode::Normal);
        let add = geometry
            .actions
            .add_child
            .expect("normal mode has add-child");

        assert_eq!(geometry.actions.density, TreeRowActionDensity::Full);
        assert!(geometry.text.right <= geometry.actions.bounds.left);
        assert!(add.right <= geometry.actions.primary.left);
        assert!(geometry.actions.primary.right <= geometry.actions.secondary.left);
        assert_eq!(add.width(), TREE_ADD_ACTION_WIDTH);
        assert_eq!(geometry.actions.primary.width(), 0);
        assert_eq!(geometry.actions.secondary.width(), TREE_CLOSE_ACTION_WIDTH);
        assert_eq!(geometry.name.left, geometry.note.left);
        assert_eq!(geometry.name.right, geometry.note.right);
        assert!(geometry.name.bottom <= geometry.note.top);
        assert!(geometry.editors.is_none());
    }

    #[test]
    fn editing_row_replaces_actions_and_exposes_two_inline_editors() {
        let geometry = tree_row_geometry_for_mode(3, 2, 360, TreeRowMode::Editing);
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
            if mode == TreeRowMode::Editing {
                assert!(geometry.actions.primary.width() >= 20);
            } else {
                assert_eq!(geometry.actions.primary.width(), 0);
            }
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
    fn shared_tree_connectors_include_ancestor_guide_and_last_child_elbow() {
        let sidebar = PixelRect {
            left: 0,
            top: 0,
            right: 244,
            bottom: 700,
        };
        let geometry = sidebar_tree_row_geometry(sidebar, 1, 2, TreeRowMode::Normal);
        let segments =
            tree_connector_segments(sidebar, &geometry, 2, &[true], true, TreeRowMode::Normal);

        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0].width(), 1);
        assert_eq!(segments[0].top, geometry.row.top);
        assert_eq!(segments[0].bottom, geometry.row.bottom);
        assert_eq!(segments[1].width(), 1);
        assert_eq!(segments[1].bottom, geometry.node_y + 1);
        assert_eq!(segments[2].top, geometry.node_y);
        assert_eq!(segments[2].height(), 1);
        assert_eq!(segments[2].right, geometry.node_x);
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
    fn scrollbar_hit_test_distinguishes_thumb_and_track() {
        let terminal = PixelRect {
            left: 0,
            top: 0,
            right: 800,
            bottom: 600,
        };
        let geometry = terminal_scrollbar_geometry(terminal, 24, 45, 90);
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

    fn sidebar_tree_for_configured_width(tabs_width: i32) -> PixelRect {
        layout(1000, 700, true, tabs_width).sidebar_tree
    }

    fn assert_disclosure_hit_region(geometry: &TreeRowGeometry) {
        assert!(
            geometry
                .disclosure_hit
                .contains(geometry.node_x, geometry.node_y),
            "{geometry:?}"
        );
        assert!(
            geometry.disclosure_hit.contains_x(geometry.node_x - 6),
            "{geometry:?}"
        );
        assert!(
            !geometry.disclosure_hit.contains_x(geometry.node_x + 12),
            "{geometry:?}"
        );
        assert_eq!(
            geometry.disclosure_hit.top, geometry.row.top,
            "{geometry:?}"
        );
        assert_eq!(
            geometry.disclosure_hit.bottom,
            geometry.row.top + TAB_HEIGHT,
            "{geometry:?}"
        );
        assert!(
            geometry
                .disclosure_hit
                .contains(geometry.node_x, geometry.row.top + TAB_HEIGHT / 2),
            "{geometry:?}"
        );
    }

    fn assert_sidebar_row_disjoint_from_scrollbar(
        sidebar_tree: PixelRect,
        geometry: &TreeRowGeometry,
    ) {
        let track = sidebar_scrollbar_track(sidebar_tree);
        assert_eq!(track.left, sidebar_tree.left);
        assert_eq!(track.right, sidebar_tree.left + TERMINAL_SCROLLBAR_WIDTH);
        assert!(
            geometry.row.left >= track.right,
            "row overlaps scrollbar track: {geometry:?} track={track:?}"
        );
        assert!(
            geometry.disclosure_hit.left >= track.right,
            "disclosure overlaps scrollbar track: {geometry:?} track={track:?}"
        );
        assert!(
            geometry.selection.left >= track.right,
            "selection overlaps scrollbar track: {geometry:?} track={track:?}"
        );
        assert!(
            geometry.text.left >= track.right,
            "text overlaps scrollbar track: {geometry:?} track={track:?}"
        );
        assert!(
            geometry.actions.bounds.right <= sidebar_tree.right,
            "actions extend past sidebar tree: {geometry:?} sidebar_tree={sidebar_tree:?}"
        );
        if let Some(editors) = geometry.editors {
            assert!(
                editors.name.left >= track.right,
                "editor overlaps scrollbar track: {editors:?} track={track:?}"
            );
            assert!(editors.note.left >= track.right);
        }
        if let Some(add) = geometry.actions.add_child {
            assert!(add.left >= track.right);
        }
        assert!(geometry.actions.primary.left >= track.right);
        assert!(geometry.actions.secondary.left >= track.right);
    }

    fn assert_sidebar_row_partitions_without_overlap(geometry: &TreeRowGeometry) {
        assert!(geometry.text.right <= geometry.actions.bounds.left);
        if let Some(add) = geometry.actions.add_child {
            assert!(add.right <= geometry.actions.primary.left);
        }
        assert!(geometry.actions.primary.right <= geometry.actions.secondary.left);
        assert!(geometry.actions.bounds.right <= geometry.selection.right);
        assert!(geometry.disclosure_hit.right <= geometry.text.left);
        assert!(geometry.node_x < geometry.text.left);
    }

    #[test]
    fn sidebar_tree_row_geometry_at_minimum_width_normal_and_editing() {
        let sidebar_tree = sidebar_tree_for_configured_width(TABS_MIN_WIDTH);
        assert_eq!(sidebar_tree.right, TABS_MIN_WIDTH - TABS_RESIZE_GRIP_WIDTH);

        for mode in [TreeRowMode::Normal, TreeRowMode::Editing] {
            let shallow = sidebar_tree_row_geometry(sidebar_tree, 0, 0, mode);
            let deep = sidebar_tree_row_geometry(sidebar_tree, 1, MAX_TREE_DEPTH, mode);

            assert_eq!(shallow.mode, mode);
            assert_eq!(deep.mode, mode);
            assert_eq!(shallow.actions.density, TreeRowActionDensity::Compact);
            assert_eq!(deep.actions.density, TreeRowActionDensity::Compact);
            assert_disclosure_hit_region(&shallow);
            assert_disclosure_hit_region(&deep);
            assert_sidebar_row_disjoint_from_scrollbar(sidebar_tree, &shallow);
            assert_sidebar_row_disjoint_from_scrollbar(sidebar_tree, &deep);
            assert_sidebar_row_partitions_without_overlap(&shallow);
            assert_sidebar_row_partitions_without_overlap(&deep);

            let content_width = sidebar_tree.right - TERMINAL_SCROLLBAR_WIDTH;
            let untranslated = tree_row_geometry_for_mode(1, MAX_TREE_DEPTH, content_width, mode);
            assert_eq!(deep.node_x, untranslated.node_x + TERMINAL_SCROLLBAR_WIDTH);
            assert_eq!(
                deep.disclosure_hit,
                translate_tree_row_geometry(untranslated, TERMINAL_SCROLLBAR_WIDTH).disclosure_hit
            );
        }
    }

    #[test]
    fn sidebar_tree_row_geometry_at_default_width_uses_compact_actions() {
        let sidebar_tree = sidebar_tree_for_configured_width(TABS_DEFAULT_WIDTH);
        assert_eq!(
            sidebar_tree.right,
            TABS_DEFAULT_WIDTH - TABS_RESIZE_GRIP_WIDTH
        );

        for mode in [TreeRowMode::Normal, TreeRowMode::Editing] {
            let geometry = sidebar_tree_row_geometry(sidebar_tree, 2, 1, mode);

            assert_eq!(geometry.mode, mode);
            assert_eq!(geometry.actions.density, TreeRowActionDensity::Compact);
            assert_disclosure_hit_region(&geometry);
            assert_sidebar_row_disjoint_from_scrollbar(sidebar_tree, &geometry);
            assert_sidebar_row_partitions_without_overlap(&geometry);
            assert_eq!(geometry.row.left, TERMINAL_SCROLLBAR_WIDTH + TAB_LEFT);
            assert_eq!(geometry.row.right, sidebar_tree.right - TAB_RIGHT_MARGIN);
        }
    }

    #[test]
    fn default_tabs_width_omits_edit_action() {
        let geometry = tree_row_geometry_for_mode(0, 0, TABS_DEFAULT_WIDTH, TreeRowMode::Normal);
        let add = geometry
            .actions
            .add_child
            .expect("normal mode has add-child");

        assert_eq!(geometry.actions.density, TreeRowActionDensity::Compact);
        assert_eq!(add.width(), TREE_COMPACT_ADD_ACTION_WIDTH);
        assert_eq!(geometry.actions.primary.width(), 0);
        assert_eq!(
            geometry.actions.secondary.width(),
            TREE_COMPACT_CLOSE_ACTION_WIDTH
        );
    }

    #[test]
    fn sidebar_tree_row_geometry_at_maximum_width_uses_full_actions() {
        let sidebar_tree = sidebar_tree_for_configured_width(TABS_MAX_WIDTH);
        assert_eq!(sidebar_tree.right, TABS_MAX_WIDTH - TABS_RESIZE_GRIP_WIDTH);

        let normal = sidebar_tree_row_geometry(sidebar_tree, 0, 2, TreeRowMode::Normal);
        let editing = sidebar_tree_row_geometry(sidebar_tree, 3, 1, TreeRowMode::Editing);

        assert_eq!(normal.actions.density, TreeRowActionDensity::Full);
        assert_eq!(editing.actions.density, TreeRowActionDensity::Full);
        assert_disclosure_hit_region(&normal);
        assert_disclosure_hit_region(&editing);
        assert_sidebar_row_disjoint_from_scrollbar(sidebar_tree, &normal);
        assert_sidebar_row_disjoint_from_scrollbar(sidebar_tree, &editing);
        assert_sidebar_row_partitions_without_overlap(&normal);
        assert_sidebar_row_partitions_without_overlap(&editing);

        let add = normal
            .actions
            .add_child
            .expect("full normal row has add-child");
        assert_eq!(add.width(), TREE_ADD_ACTION_WIDTH);
        assert_eq!(normal.actions.primary.width(), 0);
        assert_eq!(normal.actions.secondary.width(), TREE_CLOSE_ACTION_WIDTH);
        assert_eq!(editing.actions.add_child, None);
        assert_eq!(editing.actions.primary.width(), TREE_SAVE_ACTION_WIDTH);
        assert_eq!(editing.actions.secondary.width(), TREE_CANCEL_ACTION_WIDTH);
        assert!(editing.editors.is_some());
    }

    #[test]
    fn sidebar_disclosure_hit_is_right_of_scrollbar_at_all_canonical_widths() {
        for tabs_width in [TABS_MIN_WIDTH, TABS_DEFAULT_WIDTH, TABS_MAX_WIDTH] {
            let sidebar_tree = sidebar_tree_for_configured_width(tabs_width);
            let track = sidebar_scrollbar_track(sidebar_tree);

            for (viewport, depth, mode) in [
                (0, 0, TreeRowMode::Normal),
                (1, MAX_TREE_DEPTH, TreeRowMode::Normal),
                (2, 3, TreeRowMode::Editing),
            ] {
                let geometry = sidebar_tree_row_geometry(sidebar_tree, viewport, depth, mode);
                assert_disclosure_hit_region(&geometry);
                assert!(
                    geometry.disclosure_hit.left >= track.right,
                    "width={tabs_width} viewport={viewport} depth={depth} mode={mode:?}"
                );
                assert!(
                    geometry.disclosure_hit.right <= geometry.text.left,
                    "width={tabs_width} viewport={viewport} depth={depth} mode={mode:?}"
                );
            }
        }
    }

    #[test]
    fn sidebar_scrollbar_track_and_row_capacity_match_tree_surface() {
        for tabs_width in [TABS_MIN_WIDTH, TABS_DEFAULT_WIDTH, TABS_MAX_WIDTH] {
            let sidebar_tree = sidebar_tree_for_configured_width(tabs_width);
            let track = sidebar_scrollbar_track(sidebar_tree);
            let capacity = sidebar_row_capacity(sidebar_tree.height());
            let scrollbar = sidebar_scrollbar_geometry(track, 0, 10, capacity, capacity + 5);

            assert_eq!(track.width(), TERMINAL_SCROLLBAR_WIDTH);
            assert_eq!(scrollbar.track, track);
            assert!(scrollbar.thumb.left >= track.left + 2);
            assert!(scrollbar.thumb.right <= track.right - 2);
            assert_valid_rect(track, sidebar_tree);
            assert_valid_rect(scrollbar.thumb, sidebar_tree);
            assert!(
                capacity >= 1,
                "sidebar tree should expose at least one row slot"
            );
        }
    }
}
