//! Spectacle 1.2 window-position calculations, rewritten against the JS specs.

use super::PlaceAction;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn min_x(self) -> f64 {
        self.x
    }
    pub fn min_y(self) -> f64 {
        self.y
    }
    pub fn mid_x(self) -> f64 {
        self.x + self.width / 2.0
    }
    pub fn mid_y(self) -> f64 {
        self.y + self.height / 2.0
    }
    pub fn max_x(self) -> f64 {
        self.x + self.width
    }
    pub fn max_y(self) -> f64 {
        self.y + self.height
    }

    pub fn area(self) -> f64 {
        (self.width * self.height).max(0.0)
    }

    pub fn contains(self, inner: Self) -> bool {
        self.min_x() <= inner.min_x()
            && self.min_y() <= inner.min_y()
            && self.max_x() >= inner.max_x()
            && self.max_y() >= inner.max_y()
    }

    pub fn intersection(self, other: Self) -> Option<Self> {
        let x = self.min_x().max(other.min_x());
        let y = self.min_y().max(other.min_y());
        let max_x = self.max_x().min(other.max_x());
        let max_y = self.max_y().min(other.max_y());
        if max_x > x && max_y > y {
            Some(Self::new(x, y, max_x - x, max_y - y))
        } else {
            None
        }
    }

    pub fn centered_within(self, outer: Self) -> bool {
        outer.contains(self)
            && (self.mid_x() - outer.mid_x()).abs() <= 1.0
            && (self.mid_y() - outer.mid_y()).abs() <= 1.0
    }

    pub fn fits_within(self, outer: Self) -> bool {
        self.width <= outer.width && self.height <= outer.height
    }

    pub fn almost_eq(self, other: Self) -> bool {
        (self.x - other.x).abs() <= 1.0
            && (self.y - other.y).abs() <= 1.0
            && (self.width - other.width).abs() <= 1.0
            && (self.height - other.height).abs() <= 1.0
    }

    pub fn to_i32(self) -> (i32, i32, u32, u32) {
        (
            self.x.round() as i32,
            self.y.round() as i32,
            self.width.round().max(1.0) as u32,
            self.height.round().max(1.0) as u32,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Screen {
    pub visible: Rect,
    pub frame: Rect,
}

pub fn place(action: PlaceAction, window: Rect, screens: &[Screen]) -> Option<Rect> {
    if matches!(action, PlaceAction::Undo | PlaceAction::Redo) {
        return None;
    }
    if screens.is_empty() {
        return None;
    }
    let ordered = screens_in_consistent_order(screens);
    let source = screen_containing(window, &ordered)?;
    let dest = if action.is_display_walk() {
        next_or_previous_screen(source, action, &ordered)?
    } else {
        source
    };
    Some(calculate(action, window, source.visible, dest.visible))
}

fn screens_in_consistent_order(screens: &[Screen]) -> Vec<Screen> {
    let mut out = screens.to_vec();
    out.sort_by(|a, b| {
        let a0 = a.frame.x == 0.0 && a.frame.y == 0.0;
        let b0 = b.frame.x == 0.0 && b.frame.y == 0.0;
        match (a0, b0) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => b
                .frame
                .y
                .partial_cmp(&a.frame.y)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| {
                    b.frame
                        .x
                        .partial_cmp(&a.frame.x)
                        .unwrap_or(std::cmp::Ordering::Equal)
                }),
        }
    });
    out
}

fn screen_containing(window: Rect, screens: &[Screen]) -> Option<Screen> {
    let mut best: Option<(f64, Screen)> = None;
    for screen in screens {
        if screen.frame.contains(window) {
            return Some(*screen);
        }
        if let Some(hit) = window.intersection(screen.frame) {
            let pct = hit.area() / window.area().max(1.0);
            if best.map(|(p, _)| pct > p).unwrap_or(true) {
                best = Some((pct, *screen));
            }
        }
    }
    best.map(|(_, s)| s).or_else(|| screens.first().copied())
}

fn next_or_previous_screen(
    source: Screen,
    action: PlaceAction,
    screens: &[Screen],
) -> Option<Screen> {
    if screens.len() <= 1 {
        return None;
    }
    let idx = screens.iter().position(|s| {
        (s.frame.x - source.frame.x).abs() < 0.5 && (s.frame.y - source.frame.y).abs() < 0.5
    })?;
    let next = match action {
        PlaceAction::NextDisplay => (idx + 1) % screens.len(),
        PlaceAction::PreviousDisplay => (idx + screens.len() - 1) % screens.len(),
        _ => return Some(source),
    };
    Some(screens[next])
}

