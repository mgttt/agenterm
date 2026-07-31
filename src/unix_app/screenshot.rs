use std::path::Path;

use crate::platform::contract::ui_screenshot::{XrgbClip, XrgbFrame};

/// Encode softbuffer `0RGB`/`XRGB` pixels (little-endian `0x00RRGGBB`) as PNG.
pub(crate) fn write_xrgb_png(
    path: &Path,
    width: u32,
    height: u32,
    pixels: &[u32],
    clip: Option<(u32, u32, u32, u32)>,
) -> Result<(), String> {
    crate::platform::services::ui_screenshot::write_xrgb_png(XrgbFrame {
        path,
        width,
        height,
        pixels,
        clip: clip.map(|(x, y, width, height)| XrgbClip {
            x,
            y,
            width,
            height,
        }),
    })
    .map_err(|error| error.message())
}
