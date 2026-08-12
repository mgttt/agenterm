use std::{cell::RefCell, mem, ptr};

#[cfg(test)]
use std::cell::Cell;

use windows_sys::Win32::Graphics::Gdi::{
    ANTIALIASED_QUALITY, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateCompatibleDC, CreateFontW,
    DEFAULT_CHARSET, DeleteDC, DeleteObject, FF_MODERN, FIXED, FIXED_PITCH, FW_NORMAL, GDI_ERROR,
    GGI_MARK_NONEXISTING_GLYPHS, GGO_GLYPH_INDEX, GGO_GRAY8_BITMAP, GLYPHMETRICS, GetDC,
    GetDeviceCaps, GetGlyphIndicesW, GetGlyphOutlineW, GetTextFaceW, GetTextMetricsW, HGDIOBJ,
    LOGPIXELSY, MAT2, OUT_DEFAULT_PRECIS, ReleaseDC, SelectObject, TEXTMETRICW,
};

use crate::contract::font::{
    FontDiscovery, FontError, FontFileCandidate, FontMetrics, FontRequest, OpaqueWindowHandle,
    RasterGlyph,
};

const MAX_GLYPH_DIM: u32 = 4096;
const MAX_GLYPH_BYTES: u32 = MAX_GLYPH_DIM * MAX_GLYPH_DIM;
const RASTER_FAMILIES: &[&str] = &[
    "NSimSun",
    "Sarasa Fixed SC",
    "Cascadia Mono",
    "Consolas",
    "Microsoft YaHei",
    "MS Gothic",
    "Malgun Gothic",
    "Segoe UI Symbol",
    "Segoe UI Emoji",
];

thread_local! {
    static RASTER_FACES: RefCell<RasterFaces> = const { RefCell::new(RasterFaces::empty()) };
}

#[cfg(test)]
thread_local! {
    static FACE_CREATIONS: Cell<usize> = const { Cell::new(0) };
}

struct RasterFaces {
    size_px: u16,
    faces: [Option<PixelFace>; RASTER_FAMILIES.len()],
}

impl RasterFaces {
    const fn empty() -> Self {
        Self {
            size_px: 0,
            faces: [const { None }; RASTER_FAMILIES.len()],
        }
    }

    fn reset(&mut self, size_px: u16) {
        if self.size_px != size_px {
            *self = Self {
                size_px,
                faces: std::array::from_fn(|_| None),
            };
        }
    }

    fn face(&mut self, index: usize) -> Result<&PixelFace, FontError> {
        if self.faces[index].is_none() {
            self.faces[index] = Some(PixelFace::create(RASTER_FAMILIES[index], self.size_px)?);
        }
        self.faces[index].as_ref().ok_or(FontError::CreateFailed)
    }
}

const PRIMARY_CANDIDATES: &[FontFileCandidate] = &[
    FontFileCandidate {
        name: "Sarasa Fixed SC",
        components: &["C:", "Windows", "Fonts", "sarasa-mono-sc-regular.ttf"],
    },
    FontFileCandidate {
        name: "Sarasa Fixed SC",
        components: &["C:", "Windows", "Fonts", "sarasaMonoSC-Regular.ttf"],
    },
    FontFileCandidate {
        name: "Cascadia Code",
        components: &["C:", "Windows", "Fonts", "cascadia.ttf"],
    },
    FontFileCandidate {
        name: "Cascadia Mono",
        components: &["C:", "Windows", "Fonts", "cascadiamono.ttf"],
    },
    FontFileCandidate {
        name: "Consolas",
        components: &["C:", "Windows", "Fonts", "consola.ttf"],
    },
    FontFileCandidate {
        name: "Courier New",
        components: &["C:", "Windows", "Fonts", "cour.ttf"],
    },
];

const FALLBACK_CANDIDATES: &[FontFileCandidate] = &[
    FontFileCandidate {
        name: "SimSun / NSimSun",
        components: &["C:", "Windows", "Fonts", "simsun.ttc"],
    },
    FontFileCandidate {
        name: "Microsoft YaHei",
        components: &["C:", "Windows", "Fonts", "msyh.ttc"],
    },
    FontFileCandidate {
        name: "MS Gothic",
        components: &["C:", "Windows", "Fonts", "msgothic.ttc"],
    },
    FontFileCandidate {
        name: "Malgun Gothic",
        components: &["C:", "Windows", "Fonts", "malgun.ttf"],
    },
    FontFileCandidate {
        name: "Segoe UI Emoji",
        components: &["C:", "Windows", "Fonts", "seguiemj.ttf"],
    },
];

