use std::path::Path;

use agenterm_chassis::{ChassisError, check_product_image, inspect};

#[cfg(feature = "loader")]
mod native;

/// A composed image that passed the complete L1/L2/L3 layout check.
///
/// Construction is private so a presenter cannot accidentally receive an
/// unchecked image.
#[derive(Debug)]
pub struct LoadedImage {
    report: serde_json::Value,
}

impl LoadedImage {
    pub fn l3_name(&self) -> &str {
        self.report["l3_name"].as_str().unwrap_or("unnamed")
    }

    pub fn capability_count(&self) -> usize {
        self.report["l3_capabilities"]
            .as_array()
            .map_or(0, Vec::len)
    }
}

/// Load and inspect an unpacked composed image without opening a window.
///
/// `check_layout` rejects undeclared capabilities and native doors in L3.
/// Only after that check succeeds is the inspected image wrapped in the
/// unforgeable [`LoadedImage`] state accepted by native presentation.
pub fn load_image(root: &Path) -> Result<LoadedImage, ChassisError> {
    check_product_image(root)?;
    let report = inspect(root)?;
    Ok(LoadedImage { report })
}

#[derive(Debug)]
pub enum LoadThenError<E> {
    Image(ChassisError),
    Present(E),
}

impl<E: std::fmt::Display> std::fmt::Display for LoadThenError<E> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Image(error) => write!(formatter, "image rejected: {error}"),
            Self::Present(error) => write!(formatter, "image presentation failed: {error}"),
        }
    }
}

impl<E> std::error::Error for LoadThenError<E> where E: std::error::Error + 'static {}

/// Enforce load-before-present ordering while allowing headless verification.
pub fn load_then<T, E>(
    root: &Path,
    present: impl FnOnce(LoadedImage) -> Result<T, E>,
) -> Result<T, LoadThenError<E>> {
    let image = load_image(root).map_err(LoadThenError::Image)?;
    present(image).map_err(LoadThenError::Present)
}

#[cfg(feature = "loader")]
pub fn present_image(
    image: LoadedImage,
) -> Result<(), agenterm_platform::window_host::PixelWindowError> {
    native::present(image)
}