fn calculate(action: PlaceAction, window: Rect, _src: Rect, dest: Rect) -> Rect {
    match action {
        PlaceAction::Center => center(window, dest),
        PlaceAction::Fullscreen => dest,
        PlaceAction::LeftHalf => cycle_left(window, dest),
        PlaceAction::RightHalf => cycle_right(window, dest),
        PlaceAction::TopHalf => cycle_top(window, dest),
        PlaceAction::BottomHalf => cycle_bottom(window, dest),
        PlaceAction::UpperLeft => cycle_upper_left(window, dest),
        PlaceAction::LowerLeft => cycle_lower_left(window, dest),
        PlaceAction::UpperRight => cycle_upper_right(window, dest),
        PlaceAction::LowerRight => cycle_lower_right(window, dest),
        PlaceAction::NextThird => find_next_third(window, dest),
        PlaceAction::PreviousThird => find_previous_third(window, dest),
        PlaceAction::NextDisplay | PlaceAction::PreviousDisplay => {
            if window.fits_within(dest) {
                center(window, dest)
            } else {
                dest
            }
        }
        PlaceAction::Larger => resize_window_rect(window, dest, 30.0),
        PlaceAction::Smaller => resize_window_rect(window, dest, -30.0),
        PlaceAction::Undo | PlaceAction::Redo => window,
    }
}

fn center(window: Rect, visible: Rect) -> Rect {
    Rect::new(
        ((visible.width - window.width) / 2.0).round() + visible.x,
        ((visible.height - window.height) / 2.0).round() + visible.y,
        window.width,
        window.height,
    )
}

fn cycle_left(window: Rect, visible: Rect) -> Rect {
    let mut half = visible;
    half.width = (visible.width / 2.0).floor();
    if (window.mid_y() - half.mid_y()).abs() <= 1.0 {
        let mut two = half;
        two.width = (visible.width * 2.0 / 3.0).floor();
        if window.centered_within(half) {
            return two;
        }
        if window.centered_within(two) {
            let mut one = half;
            one.width = (visible.width / 3.0).floor();
            return one;
        }
    }
    half
}

fn cycle_right(window: Rect, visible: Rect) -> Rect {
    let mut half = visible;
    half.width = (visible.width / 2.0).floor();
    half.x += half.width;
    if (window.mid_y() - half.mid_y()).abs() <= 1.0 {
        let mut two = half;
        two.width = (visible.width * 2.0 / 3.0).floor();
        two.x = visible.x + visible.width - two.width;
        if window.centered_within(half) {
            return two;
        }
        if window.centered_within(two) {
            let mut one = half;
            one.width = (visible.width / 3.0).floor();
            one.x = visible.x + visible.width - one.width;
            return one;
        }
    }
    half
}

fn cycle_top(window: Rect, visible: Rect) -> Rect {
    let mut half = visible;
    half.height = (visible.height / 2.0).floor();
    half.y += half.height + (visible.height % 2.0);
    if (window.mid_x() - half.mid_x()).abs() <= 1.0 {
        let mut two = half;
        two.height = (visible.height * 2.0 / 3.0).floor();
        two.y = visible.y + visible.height - two.height;
        if window.centered_within(half) {
            return two;
        }
        if window.centered_within(two) {
            let mut one = half;
            one.height = (visible.height / 3.0).floor();
            one.y = visible.y + visible.height - one.height;
            return one;
        }
    }
    half
}

fn cycle_bottom(window: Rect, visible: Rect) -> Rect {
    let mut half = visible;
    half.height = (visible.height / 2.0).floor();
    if (window.mid_x() - half.mid_x()).abs() <= 1.0 {
        let mut two = half;
        two.height = (visible.height * 2.0 / 3.0).floor();
        if window.centered_within(half) {
            return two;
        }
        if window.centered_within(two) {
            let mut one = half;
            one.height = (visible.height / 3.0).floor();
            return one;
        }
    }
    half
}

fn cycle_upper_left(window: Rect, visible: Rect) -> Rect {
    let mut q = visible;
    q.width = (visible.width / 2.0).floor();
    q.height = (visible.height / 2.0).floor();
    q.y = visible.y + (visible.height / 2.0).floor() + (visible.height % 2.0);
    cycle_corner_width(window, visible, q, false)
}

fn cycle_lower_left(window: Rect, visible: Rect) -> Rect {
    let mut q = visible;
    q.width = (visible.width / 2.0).floor();
    q.height = (visible.height / 2.0).floor();
    cycle_corner_width(window, visible, q, false)
}

fn cycle_upper_right(window: Rect, visible: Rect) -> Rect {
    let mut q = visible;
    q.width = (visible.width / 2.0).floor();
    q.height = (visible.height / 2.0).floor();
    q.x += q.width;
    q.y = visible.y + (visible.height / 2.0).floor() + (visible.height % 2.0);
    cycle_corner_width(window, visible, q, true)
}