struct PixelFace {
    dc: *mut core::ffi::c_void,
    font: HGDIOBJ,
    previous: HGDIOBJ,
    metrics: TEXTMETRICW,
}

impl Drop for PixelFace {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.previous);
            DeleteObject(self.font);
            DeleteDC(self.dc);
        }
    }
}

impl PixelFace {
    fn create(family: &str, size_px: u16) -> Result<Self, FontError> {
        if family.is_empty() || size_px == 0 {
            return Err(FontError::InvalidRequest);
        }
        let dc = unsafe { CreateCompatibleDC(ptr::null_mut()) };
        if dc.is_null() {
            return Err(FontError::DeviceContextUnavailable);
        }
        let family: Vec<u16> = family.encode_utf16().chain(std::iter::once(0)).collect();
        let font = unsafe {
            CreateFontW(
                -i32::from(size_px),
                0,
                0,
                0,
                FW_NORMAL as i32,
                0,
                0,
                0,
                u32::from(DEFAULT_CHARSET),
                u32::from(OUT_DEFAULT_PRECIS),
                u32::from(CLIP_DEFAULT_PRECIS),
                u32::from(ANTIALIASED_QUALITY),
                u32::from(FIXED_PITCH | FF_MODERN),
                family.as_ptr(),
            )
        } as HGDIOBJ;
        if font.is_null() {
            unsafe { DeleteDC(dc) };
            return Err(FontError::CreateFailed);
        }
        let previous = unsafe { SelectObject(dc, font) };
        if previous.is_null() {
            unsafe {
                DeleteObject(font);
                DeleteDC(dc);
            }
            return Err(FontError::CreateFailed);
        }
        let mut metrics = TEXTMETRICW::default();
        if unsafe { GetTextMetricsW(dc, &mut metrics) } == 0 {
            unsafe {
                SelectObject(dc, previous);
                DeleteObject(font);
                DeleteDC(dc);
            }
            return Err(FontError::MetricsFailed);
        }
        #[cfg(test)]
        FACE_CREATIONS.with(|count| count.set(count.get() + 1));
        Ok(Self {
            dc,
            font,
            previous,
            metrics,
        })
    }

    fn actual_name(&self) -> Result<String, FontError> {
        let mut name = [0u16; 64];
        let copied = unsafe { GetTextFaceW(self.dc, name.len() as i32, name.as_mut_ptr()) };
        if copied <= 0 {
            return Err(FontError::MetricsFailed);
        }
        let len = name
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(name.len());
        Ok(String::from_utf16_lossy(&name[..len]))
    }
}

pub(crate) const fn candidates() -> &'static [FontFileCandidate] {
    // Windows monospace font files. Ordered by preference; the first readable
    // file wins. Sarasa Fixed SC is preferred for broad Chinese coverage,
    // then Cascadia Code / Consolas for fallback. Paths are absolute and use
    // backslashes on Windows.
    PRIMARY_CANDIDATES
}

/// Fonts consulted only for glyphs the primary face does not have.
///
/// The primary faces above are fixed-pitch and optimized for terminal metrics;
/// without explicit coverage fallbacks, a CJK/Japanese/Korean terminal may render
/// blank cells (width reserved, glyph absent). These are never chosen as the
/// primary face (cell metrics must come from the monospace font); they are only
/// used for missing glyphs.
pub(crate) const fn fallback_candidates() -> &'static [FontFileCandidate] {
    // SimSun's collection includes NSimSun (New Song), the traditional
    // fixed-width Chinese terminal face. Keep it ahead of proportional UI
    // fonts so CJK glyphs fill the terminal's two-cell-wide grid cleanly.
    FALLBACK_CANDIDATES
}

pub(crate) fn probe() -> FontDiscovery {
    let families: Vec<&'static str> = candidates().iter().map(|c| c.name).collect();
    FontDiscovery {
        primary_family: families.first().copied(),
        available_families: families,
    }
}

pub(crate) fn primary_family_name() -> Result<&'static str, FontError> {
    candidates()
        .first()
        .map(|c| c.name)
        .ok_or(FontError::Unavailable)
}

pub(crate) fn primary_metrics(size_px: u16) -> Result<FontMetrics, FontError> {
    let size_px = size_px.clamp(8, 72);
    let face = PixelFace::create(RASTER_FAMILIES[0], size_px)?;
    Ok(FontMetrics {
        family: None,
        size_px,
        cell_width: face.metrics.tmAveCharWidth.max(1) as f32,
        cell_height: face.metrics.tmHeight.max(1) as f32,
        ascent: face.metrics.tmAscent.max(1) as f32,
    })
}

