//! Shared file-font rasterizer for Unix adapters.

use std::{fs, sync::OnceLock};

use ab_glyph::{Font, FontArc, FontRef, GlyphId, PxScale, ScaleFont};

use crate::contract::font::{FontError, FontFileCandidate, RasterGlyph};

const MAX_COLLECTION_FACES: u32 = 32;
const MAX_GLYPH_DIM: u32 = 4096;

struct Face {
    name: &'static str,
    font: FontArc,
}

struct Renderer {
    faces: Vec<Face>,
}

type CandidateSource = fn() -> &'static [FontFileCandidate];

fn renderer(primary: CandidateSource, fallback: CandidateSource) -> &'static Renderer {
    static RENDERER: OnceLock<Renderer> = OnceLock::new();
    RENDERER.get_or_init(|| Renderer {
        faces: load_faces(primary(), fallback()),
    })
}

fn load_faces(primary: &[FontFileCandidate], fallback: &[FontFileCandidate]) -> Vec<Face> {
    let mut faces = Vec::new();
    for &candidate in primary {
        if push_faces(&mut faces, candidate) {
            break;
        }
    }
    for &candidate in fallback {
        push_faces(&mut faces, candidate);
    }
    faces
}

fn push_faces(faces: &mut Vec<Face>, candidate: FontFileCandidate) -> bool {
    let Ok(data) = fs::read(candidate.absolute_path()) else {
        return false;
    };
    let leaked: &'static [u8] = Box::leak(data.into_boxed_slice());
    let before = faces.len();
    for index in 0..MAX_COLLECTION_FACES {
        match FontRef::try_from_slice_and_index(leaked, index) {
            Ok(font) => faces.push(Face {
                name: candidate.name,
                font: FontArc::from(font),
            }),
            Err(_) => break,
        }
    }
    faces.len() != before
}

pub(crate) fn rasterizer_name(
    primary: CandidateSource,
    fallback: CandidateSource,
) -> Result<String, FontError> {
    renderer(primary, fallback)
        .faces
        .first()
        .map(|face| face.name.to_owned())
        .ok_or(FontError::Unavailable)
}

pub(crate) fn rasterize(
    primary: CandidateSource,
    fallback: CandidateSource,
    ch: char,
    size_px: u16,
) -> Result<Option<RasterGlyph>, FontError> {
    let size_px = size_px.clamp(8, 72);
    let renderer = renderer(primary, fallback);
    let Some(face) = renderer
        .faces
        .iter()
        .find(|face| face.font.glyph_id(ch) != GlyphId(0))
    else {
        return Ok(None);
    };
    let scaled = face.font.as_scaled(f32::from(size_px));
    let glyph_id = scaled.glyph_id(ch);
    let Some(outlined) =
        scaled.outline_glyph(glyph_id.with_scale(PxScale::from(f32::from(size_px))))
    else {
        return Ok(None);
    };
    let bounds = outlined.px_bounds();
    let width = bounded_dimension(bounds.width())?;
    let height = bounded_dimension(bounds.height())?;
    let len = (width as usize)
        .checked_mul(height as usize)
        .ok_or(FontError::GlyphTooLarge)?;
    let mut alpha = vec![0u8; len];
    outlined.draw(|x, y, coverage| {
        if x < width && y < height {
            alpha[(y * width + x) as usize] = (coverage.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    });
    Ok(Some(RasterGlyph {
        alpha,
        width,
        height,
        offset_x: bounds.min.x.round() as i32,
        offset_y: (scaled.ascent() + bounds.min.y).round() as i32,
    }))
}

fn bounded_dimension(value: f32) -> Result<u32, FontError> {
    if !value.is_finite() || value < 0.0 || value > MAX_GLYPH_DIM as f32 {
        return Err(FontError::GlyphTooLarge);
    }
    Ok(value as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyph_dimensions_reject_non_finite_and_oversized_values() {
        assert_eq!(bounded_dimension(42.0), Ok(42));
        assert_eq!(bounded_dimension(f32::NAN), Err(FontError::GlyphTooLarge));
        assert_eq!(
            bounded_dimension(1_000_000.0),
            Err(FontError::GlyphTooLarge)
        );
    }
}
