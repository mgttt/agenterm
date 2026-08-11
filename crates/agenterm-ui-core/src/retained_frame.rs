//! Host-neutral retained XRGB frame storage.
//!
//! The product owns this buffer. A native host may expose or resize its own
//! surface at any time, so the host buffer is never treated as retained state.

use std::error::Error;
use std::fmt;

const MAX_RETAINED_PIXELS: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetainedFrameError {
    LengthOverflow {
        width: u32,
        height: u32,
    },
    CapacityExceeded {
        requested: usize,
        maximum: usize,
    },
    AllocationFailed {
        requested: usize,
    },
    DimensionMismatch {
        expected_width: u32,
        expected_height: u32,
        actual_width: u32,
        actual_height: u32,
    },
    BufferLengthMismatch {
        expected: usize,
        actual: usize,
    },
    NotValid,
}

impl fmt::Display for RetainedFrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LengthOverflow { width, height } => {
                write!(f, "retained frame length overflow for {width}x{height}")
            }
            Self::CapacityExceeded { requested, maximum } => write!(
                f,
                "retained frame requests {requested} pixels, limit is {maximum}"
            ),
            Self::AllocationFailed { requested } => {
                write!(f, "retained frame allocation failed for {requested} pixels")
            }
            Self::DimensionMismatch {
                expected_width,
                expected_height,
                actual_width,
                actual_height,
            } => write!(
                f,
                "retained frame is {actual_width}x{actual_height}, expected {expected_width}x{expected_height}"
            ),
            Self::BufferLengthMismatch { expected, actual } => {
                write!(f, "host frame has {actual} pixels, expected {expected}")
            }
            Self::NotValid => f.write_str("retained frame is not valid"),
        }
    }
}

impl Error for RetainedFrameError {}

#[derive(Debug, Default)]
pub struct RetainedXrgbFrame {
    pixels: Vec<u32>,
    width: u32,
    height: u32,
    valid: bool,
}

impl RetainedXrgbFrame {
    pub const fn new() -> Self {
        Self {
            pixels: Vec::new(),
            width: 0,
            height: 0,
            valid: false,
        }
    }

    /// Ensures storage for one frame and returns whether the caller must do a
    /// full raster. Invalid storage and every dimension change require full.
    pub fn prepare(&mut self, width: u32, height: u32) -> Result<bool, RetainedFrameError> {
        let length = checked_pixel_len(width, height)?;
        if self.width == width && self.height == height && self.pixels.len() == length {
            return Ok(!self.valid);
        }

        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(length)
            .map_err(|_| RetainedFrameError::AllocationFailed { requested: length })?;
        pixels.resize(length, 0);
        self.pixels = pixels;
        self.width = width;
        self.height = height;
        self.valid = false;
        Ok(true)
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn is_valid(&self) -> bool {
        self.valid
    }

    pub fn pixels(&self) -> &[u32] {
        &self.pixels
    }

    pub fn pixels_mut(&mut self) -> &mut [u32] {
        &mut self.pixels
    }

    pub fn mark_valid(&mut self) {
        self.valid = true;
    }

    pub fn invalidate(&mut self) {
        self.valid = false;
    }

    /// Copies a valid product frame into the transient host frame. The exact
    /// length check prevents a mismatched host surface from causing a slice
    /// panic or leaving a partially copied frame visible.
    pub fn copy_to(
        &self,
        destination: &mut [u32],
        width: u32,
        height: u32,
    ) -> Result<(), RetainedFrameError> {
        let expected = checked_pixel_len(width, height)?;
        if self.width != width || self.height != height {
            return Err(RetainedFrameError::DimensionMismatch {
                expected_width: self.width,
                expected_height: self.height,
                actual_width: width,
                actual_height: height,
            });
        }
        if self.pixels.len() != expected || destination.len() != expected {
            return Err(RetainedFrameError::BufferLengthMismatch {
                expected,
                actual: destination.len(),
            });
        }
        if !self.valid {
            return Err(RetainedFrameError::NotValid);
        }
        destination.copy_from_slice(&self.pixels);
        Ok(())
    }
}

fn checked_pixel_len(width: u32, height: u32) -> Result<usize, RetainedFrameError> {
    let original_width = width;
    let original_height = height;
    let width = usize::try_from(width).map_err(|_| RetainedFrameError::LengthOverflow {
        width: original_width,
        height: original_height,
    })?;
    let height = usize::try_from(height).map_err(|_| RetainedFrameError::LengthOverflow {
        width: original_width,
        height: original_height,
    })?;
    let length = width
        .checked_mul(height)
        .ok_or(RetainedFrameError::LengthOverflow {
            width: original_width,
            height: original_height,
        })?;
    if length > MAX_RETAINED_PIXELS {
        return Err(RetainedFrameError::CapacityExceeded {
            requested: length,
            maximum: MAX_RETAINED_PIXELS,
        });
    }
    Ok(length)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_frame_requires_full_and_same_valid_frame_can_be_partial() {
        let mut frame = RetainedXrgbFrame::new();
        assert!(frame.prepare(2, 2).expect("small frame is valid"));
        frame.pixels_mut().fill(0x0011_2233);
        frame.mark_valid();
        assert!(!frame.prepare(2, 2).expect("same dimensions are valid"));

        let mut host = vec![0; 4];
        frame.copy_to(&mut host, 2, 2).expect("copy succeeds");
        assert_eq!(host, vec![0x0011_2233; 4]);
    }

    #[test]
    fn partial_update_preserves_pixels_outside_the_candidate() {
        let mut frame = RetainedXrgbFrame::new();
        assert!(frame.prepare(3, 2).expect("small frame is valid"));
        frame.pixels_mut().fill(7);
        frame.pixels_mut()[1] = 9;
        frame.mark_valid();
        assert_eq!(frame.pixels(), &[7, 9, 7, 7, 7, 7]);
    }

    #[test]
    fn resize_invalidates_and_requires_full() {
        let mut frame = RetainedXrgbFrame::new();
        assert!(frame.prepare(2, 1).expect("small frame is valid"));
        frame.mark_valid();
        assert!(frame.prepare(3, 1).expect("small resize is valid"));
        assert!(!frame.is_valid());
    }

    #[test]
    fn zero_size_frame_is_safe_and_empty_copy_works() {
        let mut frame = RetainedXrgbFrame::new();
        assert!(frame.prepare(0, 0).expect("zero frame is valid"));
        frame.mark_valid();
        frame.copy_to(&mut [], 0, 0).expect("empty copy succeeds");
    }

    #[test]
    fn oversized_frame_returns_typed_error_before_allocation() {
        let mut frame = RetainedXrgbFrame::new();
        let error = frame
            .prepare(u32::MAX, u32::MAX)
            .expect_err("must be bounded");
        assert!(matches!(
            error,
            RetainedFrameError::CapacityExceeded { .. } | RetainedFrameError::LengthOverflow { .. }
        ));
    }

    #[test]
    fn copy_rejects_invalid_or_mismatched_host_storage() {
        let mut frame = RetainedXrgbFrame::new();
        frame.prepare(2, 1).expect("small frame is valid");
        assert_eq!(
            frame.copy_to(&mut [0; 1], 2, 1),
            Err(RetainedFrameError::BufferLengthMismatch {
                expected: 2,
                actual: 1,
            })
        );
        frame.mark_valid();
        assert_eq!(
            frame.copy_to(&mut [0; 1], 2, 1),
            Err(RetainedFrameError::BufferLengthMismatch {
                expected: 2,
                actual: 1,
            })
        );
    }
}
