//! Host-neutral, allocation-free dirty evidence primitives.
//!
//! These types describe a conservative raster candidate. They do not imply
//! that a host can perform a partial present. Missing or uncertain damage must
//! be represented as full damage rather than guessed as a smaller rectangle.

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PixelRect {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

impl PixelRect {
    pub const fn empty() -> Self {
        Self {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        }
    }

    pub const fn full_frame(width: u32, height: u32) -> Self {
        Self {
            left: 0,
            top: 0,
            right: width,
            bottom: height,
        }
    }

    /// Creates a half-open rectangle. Coordinate addition saturates so a
    /// malformed or adversarial width cannot wrap around and lose coverage.
    pub const fn from_xywh(left: u32, top: u32, width: u32, height: u32) -> Self {
        Self {
            left,
            top,
            right: left.saturating_add(width),
            bottom: top.saturating_add(height),
        }
    }

    pub const fn is_empty(self) -> bool {
        self.right <= self.left || self.bottom <= self.top
    }

    pub const fn width(self) -> u32 {
        self.right.saturating_sub(self.left)
    }

    pub const fn height(self) -> u32 {
        self.bottom.saturating_sub(self.top)
    }

    pub const fn area(self) -> u64 {
        self.width() as u64 * self.height() as u64
    }

    pub fn union(self, other: Self) -> Self {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return self;
        }
        Self {
            left: self.left.min(other.left),
            top: self.top.min(other.top),
            right: self.right.max(other.right),
            bottom: self.bottom.max(other.bottom),
        }
    }

    pub fn clip(self, width: u32, height: u32) -> Self {
        let left = self.left.min(width);
        let top = self.top.min(height);
        let right = self.right.min(width).max(left);
        let bottom = self.bottom.min(height).max(top);
        Self {
            left,
            top,
            right,
            bottom,
        }
    }
}

/// A conservative half-open row interval. Non-contiguous changes are merged
/// into first..end so this type never needs an allocation or a small-vector
/// overflow policy.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DirtyRows {
    first: u32,
    end: u64,
}

impl DirtyRows {
    pub const fn empty() -> Self {
        Self { first: 0, end: 0 }
    }

    pub const fn is_empty(self) -> bool {
        self.first as u64 >= self.end
    }

    pub const fn first(self) -> u32 {
        self.first
    }

    pub const fn end(self) -> u64 {
        self.end
    }

    pub fn mark_row(&mut self, row: u32) {
        self.mark_range(row, u64::from(row).saturating_add(1));
    }

    /// `end` is u64 so a row at `u32::MAX` can still be represented as an
    /// exclusive interval without wrapping.
    pub fn mark_range(&mut self, first: u32, end: u64) {
        if end <= first as u64 {
            return;
        }
        if self.is_empty() {
            self.first = first;
            self.end = end;
        } else {
            self.first = self.first.min(first);
            self.end = self.end.max(end);
        }
    }

    pub fn union(&mut self, other: Self) {
        if other.is_empty() {
            return;
        }
        self.mark_range(other.first, other.end);
    }

    pub fn clip(self, row_count: u32) -> Self {
        if row_count == 0 || self.is_empty() {
            return Self::empty();
        }
        let first = self.first.min(row_count);
        let end = self.end.min(row_count as u64);
        if first as u64 >= end {
            Self::empty()
        } else {
            Self { first, end }
        }
    }

    /// Converts the row interval to a full-width pixel bound. Saturating
    /// arithmetic makes oversized row/cell inputs deterministic; clipping
    /// then guarantees that the result stays inside the frame.
    pub fn to_pixel_bounds(
        self,
        origin_x: u32,
        origin_y: u32,
        cell_w: u32,
        cell_h: u32,
        frame_width: u32,
        frame_height: u32,
    ) -> Option<PixelRect> {
        if self.is_empty() || cell_h == 0 || frame_width == 0 || frame_height == 0 {
            return None;
        }
        let top = origin_y
            .saturating_add(saturating_mul_u32(self.first as u64, cell_h))
            .min(frame_height);
        let bottom = origin_y
            .saturating_add(saturating_mul_u32(self.end, cell_h))
            .min(frame_height);
        let left = origin_x.min(frame_width);
        let right = frame_width;
        if bottom <= top || right <= left || cell_w == 0 {
            return None;
        }
        Some(PixelRect {
            left,
            top,
            right,
            bottom,
        })
    }
}