pub(crate) fn probe_capability() -> Result<(), FontError> {
    Ok(())
}

pub(crate) fn rasterizer_name() -> Result<String, FontError> {
    PixelFace::create(RASTER_FAMILIES[0], 16)?.actual_name()
}

pub(crate) fn rasterize(ch: char, size_px: u16) -> Result<Option<RasterGlyph>, FontError> {
    let mut utf16 = [0u16; 2];
    let units = ch.encode_utf16(&mut utf16);
    if units.len() != 1 {
        return Ok(None);
    }
    let size_px = size_px.clamp(8, 72);
    RASTER_FACES
        .try_with(|slot| {
            let mut renderer = slot.try_borrow_mut().map_err(|_| FontError::RasterFailed)?;
            renderer.reset(size_px);
            for index in 0..RASTER_FAMILIES.len() {
                let face = renderer.face(index)?;
                let mut glyph_index = 0u16;
                let mapped = unsafe {
                    GetGlyphIndicesW(
                        face.dc,
                        units.as_ptr(),
                        1,
                        &mut glyph_index,
                        GGI_MARK_NONEXISTING_GLYPHS,
                    )
                };
                if mapped == GDI_ERROR as u32 || glyph_index == u16::MAX {
                    continue;
                }
                if let Some(glyph) = raster_face(face, glyph_index)? {
                    return Ok(Some(glyph));
                }
            }
            Ok(None)
        })
        .map_err(|_| FontError::RasterFailed)?
}

fn raster_face(face: &PixelFace, glyph_index: u16) -> Result<Option<RasterGlyph>, FontError> {
    let identity = MAT2 {
        eM11: FIXED { fract: 0, value: 1 },
        eM12: FIXED::default(),
        eM21: FIXED::default(),
        eM22: FIXED { fract: 0, value: 1 },
    };
    let mut metrics = GLYPHMETRICS::default();
    let format = GGO_GRAY8_BITMAP | GGO_GLYPH_INDEX;
    let required = unsafe {
        GetGlyphOutlineW(
            face.dc,
            u32::from(glyph_index),
            format,
            &mut metrics,
            0,
            ptr::null_mut(),
            &identity,
        )
    };
    if required == GDI_ERROR as u32 {
        return Ok(None);
    }
    if metrics.gmBlackBoxX > MAX_GLYPH_DIM
        || metrics.gmBlackBoxY > MAX_GLYPH_DIM
        || required > MAX_GLYPH_BYTES
    {
        return Err(FontError::GlyphTooLarge);
    }
    if required == 0 {
        return Ok(Some(RasterGlyph {
            alpha: Vec::new(),
            width: 0,
            height: 0,
            offset_x: metrics.gmptGlyphOrigin.x,
            offset_y: face.metrics.tmAscent - metrics.gmptGlyphOrigin.y,
        }));
    }
    let mut native = vec![0u8; required as usize];
    let written = unsafe {
        GetGlyphOutlineW(
            face.dc,
            u32::from(glyph_index),
            format,
            &mut metrics,
            required,
            native.as_mut_ptr().cast(),
            &identity,
        )
    };
    if written == GDI_ERROR as u32 || written > required {
        return Err(FontError::RasterFailed);
    }
    let width = metrics.gmBlackBoxX;
    let height = metrics.gmBlackBoxY;
    let stride = width.checked_add(3).ok_or(FontError::GlyphTooLarge)? & !3;
    let alpha_len = (width as usize)
        .checked_mul(height as usize)
        .ok_or(FontError::GlyphTooLarge)?;
    let mut alpha = vec![0u8; alpha_len];
    for y in 0..height {
        for x in 0..width {
            let source = (y as usize)
                .checked_mul(stride as usize)
                .and_then(|row| row.checked_add(x as usize))
                .filter(|index| *index < native.len())
                .ok_or(FontError::RasterFailed)?;
            alpha[(y * width + x) as usize] = ((u16::from(native[source]) * 255 + 32) / 64) as u8;
        }
    }
    Ok(Some(RasterGlyph {
        alpha,
        width,
        height,
        offset_x: metrics.gmptGlyphOrigin.x,
        offset_y: face.metrics.tmAscent - metrics.gmptGlyphOrigin.y,
    }))
}

