use std::path::Path;

/// Encode softbuffer `0RGB`/`XRGB` pixels (little-endian `0x00RRGGBB`) as PNG.
#[cfg(target_os = "macos")]
pub(super) fn write_xrgb_png(
    path: &Path,
    width: u32,
    height: u32,
    pixels: &[u32],
    clip: Option<(u32, u32, u32, u32)>,
) -> Result<(), String> {
    crate::platform::macos::screenshot::write_xrgb_png(path, width, height, pixels, clip)
        .map_err(|error| error.message())
}

/// Encode softbuffer `0RGB`/`XRGB` pixels (little-endian `0x00RRGGBB`) as PNG.
#[cfg(target_os = "linux")]
pub(super) fn write_xrgb_png(
    path: &Path,
    width: u32,
    height: u32,
    pixels: &[u32],
    clip: Option<(u32, u32, u32, u32)>,
) -> Result<(), String> {
    use crate::platform::linux::screenshot::{ScreenshotClip, write_xrgb_png as write_linux};
    let clip = clip.map(|(x, y, width, height)| ScreenshotClip {
        x,
        y,
        width,
        height,
    });
    write_linux(path, width, height, pixels, clip)
        .map(|_| ())
        .map_err(|error| error.message())
}

#[cfg(test)]
mod tests {
    use super::write_xrgb_png;

    #[test]
    fn write_xrgb_png_emits_readable_file() {
        let path = std::env::temp_dir().join("agenterm-unix-screenshot-test.png");
        let pixels = [0x00FF00u32, 0x0000FFu32, 0xFF0000u32, 0xFFFFFFu32];
        write_xrgb_png(&path, 2, 2, &pixels, None).expect("png write");
        let meta = std::fs::metadata(&path).expect("meta");
        assert!(meta.len() > 32);
        let _ = std::fs::remove_file(path);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_screenshot_delegates_to_platform_capability_boundary() {
        use crate::platform::{CapabilityKind, CapabilityStatus};
        let status = crate::platform::linux::capability_status(CapabilityKind::Screenshot);
        assert!(matches!(
            status,
            CapabilityStatus::Available
                | CapabilityStatus::Unsupported {
                    reason: "headless-display"
                }
        ));
    }
}
