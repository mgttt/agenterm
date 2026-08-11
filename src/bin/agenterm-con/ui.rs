//! Pure geometry and hit-testing for the lightweight console chrome.
//!
//! This module deliberately contains no window-host, PTY, or product authority
//! state. Proven rules can therefore move into a shared frontend contract
//! without importing `agenterm-con` lifecycle policy.

pub const SIDEBAR_WIDTH_DIP: f64 = 224.0;
pub const TREE_HEADER_HEIGHT_DIP: f64 = 32.0;
pub const TREE_ROW_HEIGHT_DIP: f64 = 30.0;
pub const COMPOSER_HEIGHT_DIP: f64 = 96.0;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub fn contains(self, x: u32, y: u32) -> bool {
        x >= self.x
            && y >= self.y
            && x < self.x.saturating_add(self.width)
            && y < self.y.saturating_add(self.height)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Layout {
    pub sidebar: Rect,
    pub tree_header_height: u32,
    pub tree_row_height: u32,
    pub composer: Rect,
    pub composer_input: Rect,
    pub composer_send: Rect,
    pub zoom_out: Rect,
    pub zoom_in: Rect,
}

impl Layout {
    pub fn new(width: u32, height: u32, scale: f64) -> Self {
        let scale = scale.max(1.0);
        let sidebar_width = dip(SIDEBAR_WIDTH_DIP, scale).min(width);
        let composer_height = dip(COMPOSER_HEIGHT_DIP, scale).min(height);
        let composer = Rect {
            x: sidebar_width,
            y: height.saturating_sub(composer_height),
            width: width.saturating_sub(sidebar_width),
            height: composer_height,
        };
        let padding = dip(12.0, scale);
        let label_height = dip(24.0, scale);
        let send_width = dip(76.0, scale).min(composer.width.saturating_sub(padding * 2));
        let control_y = composer.y.saturating_add(label_height);
        let control_height = composer
            .height
            .saturating_sub(label_height)
            .saturating_sub(padding);
        let send = Rect {
            x: width.saturating_sub(padding).saturating_sub(send_width),
            y: control_y,
            width: send_width,
            height: control_height,
        };
        let input_x = composer.x.saturating_add(padding);
        let input = Rect {
            x: input_x,
            y: control_y,
            width: send
                .x
                .saturating_sub(dip(8.0, scale))
                .saturating_sub(input_x),
            height: control_height,
        };
        let tool_size = dip(24.0, scale).min(sidebar_width / 2);
        let tool_y = dip(4.0, scale);
        let zoom_in = Rect {
            x: sidebar_width
                .saturating_sub(dip(8.0, scale))
                .saturating_sub(tool_size),
            y: tool_y,
            width: tool_size,
            height: tool_size,
        };
        let zoom_out = Rect {
            x: zoom_in.x.saturating_sub(tool_size),
            y: tool_y,
            width: tool_size,
            height: tool_size,
        };
        Self {
            sidebar: Rect {
                x: 0,
                y: 0,
                width: sidebar_width,
                height,
            },
            tree_header_height: dip(TREE_HEADER_HEIGHT_DIP, scale),
            tree_row_height: dip(TREE_ROW_HEIGHT_DIP, scale).max(1),
            composer,
            composer_input: input,
            composer_send: send,
            zoom_out,
            zoom_in,
        }
    }

    pub fn tree_capacity(self) -> usize {
        self.sidebar
            .height
            .saturating_sub(self.tree_header_height)
            .checked_div(self.tree_row_height)
            .unwrap_or(0) as usize
    }

    pub fn tree_close_rect(self, visible_row: usize, scale: f64) -> Rect {
        let size = dip(20.0, scale).min(self.tree_row_height);
        Rect {
            x: self
                .sidebar
                .width
                .saturating_sub(dip(8.0, scale))
                .saturating_sub(size),
            y: self
                .tree_header_height
                .saturating_add(visible_row as u32 * self.tree_row_height)
                .saturating_add((self.tree_row_height.saturating_sub(size)) / 2),
            width: size,
            height: size,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TreeHit {
    Outside,
    Background,
    ZoomOut,
    ZoomIn,
    Select(usize),
    Close(usize),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComposerHit {
    Outside,
    Input,
    Send,
}

pub fn tree_hit(
    layout: Layout,
    x: u32,
    y: u32,
    scroll_offset: usize,
    item_count: usize,
    scale: f64,
) -> TreeHit {
    if !layout.sidebar.contains(x, y) {
        return TreeHit::Outside;
    }
    if layout.zoom_out.contains(x, y) {
        return TreeHit::ZoomOut;
    }
    if layout.zoom_in.contains(x, y) {
        return TreeHit::ZoomIn;
    }
    if y < layout.tree_header_height {
        return TreeHit::Background;
    }
    let visible_row = ((y - layout.tree_header_height) / layout.tree_row_height) as usize;
    let index = scroll_offset.saturating_add(visible_row);
    if index >= item_count {
        return TreeHit::Background;
    }
    if layout.tree_close_rect(visible_row, scale).contains(x, y) {
        TreeHit::Close(index)
    } else {
        TreeHit::Select(index)
    }
}

pub fn composer_hit(layout: Layout, x: u32, y: u32) -> ComposerHit {
    if layout.composer_send.contains(x, y) {
        ComposerHit::Send
    } else if layout.composer.contains(x, y) {
        ComposerHit::Input
    } else {
        ComposerHit::Outside
    }
}

pub fn clamp_tree_scroll(offset: usize, item_count: usize, capacity: usize) -> usize {
    offset.min(item_count.saturating_sub(capacity.max(1)))
}

pub fn scroll_tree(offset: usize, delta_rows: isize, item_count: usize, capacity: usize) -> usize {
    let next = if delta_rows < 0 {
        offset.saturating_sub(delta_rows.unsigned_abs())
    } else {
        offset.saturating_add(delta_rows as usize)
    };
    clamp_tree_scroll(next, item_count, capacity)
}

pub fn reveal_tree_index(offset: usize, index: usize, item_count: usize, capacity: usize) -> usize {
    let capacity = capacity.max(1);
    let next = if index < offset {
        index
    } else if index >= offset.saturating_add(capacity) {
        index.saturating_add(1).saturating_sub(capacity)
    } else {
        offset
    };
    clamp_tree_scroll(next, item_count, capacity)
}

fn dip(value: f64, scale: f64) -> u32 {
    (value * scale.max(1.0)).round().max(0.0) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_separates_sidebar_terminal_and_composer_controls() {
        let layout = Layout::new(1200, 800, 1.0);
        assert_eq!(layout.sidebar.width, 224);
        assert_eq!(layout.composer.y, 704);
        assert!(layout.composer_input.width > layout.composer_send.width);
        assert!(layout.composer_input.x >= layout.composer.x);
        assert!(layout.composer_input.x + layout.composer_input.width < layout.composer_send.x);
    }

    #[test]
    fn tree_hit_distinguishes_rows_close_buttons_and_background() {
        let layout = Layout::new(1000, 500, 1.0);
        assert_eq!(tree_hit(layout, 10, 10, 0, 4, 1.0), TreeHit::Background);
        assert_eq!(
            tree_hit(
                layout,
                layout.zoom_out.x + 1,
                layout.zoom_out.y + 1,
                0,
                4,
                1.0
            ),
            TreeHit::ZoomOut
        );
        assert_eq!(
            tree_hit(
                layout,
                layout.zoom_in.x + 1,
                layout.zoom_in.y + 1,
                0,
                4,
                1.0
            ),
            TreeHit::ZoomIn
        );
        assert_eq!(tree_hit(layout, 20, 40, 1, 4, 1.0), TreeHit::Select(1));
        let close = layout.tree_close_rect(0, 1.0);
        assert_eq!(
            tree_hit(layout, close.x + 1, close.y + 1, 1, 4, 1.0),
            TreeHit::Close(1)
        );
        assert_eq!(tree_hit(layout, 20, 490, 0, 1, 1.0), TreeHit::Background);
        assert_eq!(tree_hit(layout, 500, 40, 0, 4, 1.0), TreeHit::Outside);
    }

    #[test]
    fn scrolling_is_bounded_and_reveals_active_rows() {
        assert_eq!(scroll_tree(0, 3, 10, 4), 3);
        assert_eq!(scroll_tree(3, -9, 10, 4), 0);
        assert_eq!(scroll_tree(5, 9, 10, 4), 6);
        assert_eq!(reveal_tree_index(0, 8, 10, 4), 5);
        assert_eq!(reveal_tree_index(5, 2, 10, 4), 2);
    }

    #[test]
    fn composer_hit_has_a_distinct_send_action() {
        let layout = Layout::new(1000, 500, 1.0);
        assert_eq!(
            composer_hit(
                layout,
                layout.composer_input.x + 1,
                layout.composer_input.y + 1
            ),
            ComposerHit::Input
        );
        assert_eq!(
            composer_hit(
                layout,
                layout.composer_send.x + 1,
                layout.composer_send.y + 1
            ),
            ComposerHit::Send
        );
        assert_eq!(composer_hit(layout, 500, 100), ComposerHit::Outside);
    }
}
