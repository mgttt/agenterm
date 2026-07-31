//! Typed XRGB screenshot encoding contract for native frontend projections.

use std::path::Path;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct XrgbClip {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum UiScreenshotError {
    Unsupported {
        reason: &'static str,
    },
    #[allow(dead_code)] // Constructed by Unix adapters only.
    Failed {
        code: &'static str,
        message: String,
    },
}

impl UiScreenshotError {
    #[allow(dead_code)] // Consumed by the Unix frontend compatibility projection.
    pub(crate) fn message(&self) -> String {
        match self {
            Self::Unsupported { reason } => format!("screenshot unsupported: {reason}"),
            Self::Failed { message, .. } => message.clone(),
        }
    }
}

#[allow(dead_code)] // Fields are read by Unix adapters only.
pub(crate) struct XrgbFrame<'a> {
    pub(crate) path: &'a Path,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixels: &'a [u32],
    pub(crate) clip: Option<XrgbClip>,
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn frame_and_typed_failure_preserve_requested_geometry() {
        let pixels = [0_u32; 4];
        let frame = XrgbFrame {
            path: Path::new("capture.png"),
            width: 2,
            height: 2,
            pixels: &pixels,
            clip: Some(XrgbClip {
                x: 1,
                y: 1,
                width: 1,
                height: 1,
            }),
        };
        assert_eq!(frame.path, Path::new("capture.png"));
        assert_eq!((frame.width, frame.height, frame.pixels.len()), (2, 2, 4));
        assert_eq!(frame.clip.expect("clip").width, 1);
        assert_eq!(
            UiScreenshotError::Failed {
                code: "screenshot_invalid_clip",
                message: "outside frame".to_owned(),
            }
            .message(),
            "outside frame"
        );
    }
}
