use std::{fs::File, io::BufReader, path::Path};

use rhai::{Engine, EvalAltResult, Module};

use crate::{script_error::runtime_error, script_stdlib::ScriptPath};

const MAX_DECODED_PNG_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct ScriptPngInfo {
    width: i64,
    height: i64,
    samples: i64,
    red: f64,
    green: f64,
    blue: f64,
    luminance: f64,
}

pub(crate) fn register(engine: &mut Engine, rhai_module: &mut Module) {
    engine.register_type_with_name::<ScriptPngInfo>("PngInfo");
    engine.register_get("width", |info: &mut ScriptPngInfo| info.width);
    engine.register_get("height", |info: &mut ScriptPngInfo| info.height);
    engine.register_get("samples", |info: &mut ScriptPngInfo| info.samples);
    engine.register_get("red", |info: &mut ScriptPngInfo| info.red);
    engine.register_get("green", |info: &mut ScriptPngInfo| info.green);
    engine.register_get("blue", |info: &mut ScriptPngInfo| info.blue);
    engine.register_get("luminance", |info: &mut ScriptPngInfo| info.luminance);

    let mut image = Module::new();
    image.set_native_fn("inspect_png", inspect_png_text);
    image.set_native_fn("inspect_png", inspect_png_path);
    rhai_module.set_sub_module("image", image);
}

fn inspect_png_text(path: &str) -> Result<ScriptPngInfo, Box<EvalAltResult>> {
    inspect_png(Path::new(path))
}

fn inspect_png_path(path: ScriptPath) -> Result<ScriptPngInfo, Box<EvalAltResult>> {
    inspect_png(&path.0)
}

fn inspect_png(path: &Path) -> Result<ScriptPngInfo, Box<EvalAltResult>> {
    let file = File::open(path).map_err(|_| {
        image_error(
            "image_png_open",
            "rh.image.inspect_png",
            "PNG file could not be opened",
            Some("io"),
        )
    })?;
    let mut decoder = png::Decoder::new(BufReader::new(file));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|_| {
        image_error(
            "image_png_header",
            "rh.image.inspect_png",
            "PNG header is invalid or unsupported",
            Some("invalid_data"),
        )
    })?;
    let decoded_bytes = reader.output_buffer_size();
    if decoded_bytes == 0 || decoded_bytes > MAX_DECODED_PNG_BYTES {
        return Err(image_error(
            "image_png_size",
            "rh.image.inspect_png",
            "decoded PNG must contain 1..67108864 bytes",
            Some("size_limit"),
        ));
    }
    let mut pixels = vec![0; decoded_bytes];
    let frame = reader.next_frame(&mut pixels).map_err(|_| {
        image_error(
            "image_png_decode",
            "rh.image.inspect_png",
            "PNG pixels could not be decoded",
            Some("invalid_data"),
        )
    })?;
    pixels.truncate(frame.buffer_size());
    if frame.width == 0 || frame.height == 0 {
        return Err(image_error(
            "image_png_dimensions",
            "rh.image.inspect_png",
            "PNG dimensions must be nonzero",
            Some("invalid_data"),
        ));
    }

    let channels = match frame.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => {
            return Err(image_error(
                "image_png_color",
                "rh.image.inspect_png",
                "expanded PNG retained an indexed color type",
                Some("invalid_data"),
            ));
        }
    };
    let width = frame.width as usize;
    let height = frame.height as usize;
    let row_bytes = width.checked_mul(channels).ok_or_else(|| {
        image_error(
            "image_png_size",
            "rh.image.inspect_png",
            "PNG row size overflowed",
            Some("size_limit"),
        )
    })?;
    if row_bytes.checked_mul(height) != Some(pixels.len()) {
        return Err(image_error(
            "image_png_size",
            "rh.image.inspect_png",
            "decoded PNG byte length does not match its dimensions",
            Some("invalid_data"),
        ));
    }

    let step_x = (width / 64).max(1);
    let step_y = (height / 64).max(1);
    let mut red = 0_u64;
    let mut green = 0_u64;
    let mut blue = 0_u64;
    let mut samples = 0_u64;
    for y in (0..height).step_by(step_y) {
        for x in (0..width).step_by(step_x) {
            let offset = y * row_bytes + x * channels;
            let (r, g, b) = match channels {
                1 | 2 => {
                    let value = pixels[offset];
                    (value, value, value)
                }
                3 | 4 => (pixels[offset], pixels[offset + 1], pixels[offset + 2]),
                _ => unreachable!("PNG channel count was matched above"),
            };
            red += u64::from(r);
            green += u64::from(g);
            blue += u64::from(b);
            samples += 1;
        }
    }
    let divisor = samples as f64;
    let red = red as f64 / divisor;
    let green = green as f64 / divisor;
    let blue = blue as f64 / divisor;
    Ok(ScriptPngInfo {
        width: i64::from(frame.width),
        height: i64::from(frame.height),
        samples: samples as i64,
        red,
        green,
        blue,
        luminance: 0.2126 * red + 0.7152 * green + 0.0722 * blue,
    })
}

pub(crate) fn inspect_png_json(path: &Path) -> Result<serde_json::Value, String> {
    let info = inspect_png(path).map_err(|error| error.to_string())?;
    Ok(serde_json::json!({
        "width": info.width,
        "height": info.height,
        "samples": info.samples,
        "red": info.red,
        "green": info.green,
        "blue": info.blue,
        "luminance": info.luminance,
    }))
}

fn image_error(
    code: &'static str,
    operation: &'static str,
    message: &'static str,
    cause: Option<&'static str>,
) -> Box<EvalAltResult> {
    runtime_error(
        "image", code, operation, message, false, "png", false, cause,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_inspection_reports_dimensions_and_sampled_color() {
        let path = std::env::temp_dir().join(format!(
            "agenterm-rh-image-{}-{}.png",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let file = File::create(&path).unwrap();
        let mut encoder = png::Encoder::new(file, 2, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&[255, 0, 0, 255, 0, 255, 0, 255])
            .unwrap();
        writer.finish().unwrap();

        let info = inspect_png(&path).unwrap();
        assert_eq!(info.width, 2);
        assert_eq!(info.height, 1);
        assert_eq!(info.samples, 2);
        assert_eq!(info.red, 127.5);
        assert_eq!(info.green, 127.5);
        assert_eq!(info.blue, 0.0);
        assert!((info.luminance - 118.2945).abs() < 0.001);
        let json = inspect_png_json(&path).unwrap();
        assert_eq!(json["width"], 2);
        assert_eq!(json["height"], 1);
        assert!((json["luminance"].as_f64().unwrap() - 118.2945).abs() < 0.001);
        std::fs::remove_file(path).unwrap();
    }
}
