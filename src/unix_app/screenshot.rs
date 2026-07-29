use std::{fs::File, io::BufWriter, path::Path};

use png::{BitDepth, ColorType, Encoder};

/// Encode softbuffer `0RGB`/`XRGB` pixels (little-endian `0x00RRGGBB`) as PNG.
pub(super) fn write_xrgb_png(
    path: &Path,
    width: u32,
    height: u32,
    pixels: &[u32],
    clip: Option<(u32, u32, u32, u32)>,
) -> Result<(), String> {
    let (x0, y0, w, h) = match clip {
        Some((x, y, w, h)) => (
            x.min(width.saturating_sub(1)),
            y.min(height.saturating_sub(1)),
            w.min(width.saturating_sub(x)).max(1),
            h.min(height.saturating_sub(y)).max(1),
        ),
        None => (0, 0, width.max(1), height.max(1)),
    };
    if pixels.len() < (width as usize).saturating_mul(height as usize) {
        return Err("pixel buffer is smaller than the declared dimensions".to_owned());
    }

    let mut rgba = Vec::with_capacity((w as usize) * (h as usize) * 4);
    for row in y0..y0 + h {
        let row_start = (row as usize) * (width as usize);
        for col in x0..x0 + w {
            let pixel = pixels[row_start + col as usize] & 0x00FF_FFFF;
            let r = ((pixel >> 16) & 0xFF) as u8;
            let g = ((pixel >> 8) & 0xFF) as u8;
            let b = (pixel & 0xFF) as u8;
            rgba.extend_from_slice(&[r, g, b, 255]);
        }
    }

    let file = File::create(path).map_err(|error| error.to_string())?;
    let mut encoder = Encoder::new(BufWriter::new(file), w, h);
    encoder.set_color(ColorType::Rgba);
    encoder.set_depth(BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|error| error.to_string())?
        .into_stream_writer()
        .map_err(|error| error.to_string())?;
    use std::io::Write;
    writer.write_all(&rgba).map_err(|error| error.to_string())?;
    writer.finish().map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::write_xrgb_png;
    use std::path::PathBuf;

    #[test]
    fn write_xrgb_png_emits_readable_file() {
        let path = PathBuf::from(std::env::temp_dir()).join("agenterm-unix-screenshot-test.png");
        let pixels = [0x00FF00u32, 0x0000FFu32, 0xFF0000u32, 0xFFFFFFu32];
        write_xrgb_png(&path, 2, 2, &pixels, None).expect("png write");
        let meta = std::fs::metadata(&path).expect("meta");
        assert!(meta.len() > 32);
        let _ = std::fs::remove_file(path);
    }
}
