//! Apply pipeline: write, quantized shrink, clamp to visible frame.

use agenterm_platform::{
    window_enumerate::{ScreenInfo, WindowBounds},
    window_op::{self, WindowOpError},
};

use super::geometry::Rect;

pub fn apply_rect(
    handle: isize,
    target: Rect,
    visible: Rect,
) -> Result<(Rect, bool, bool), WindowOpError> {
    let (x, y, w, h) = target.to_i32();
    window_op::move_window(handle, x, y, w, h)?;
    let mut quantized = false;
    let mut actual = read_rect(handle)?;
    if actual.width > target.width + 1.0 || actual.height > target.height + 1.0 {
        quantized = true;
        let mut adjusted = target;
        while (actual.width > target.width || actual.height > target.height)
            && adjusted.width > target.width * 0.85
            && adjusted.height > target.height * 0.85
        {
            if actual.width > target.width {
                adjusted.width -= 2.0;
            }
            if actual.height > target.height {
                adjusted.height -= 2.0;
            }
            let (ax, ay, aw, ah) = adjusted.to_i32();
            window_op::move_window(handle, ax, ay, aw, ah)?;
            actual = read_rect(handle)?;
        }
        adjusted.x += ((target.width - actual.width) / 2.0).floor();
        adjusted.y += ((target.height - actual.height) / 2.0).floor();
        let (ax, ay, aw, ah) = adjusted.to_i32();
        window_op::move_window(handle, ax, ay, aw, ah)?;
        actual = read_rect(handle)?;
    }
    let clamped = clamp(actual, visible);
    let did_clamp = !clamped.almost_eq(actual);
    if did_clamp {
        let (cx, cy, cw, ch) = clamped.to_i32();
        window_op::move_window(handle, cx, cy, cw, ch)?;
        actual = read_rect(handle)?;
    }
    Ok((actual, quantized, did_clamp))
}

pub fn read_rect(handle: isize) -> Result<Rect, WindowOpError> {
    let bounds = window_op::window_rect(handle)?;
    Ok(rect_from_bounds(bounds))
}

pub fn rect_from_bounds(bounds: WindowBounds) -> Rect {
    Rect::new(
        f64::from(bounds.x),
        f64::from(bounds.y),
        f64::from(bounds.width),
        f64::from(bounds.height),
    )
}

pub fn screen_from_info(info: &ScreenInfo) -> super::geometry::Screen {
    super::geometry::Screen {
        visible: rect_from_bounds(info.visible),
        frame: rect_from_bounds(info.frame),
    }
}

fn clamp(window: Rect, visible: Rect) -> Rect {
    let mut out = window;
    if out.x < visible.x {
        out.x = visible.x;
    } else if out.max_x() > visible.max_x() {
        out.x = visible.max_x() - out.width;
    }
    if out.y < visible.y {
        out.y = visible.y;
    } else if out.max_y() > visible.max_y() {
        out.y = visible.max_y() - out.height;
    }
    out
}