fn cycle_lower_right(window: Rect, visible: Rect) -> Rect {
    let mut q = visible;
    q.width = (visible.width / 2.0).floor();
    q.height = (visible.height / 2.0).floor();
    q.x += q.width;
    cycle_corner_width(window, visible, q, true)
}

fn cycle_corner_width(window: Rect, visible: Rect, quarter: Rect, from_right: bool) -> Rect {
    if (window.mid_y() - quarter.mid_y()).abs() <= 1.0 {
        let mut two = quarter;
        two.width = (visible.width * 2.0 / 3.0).floor();
        if from_right {
            two.x = visible.x + visible.width - two.width;
        }
        if window.centered_within(quarter) {
            return two;
        }
        if window.centered_within(two) {
            let mut one = quarter;
            one.width = (visible.width / 3.0).floor();
            if from_right {
                one.x = visible.x + visible.width - one.width;
            }
            return one;
        }
    }
    quarter
}

fn thirds(visible: Rect) -> Vec<Rect> {
    let mut out = Vec::with_capacity(6);
    let w = (visible.width / 3.0).floor();
    for i in 0..3 {
        out.push(Rect::new(
            visible.x + w * f64::from(i),
            visible.y,
            w,
            visible.height,
        ));
    }
    let h = (visible.height / 3.0).floor();
    for i in 0..3 {
        out.push(Rect::new(
            visible.x,
            visible.y + visible.height - h * f64::from(i + 1),
            visible.width,
            h,
        ));
    }
    out
}

fn find_next_third(window: Rect, visible: Rect) -> Rect {
    let list = thirds(visible);
    let mut result = list[0];
    for (i, third) in list.iter().enumerate() {
        if window.centered_within(*third) {
            result = list[(i + 1) % list.len()];
            break;
        }
    }
    result
}

fn find_previous_third(window: Rect, visible: Rect) -> Rect {
    let list = thirds(visible);
    let mut result = list[0];
    for (i, third) in list.iter().enumerate() {
        if window.centered_within(*third) {
            result = list[(i + list.len() - 1) % list.len()];
            break;
        }
    }
    result
}

fn against_edge(gap: f64) -> bool {
    gap.abs() <= 5.0
}

fn resize_window_rect(window: Rect, visible: Rect, size_offset: f64) -> Rect {
    let mut resized = window;
    resized.width += size_offset;
    resized.x -= (size_offset / 2.0).floor();
    resized = adjust_left_right(window, resized, visible);
    if resized.width >= visible.width {
        resized.width = visible.width;
    }
    resized.height += size_offset;
    resized.y -= (size_offset / 2.0).floor();
    resized = adjust_top_bottom(window, resized, visible);
    if resized.height >= visible.height {
        resized.height = visible.height;
        resized.y = window.y;
    }
    if against_all(window, visible) && size_offset < 0.0 {
        resized.width = window.width + size_offset;
        resized.x = window.x - (size_offset / 2.0).floor();
        resized.height = window.height + size_offset;
        resized.y = window.y - (size_offset / 2.0).floor();
    }
    if too_small(resized, visible) {
        return window;
    }
    resized
}

fn against_left(window: Rect, visible: Rect) -> bool {
    against_edge(window.x - visible.x)
}
fn against_right(window: Rect, visible: Rect) -> bool {
    against_edge(window.max_x() - visible.max_x())
}
fn against_top(window: Rect, visible: Rect) -> bool {
    against_edge(window.max_y() - visible.max_y())
}
fn against_bottom(window: Rect, visible: Rect) -> bool {
    against_edge(window.y - visible.y)
}
fn against_all(window: Rect, visible: Rect) -> bool {
    against_left(window, visible)
        && against_right(window, visible)
        && against_top(window, visible)
        && against_bottom(window, visible)
}

fn adjust_left_right(original: Rect, resized: Rect, visible: Rect) -> Rect {
    let mut adjusted = resized;
    if against_right(original, visible) {
        adjusted.x = visible.max_x() - adjusted.width;
        if against_left(original, visible) {
            adjusted.width = visible.width;
        }
    }
    if against_left(original, visible) {
        adjusted.x = visible.x;
    }
    adjusted
}

fn adjust_top_bottom(original: Rect, resized: Rect, visible: Rect) -> Rect {
    let mut adjusted = resized;
    if against_top(original, visible) {
        adjusted.y = visible.max_y() - adjusted.height;
        if against_bottom(original, visible) {
            adjusted.height = visible.height;
        }
    }
    if against_bottom(original, visible) {
        adjusted.y = visible.y;
    }
    adjusted
}