fn saturating_mul_u32(value: u64, factor: u32) -> u32 {
    value
        .saturating_mul(u64::from(factor))
        .min(u64::from(u32::MAX)) as u32
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DirtyRegion {
    full: bool,
    bounds: Option<PixelRect>,
}

impl DirtyRegion {
    pub const fn empty() -> Self {
        Self {
            full: false,
            bounds: None,
        }
    }

    /// A full flag without dimensions is useful before the first frame. It is
    /// clipped to the actual frame immediately before candidate accounting.
    pub const fn full() -> Self {
        Self {
            full: true,
            bounds: None,
        }
    }

    pub const fn full_frame(width: u32, height: u32) -> Self {
        Self {
            full: true,
            bounds: Some(PixelRect::full_frame(width, height)),
        }
    }

    pub const fn is_empty(self) -> bool {
        !self.full && self.bounds.is_none()
    }

    pub const fn is_full(self) -> bool {
        self.full
    }

    pub const fn bounds(self) -> Option<PixelRect> {
        self.bounds
    }

    pub fn mark_full(&mut self) {
        self.full = true;
        self.bounds = None;
    }

    pub fn mark_rect(&mut self, rect: PixelRect) {
        if self.full || rect.is_empty() {
            return;
        }
        self.bounds = Some(self.bounds.map_or(rect, |current| current.union(rect)));
    }

    pub fn union(self, other: Self) -> Self {
        if self.full || other.full {
            return Self::full();
        }
        Self {
            full: false,
            bounds: match (self.bounds, other.bounds) {
                (Some(left), Some(right)) => Some(left.union(right)),
                (Some(bounds), None) | (None, Some(bounds)) => Some(bounds),
                (None, None) => None,
            },
        }
    }

    pub fn clip(self, width: u32, height: u32) -> Self {
        if self.full {
            return Self::full_frame(width, height);
        }
        match self.bounds {
            Some(bounds) => {
                let clipped = bounds.clip(width, height);
                if clipped.is_empty() {
                    Self::empty()
                } else {
                    Self {
                        full: false,
                        bounds: Some(clipped),
                    }
                }
            }
            None => Self::empty(),
        }
    }

    pub fn dirty_pixels(self, frame_width: u32, frame_height: u32) -> u64 {
        self.clip(frame_width, frame_height)
            .bounds
            .map_or(0, PixelRect::area)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_union_and_clip_are_conservative() {
        let left = PixelRect::from_xywh(2, 3, 4, 5);
        let right = PixelRect::from_xywh(20, 30, 4, 5);
        assert_eq!(left.union(right), PixelRect::from_xywh(2, 3, 22, 32));
        assert_eq!(
            left.union(right).clip(10, 10),
            PixelRect::from_xywh(2, 3, 8, 7)
        );
    }

    #[test]
    fn coordinate_and_area_overflow_saturate() {
        let rect = PixelRect::from_xywh(u32::MAX - 2, u32::MAX - 3, 99, 99);
        assert_eq!(rect.right, u32::MAX);
        assert_eq!(rect.bottom, u32::MAX);
        assert_eq!(rect.clip(0, 0), PixelRect::empty());
        assert_eq!(
            PixelRect::from_xywh(0, 0, u32::MAX, u32::MAX).area(),
            u64::from(u32::MAX) * u64::from(u32::MAX)
        );
    }

    #[test]
    fn full_dominates_rect_and_zero_frame_is_deterministic() {
        let mut region = DirtyRegion::empty();
        region.mark_rect(PixelRect::from_xywh(3, 4, 5, 6));
        region.mark_full();
        region.mark_rect(PixelRect::from_xywh(100, 100, 1, 1));
        assert!(region.is_full());
        assert_eq!(region.clip(0, 0), DirtyRegion::full_frame(0, 0));
        assert_eq!(region.dirty_pixels(0, 0), 0);
    }

    #[test]
    fn rows_merge_noncontiguous_changes_and_clip() {
        let mut rows = DirtyRows::empty();
        rows.mark_row(2);
        rows.mark_row(10);
        assert_eq!(rows.first(), 2);
        assert_eq!(rows.end(), 11);
        assert_eq!(rows.clip(8), DirtyRows { first: 2, end: 8 });
        assert_eq!(rows.clip(2), DirtyRows::empty());
    }

    #[test]
    fn rows_at_maximum_do_not_wrap() {
        let mut rows = DirtyRows::empty();
        rows.mark_row(u32::MAX);
        assert_eq!(rows.first(), u32::MAX);
        assert_eq!(rows.end(), u64::from(u32::MAX) + 1);
        assert_eq!(rows.clip(u32::MAX), DirtyRows::empty());
    }

    #[test]
    fn rows_convert_to_full_width_pixel_bounds() {
        let mut rows = DirtyRows::empty();
        rows.mark_range(2, 5);
        assert_eq!(
            rows.to_pixel_bounds(10, 4, 8, 12, 100, 100),
            Some(PixelRect::from_xywh(10, 28, 90, 36))
        );
        assert_eq!(rows.to_pixel_bounds(10, 4, 8, 12, 100, 20), None);
    }
}