pub(crate) fn create_terminal_font(
    window: OpaqueWindowHandle,
    request: FontRequest<'_>,
) -> Result<(isize, FontMetrics), FontError> {
    if request.family.is_empty() || request.point_size == 0 {
        return Err(FontError::InvalidRequest);
    }
    let window = window.get() as *mut core::ffi::c_void;
    let device = unsafe { GetDC(window) };
    if device.is_null() {
        return Err(FontError::DeviceContextUnavailable);
    }
    let dpi = unsafe { GetDeviceCaps(device, i32::try_from(LOGPIXELSY).unwrap_or(90)) };
    let requested_height = -((i32::from(request.point_size) * dpi) / 72).max(1);
    let family: Vec<u16> = request
        .family
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let font = unsafe {
        CreateFontW(
            requested_height,
            0,
            0,
            0,
            FW_NORMAL as i32,
            0,
            0,
            0,
            u32::from(DEFAULT_CHARSET),
            u32::from(OUT_DEFAULT_PRECIS),
            u32::from(CLIP_DEFAULT_PRECIS),
            u32::from(CLEARTYPE_QUALITY),
            u32::from(FIXED_PITCH | FF_MODERN),
            family.as_ptr(),
        )
    };
    if font.is_null() {
        unsafe { ReleaseDC(window, device) };
        return Err(FontError::CreateFailed);
    }

    let previous = unsafe { SelectObject(device, font as HGDIOBJ) };
    let mut metrics: TEXTMETRICW = unsafe { mem::zeroed() };
    let measured = unsafe { GetTextMetricsW(device, &mut metrics) };
    unsafe {
        SelectObject(device, previous);
        ReleaseDC(window, device);
    }
    if measured == 0 {
        unsafe { DeleteObject(font as HGDIOBJ) };
        return Err(FontError::MetricsFailed);
    }

    Ok((
        font as isize,
        FontMetrics {
            family: None,
            size_px: u16::try_from(requested_height.unsigned_abs()).unwrap_or(u16::MAX),
            cell_width: metrics.tmAveCharWidth.max(1) as f32,
            cell_height: metrics.tmHeight.max(1) as f32,
            ascent: metrics.tmAscent.max(1) as f32,
        },
    ))
}

pub(crate) fn destroy_terminal_font(raw: isize) {
    if raw != 0 {
        unsafe { DeleteObject(raw as *mut core::ffi::c_void) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_font_request_fails_before_native_access() {
        let window = unsafe { OpaqueWindowHandle::from_raw(0) };
        assert_eq!(
            create_terminal_font(
                window,
                FontRequest {
                    family: "",
                    point_size: 0,
                },
            ),
            Err(FontError::InvalidRequest)
        );
    }

    #[test]
    fn native_rasterizer_produces_bounded_ascii_and_cjk_coverage() {
        for ch in ['A', '中'] {
            let glyph = rasterize(ch, 16)
                .expect("GDI raster call")
                .expect("installed Windows font covers glyph");
            assert!(glyph.width <= MAX_GLYPH_DIM);
            assert!(glyph.height <= MAX_GLYPH_DIM);
            assert_eq!(glyph.alpha.len(), (glyph.width * glyph.height) as usize);
            assert!(glyph.alpha.iter().any(|alpha| *alpha != 0));
        }
    }

    #[test]
    fn supplementary_scalar_fails_safely_without_splitting_surrogates() {
        assert_eq!(rasterize('😀', 16), Ok(None));
    }

    #[test]
    fn native_rasterizer_reuses_a_face_until_the_size_changes() {
        RASTER_FACES.with(|slot| *slot.borrow_mut() = RasterFaces::empty());
        let face_id = |size_px| {
            RASTER_FACES.with(|slot| {
                let mut renderer = slot.borrow_mut();
                renderer.reset(size_px);
                Ok::<_, FontError>(renderer.face(0)? as *const PixelFace as usize)
            })
        };
        let first = face_id(16).expect("first face");
        let second = face_id(16).expect("reused face");
        assert_eq!(first, second);
        RASTER_FACES.with(|slot| {
            let mut renderer = slot.borrow_mut();
            renderer.reset(17);
            assert_eq!(renderer.size_px, 17);
            assert!(renderer.faces.iter().all(Option::is_none));
        });
    }

    #[test]
    fn printable_ascii_reuses_one_native_face() {
        RASTER_FACES.with(|slot| *slot.borrow_mut() = RasterFaces::empty());
        FACE_CREATIONS.with(|count| count.set(0));
        for byte in b'!'..=b'~' {
            rasterize(char::from(byte), 16)
                .expect("GDI raster call")
                .expect("primary face covers printable ASCII");
        }
        FACE_CREATIONS.with(|count| assert_eq!(count.get(), 1));
    }
}