fn too_small(window: Rect, visible: Rect) -> bool {
    window.width <= (visible.width / 4.0).floor() || window.height <= (visible.height / 4.0).floor()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vis() -> Rect {
        Rect::new(0.0, 4.0, 1440.0, 873.0)
    }
    fn screen() -> Vec<Screen> {
        vec![Screen {
            visible: vis(),
            frame: Rect::new(0.0, 0.0, 1440.0, 900.0),
        }]
    }
    fn calc(action: PlaceAction, win: Rect) -> Rect {
        place(action, win, &screen()).expect("place")
    }
    fn eq(got: Rect, x: f64, y: f64, w: f64, h: f64) {
        let want = Rect::new(x, y, w, h);
        assert!(
            got.almost_eq(want) || (got.x == x && got.y == y && got.width == w && got.height == h),
            "got {got:?} want {want:?}"
        );
        assert_eq!(got.x, x);
        assert_eq!(got.y, y);
        assert_eq!(got.width, w);
        assert_eq!(got.height, h);
    }

    #[test]
    fn center_matches_spec() {
        eq(
            calc(PlaceAction::Center, Rect::new(165.0, 245.0, 564.0, 384.0)),
            438.0,
            249.0,
            564.0,
            384.0,
        );
    }

    #[test]
    fn fullscreen_is_visible_frame() {
        eq(
            calc(
                PlaceAction::Fullscreen,
                Rect::new(165.0, 245.0, 564.0, 384.0),
            ),
            0.0,
            4.0,
            1440.0,
            873.0,
        );
    }

    #[test]
    fn left_half_cycles() {
        let a = calc(PlaceAction::LeftHalf, Rect::new(165.0, 245.0, 564.0, 384.0));
        eq(a, 0.0, 4.0, 720.0, 873.0);
        let b = calc(PlaceAction::LeftHalf, a);
        eq(b, 0.0, 4.0, 960.0, 873.0);
        let c = calc(PlaceAction::LeftHalf, b);
        eq(c, 0.0, 4.0, 480.0, 873.0);
    }

    #[test]
    fn next_third_walks_horizontal_then_vertical() {
        let a = calc(
            PlaceAction::NextThird,
            Rect::new(165.0, 245.0, 564.0, 384.0),
        );
        eq(a, 0.0, 4.0, 480.0, 873.0);
        let b = calc(PlaceAction::NextThird, a);
        eq(b, 480.0, 4.0, 480.0, 873.0);
        let c = calc(PlaceAction::NextThird, b);
        eq(c, 960.0, 4.0, 480.0, 873.0);
        let d = calc(PlaceAction::NextThird, c);
        eq(d, 0.0, 586.0, 1440.0, 291.0);
    }

    #[test]
    fn upper_left_cycles_width() {
        let a = calc(
            PlaceAction::UpperLeft,
            Rect::new(165.0, 245.0, 564.0, 384.0),
        );
        eq(a, 0.0, 441.0, 720.0, 436.0);
        let b = calc(PlaceAction::UpperLeft, a);
        eq(b, 0.0, 441.0, 960.0, 436.0);
        let c = calc(PlaceAction::UpperLeft, b);
        eq(c, 0.0, 441.0, 480.0, 436.0);
    }

    #[test]
    fn larger_centered_steps() {
        let a = calc(PlaceAction::Larger, Rect::new(360.0, 222.0, 720.0, 436.0));
        eq(a, 345.0, 207.0, 750.0, 466.0);
        let b = calc(PlaceAction::Larger, a);
        eq(b, 330.0, 192.0, 780.0, 496.0);
        let c = calc(PlaceAction::Larger, b);
        eq(c, 315.0, 177.0, 810.0, 526.0);
    }

    #[test]
    fn larger_against_bottom() {
        let a = calc(PlaceAction::Larger, Rect::new(238.0, 4.0, 720.0, 436.0));
        eq(a, 223.0, 4.0, 750.0, 466.0);
    }

    #[test]
    fn larger_against_right() {
        let a = calc(PlaceAction::Larger, Rect::new(720.0, 303.0, 720.0, 436.0));
        eq(a, 690.0, 288.0, 750.0, 466.0);
    }

    #[test]
    fn parse_both_spellings() {
        assert_eq!(PlaceAction::parse("left-half"), Some(PlaceAction::LeftHalf));
        assert_eq!(
            PlaceAction::parse("SpectacleWindowActionLeftHalf"),
            Some(PlaceAction::LeftHalf)
        );
        assert!(PlaceAction::parse("tile-magic").is_none());
    }
}
