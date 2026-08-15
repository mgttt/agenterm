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
        assert_eq!(got, want);
    }

    #[test]
    fn deterministic_fixtures_cover_every_stateless_action() {
        let ordinary = Rect::new(165.0, 245.0, 564.0, 384.0);
        let first_third = Rect::new(0.0, 4.0, 480.0, 873.0);
        let centered = Rect::new(360.0, 222.0, 720.0, 436.0);
        let fixtures = [
            (
                PlaceAction::Center,
                ordinary,
                Rect::new(438.0, 249.0, 564.0, 384.0),
            ),
            (PlaceAction::Fullscreen, ordinary, vis()),
            (
                PlaceAction::LeftHalf,
                ordinary,
                Rect::new(0.0, 4.0, 720.0, 873.0),
            ),
            (
                PlaceAction::RightHalf,
                ordinary,
                Rect::new(720.0, 4.0, 720.0, 873.0),
            ),
            (
                PlaceAction::TopHalf,
                ordinary,
                Rect::new(0.0, 441.0, 1440.0, 436.0),
            ),
            (
                PlaceAction::BottomHalf,
                ordinary,
                Rect::new(0.0, 4.0, 1440.0, 436.0),
            ),
            (
                PlaceAction::UpperLeft,
                ordinary,
                Rect::new(0.0, 441.0, 720.0, 436.0),
            ),
            (
                PlaceAction::LowerLeft,
                ordinary,
                Rect::new(0.0, 4.0, 720.0, 436.0),
            ),
            (
                PlaceAction::UpperRight,
                ordinary,
                Rect::new(720.0, 441.0, 720.0, 436.0),
            ),
            (
                PlaceAction::LowerRight,
                ordinary,
                Rect::new(720.0, 4.0, 720.0, 436.0),
            ),
            (
                PlaceAction::NextThird,
                first_third,
                Rect::new(480.0, 4.0, 480.0, 873.0),
            ),
            (
                PlaceAction::PreviousThird,
                first_third,
                Rect::new(0.0, 4.0, 1440.0, 291.0),
            ),
            (
                PlaceAction::Larger,
                centered,
                Rect::new(345.0, 207.0, 750.0, 466.0),
            ),
            (
                PlaceAction::Smaller,
                centered,
                Rect::new(375.0, 237.0, 690.0, 406.0),
            ),
        ];

        for (action, input, want) in fixtures {
            assert_eq!(calc(action, input), want, "fixture for {action:?}");
        }

        // Display walk has a multi-screen input and is covered independently.
        // Undo/redo deliberately have no stateless geometry; see the safe-failure test.
    }

    #[test]
    fn halves_cycle_half_two_thirds_one_third_then_half() {
        let ordinary = Rect::new(165.0, 245.0, 564.0, 384.0);
        let cycles = [
            (
                PlaceAction::LeftHalf,
                [
                    Rect::new(0.0, 4.0, 720.0, 873.0),
                    Rect::new(0.0, 4.0, 960.0, 873.0),
                    Rect::new(0.0, 4.0, 480.0, 873.0),
                ],
            ),
            (
                PlaceAction::RightHalf,
                [
                    Rect::new(720.0, 4.0, 720.0, 873.0),
                    Rect::new(480.0, 4.0, 960.0, 873.0),
                    Rect::new(960.0, 4.0, 480.0, 873.0),
                ],
            ),
            (
                PlaceAction::TopHalf,
                [
                    Rect::new(0.0, 441.0, 1440.0, 436.0),
                    Rect::new(0.0, 295.0, 1440.0, 582.0),
                    Rect::new(0.0, 586.0, 1440.0, 291.0),
                ],
            ),
            (
                PlaceAction::BottomHalf,
                [
                    Rect::new(0.0, 4.0, 1440.0, 436.0),
                    Rect::new(0.0, 4.0, 1440.0, 582.0),
                    Rect::new(0.0, 4.0, 1440.0, 291.0),
                ],
            ),
        ];

        for (action, expected) in cycles {
            let mut current = ordinary;
            for want in expected {
                current = calc(action, current);
                assert_eq!(current, want, "cycle step for {action:?}");
            }
            assert_eq!(
                calc(action, current),
                expected[0],
                "cycle reset for {action:?}"
            );
        }
    }

    #[test]
    fn corners_cycle_width_without_leaving_their_vertical_half() {
        let ordinary = Rect::new(165.0, 245.0, 564.0, 384.0);
        let cycles = [
            (
                PlaceAction::UpperLeft,
                [
                    Rect::new(0.0, 441.0, 720.0, 436.0),
                    Rect::new(0.0, 441.0, 960.0, 436.0),
                    Rect::new(0.0, 441.0, 480.0, 436.0),
                ],
            ),
            (
                PlaceAction::LowerLeft,
                [
                    Rect::new(0.0, 4.0, 720.0, 436.0),
                    Rect::new(0.0, 4.0, 960.0, 436.0),
                    Rect::new(0.0, 4.0, 480.0, 436.0),
                ],
            ),
            (
                PlaceAction::UpperRight,
                [
                    Rect::new(720.0, 441.0, 720.0, 436.0),
                    Rect::new(480.0, 441.0, 960.0, 436.0),
                    Rect::new(960.0, 441.0, 480.0, 436.0),
                ],
            ),
            (
                PlaceAction::LowerRight,
                [
                    Rect::new(720.0, 4.0, 720.0, 436.0),
                    Rect::new(480.0, 4.0, 960.0, 436.0),
                    Rect::new(960.0, 4.0, 480.0, 436.0),
                ],
            ),
        ];

        for (action, expected) in cycles {
            let mut current = ordinary;
            for want in expected {
                current = calc(action, current);
                assert_eq!(current, want, "cycle step for {action:?}");
            }
            assert_eq!(
                calc(action, current),
                expected[0],
                "cycle reset for {action:?}"
            );
        }
    }

    #[test]
    fn next_and_previous_third_walk_are_exact_inverses() {
        let expected = [
            Rect::new(0.0, 4.0, 480.0, 873.0),
            Rect::new(480.0, 4.0, 480.0, 873.0),
            Rect::new(960.0, 4.0, 480.0, 873.0),
            Rect::new(0.0, 586.0, 1440.0, 291.0),
            Rect::new(0.0, 295.0, 1440.0, 291.0),
            Rect::new(0.0, 4.0, 1440.0, 291.0),
        ];

        let mut current = Rect::new(165.0, 245.0, 564.0, 384.0);
        for want in expected {
            current = calc(PlaceAction::NextThird, current);
            assert_eq!(current, want);
        }
        assert_eq!(calc(PlaceAction::NextThird, current), expected[0]);

        current = expected[0];
        for want in expected.into_iter().rev() {
            current = calc(PlaceAction::PreviousThird, current);
            assert_eq!(current, want);
        }
        assert_eq!(current, expected[0]);
    }

    #[test]
    fn display_walk_uses_each_distinct_destination_visible_frame() {
        let primary = Screen {
            visible: Rect::new(0.0, 24.0, 1440.0, 876.0),
            frame: Rect::new(0.0, 0.0, 1440.0, 900.0),
        };
        let secondary = Screen {
            visible: Rect::new(1440.0, 30.0, 1920.0, 1050.0),
            frame: Rect::new(1440.0, 0.0, 1920.0, 1080.0),
        };
        let screens = [primary, secondary];
        let on_primary = Rect::new(100.0, 100.0, 600.0, 400.0);
        let on_secondary = Rect::new(1800.0, 200.0, 800.0, 500.0);

        let secondary_center = Rect::new(2100.0, 355.0, 600.0, 400.0);
        assert_eq!(
            place(PlaceAction::NextDisplay, on_primary, &screens),
            Some(secondary_center)
        );
        assert_eq!(
            place(PlaceAction::PreviousDisplay, on_primary, &screens),
            Some(secondary_center)
        );

        let primary_center = Rect::new(320.0, 212.0, 800.0, 500.0);
        assert_eq!(
            place(PlaceAction::NextDisplay, on_secondary, &screens),
            Some(primary_center)
        );
        assert_eq!(
            place(PlaceAction::PreviousDisplay, on_secondary, &screens),
            Some(primary_center)
        );
    }

    #[test]
    fn display_walk_fullscreens_a_window_that_does_not_fit_destination() {
        let screens = [
            Screen {
                visible: Rect::new(0.0, 0.0, 2000.0, 1200.0),
                frame: Rect::new(0.0, 0.0, 2000.0, 1200.0),
            },
            Screen {
                visible: Rect::new(2000.0, 20.0, 1000.0, 700.0),
                frame: Rect::new(2000.0, 0.0, 1000.0, 720.0),
            },
        ];
        assert_eq!(
            place(
                PlaceAction::NextDisplay,
                Rect::new(100.0, 100.0, 1400.0, 900.0),
                &screens,
            ),
            Some(screens[1].visible)
        );
    }

    #[test]
    fn resize_keeps_attached_edges_and_refuses_too_small_result() {
        eq(
            calc(PlaceAction::Larger, Rect::new(238.0, 4.0, 720.0, 436.0)),
            223.0,
            4.0,
            750.0,
            466.0,
        );
        eq(
            calc(PlaceAction::Larger, Rect::new(720.0, 303.0, 720.0, 436.0)),
            690.0,
            288.0,
            750.0,
            466.0,
        );

        let minimum = Rect::new(540.0, 331.0, 360.0, 219.0);
        assert_eq!(calc(PlaceAction::Smaller, minimum), minimum);
    }

    #[test]
    fn missing_geometry_context_and_deferred_history_fail_without_a_rect() {
        let window = Rect::new(100.0, 100.0, 600.0, 400.0);
        assert_eq!(place(PlaceAction::Center, window, &[]), None);
        assert_eq!(place(PlaceAction::NextDisplay, window, &screen()), None);
        assert_eq!(place(PlaceAction::PreviousDisplay, window, &screen()), None);
        assert_eq!(place(PlaceAction::Undo, window, &screen()), None);
        assert_eq!(place(PlaceAction::Redo, window, &screen()), None);

        // Undo/redo require per-application history and are intentionally not
        // fabricated by this stateless geometry module. Native application
        // constraints and size/position actuation are also outside its scope.
    }
}
