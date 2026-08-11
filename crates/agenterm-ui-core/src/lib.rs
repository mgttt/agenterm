//! Host-neutral interaction and rendering primitives shared by AgenTerm UIs.

pub mod damage;
pub mod glyph_cache;
pub mod pixel;
pub mod tree;

pub use damage::{DirtyRegion, DirtyRows, PixelRect};
pub use glyph_cache::{GlyphCache, GlyphCacheKey, GlyphCacheStats};
pub use tree::{TreeDepthError, TreeDepthNode, compute_tree_depths, compute_tree_depths_by};

const MIN_THUMB_HEIGHT: i32 = 24;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub const fn width(self) -> i32 {
        self.right - self.left
    }
    pub const fn height(self) -> i32 {
        self.bottom - self.top
    }
    pub const fn contains(self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScrollbarGeometry {
    pub track: Rect,
    pub thumb: Rect,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScrollbarHit {
    Thumb,
    TrackAbove,
    TrackBelow,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ScrollbarThumbDrag {
    grab: i32,
}

impl ScrollbarThumbDrag {
    pub const fn begin(pointer_y: i32, thumb_top: i32) -> Self {
        Self {
            grab: pointer_y - thumb_top,
        }
    }
    pub const fn thumb_top(self, pointer_y: i32) -> i32 {
        pointer_y - self.grab
    }
}

pub fn terminal_scrollbar_geometry(
    terminal: Rect,
    width: i32,
    visible: usize,
    offset: usize,
    maximum: usize,
) -> ScrollbarGeometry {
    let track = Rect {
        left: (terminal.right - width.max(0)).max(terminal.left),
        ..terminal
    };
    let height = track.height().max(0);
    let total = visible.saturating_add(maximum).max(1);
    let proportional = (i64::from(height) * visible.max(1) as i64 / total as i64) as i32;
    let thumb_height = if maximum == 0 {
        height
    } else {
        proportional.max(MIN_THUMB_HEIGHT).min(height)
    };
    let travel = (height - thumb_height).max(0);
    let from_bottom = if maximum == 0 {
        0
    } else {
        (offset.min(maximum) as i64 * i64::from(travel) / maximum as i64) as i32
    };
    let top = track.bottom - thumb_height - from_bottom;
    ScrollbarGeometry {
        track,
        thumb: Rect {
            left: (track.left + 2).min(track.right),
            top,
            right: (track.right - 2).max((track.left + 2).min(track.right)),
            bottom: top + thumb_height,
        },
    }
}

pub fn scrollback_for_thumb_top(g: ScrollbarGeometry, top: i32, maximum: usize) -> usize {
    let travel = g.track.height() - g.thumb.height();
    if maximum == 0 || travel <= 0 {
        return 0;
    }
    let top = top.clamp(g.track.top, g.track.bottom - g.thumb.height());
    let from_bottom = g.track.bottom - g.thumb.height() - top;
    ((i64::from(from_bottom) * maximum as i64 + i64::from(travel) / 2) / i64::from(travel)) as usize
}

pub fn scrollbar_hit_test(g: &ScrollbarGeometry, x: i32, y: i32) -> Option<ScrollbarHit> {
    if !g.track.contains(x, y) {
        None
    } else if g.thumb.contains(x, y) {
        Some(ScrollbarHit::Thumb)
    } else if y < g.thumb.top {
        Some(ScrollbarHit::TrackAbove)
    } else {
        Some(ScrollbarHit::TrackBelow)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rect() -> Rect {
        Rect {
            left: 200,
            top: 0,
            right: 1000,
            bottom: 600,
        }
    }

    #[test]
    fn bottom_middle_top_round_trip() {
        let bottom = terminal_scrollbar_geometry(rect(), 12, 30, 0, 90);
        let middle = terminal_scrollbar_geometry(rect(), 12, 30, 45, 90);
        let top = terminal_scrollbar_geometry(rect(), 12, 30, 90, 90);
        assert_eq!(bottom.track.left, 988);
        assert!(bottom.thumb.top > middle.thumb.top && middle.thumb.top > top.thumb.top);
        assert_eq!(scrollback_for_thumb_top(middle, middle.thumb.top, 90), 45);
    }

    #[test]
    fn drag_keeps_grab_offset() {
        let g = terminal_scrollbar_geometry(rect(), 12, 30, 45, 90);
        assert_eq!(
            scrollbar_hit_test(&g, g.thumb.left, g.thumb.top),
            Some(ScrollbarHit::Thumb)
        );
        let drag = ScrollbarThumbDrag::begin(g.thumb.top + 5, g.thumb.top);
        assert_eq!(drag.thumb_top(g.thumb.top + 25), g.thumb.top + 20);
    }
}
