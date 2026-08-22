use std::{cell::RefCell, mem, ptr};

#[cfg(test)]
use std::cell::Cell;

use windows_sys::Win32::Graphics::Gdi::{
    ANTIALIASED_QUALITY, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateCompatibleDC, CreateFontW,
    DEFAULT_CHARSET, DeleteDC, DeleteObject, FF_MODERN, FIXED, FIXED_PITCH, FW_NORMAL, GDI_ERROR,
    GetCharWidth32W, GGI_MARK_NONEXISTING_GLYPHS, GGO_GLYPH_INDEX, GGO_GRAY8_BITMAP, GLYPHMETRICS, GetDC,
    GetDeviceCaps, GetFontData, GetGlyphIndicesW, GetGlyphOutlineW, GetTextFaceW, GetTextMetricsW,
    HGDIOBJ, LOGPIXELSY, MAT2, OUT_DEFAULT_PRECIS, ReleaseDC, SelectObject, TEXTMETRICW,
};

use crate::contract::font::{
    FontDiscovery, FontError, FontFileCandidate, FontMetrics, FontRequest, OpaqueWindowHandle,
    RasterGlyph,
};

const MAX_GLYPH_DIM: u32 = 4096;
const MAX_GLYPH_BYTES: u32 = MAX_GLYPH_DIM * MAX_GLYPH_DIM;
const MAX_CMAP_BYTES: u32 = 4 * 1024 * 1024;
const CMAP_TAG: u32 = u32::from_le_bytes(*b"cmap");
/// Families to consider, in coverage order.
///
/// This is a wish list, not a selection: `CreateFontW` never fails on a
/// missing family, so several of these resolve to whatever the GDI mapper
/// prefers on a given machine (`Sarasa Fixed SC` and `Cascadia Mono` ship
/// with nothing and are absent from stock Windows entirely). Which one is
/// actually used is decided by measuring — see `select_primary`.
///
/// The tail exists so a machine lacking every CJK-capable fixed face still
/// lands on a real monospaced font rather than on the mapper's default.
const RASTER_FAMILIES: &[&str] = &[
    "NSimSun",
    "SimSun",
    "Sarasa Fixed SC",
    "Cascadia Mono",
    "Consolas",
    "Courier New",
    "MS Mincho",
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
    /// The family the cell grid was measured on, resolved once per size.
    /// Glyph lookup must start here or ASCII is drawn from a face whose
    /// advance has nothing to do with the cell width.
    primary: Option<usize>,
    faces: [Option<PixelFace>; RASTER_FAMILIES.len()],
}

impl RasterFaces {
    const fn empty() -> Self {
        Self {
            size_px: 0,
            primary: None,
            faces: [const { None }; RASTER_FAMILIES.len()],
        }
    }

    fn reset(&mut self, size_px: u16) {
        if self.size_px != size_px {
            *self = Self {
                size_px,
                primary: None,
                faces: std::array::from_fn(|_| None),
            };
        }
    }

    /// Resolved lazily and cached: selection measures several faces, which is
    /// far too expensive to repeat per glyph.
    fn primary_index(&mut self) -> usize {
        if let Some(index) = self.primary {
            return index;
        }
        let index = select_primary(self.size_px).map_or(0, |(index, _, _)| index);
        self.primary = Some(index);
        index
    }

    /// Families in lookup order: the measured primary first, then the rest in
    /// declaration order for coverage.
    fn lookup_order(&mut self) -> impl Iterator<Item = usize> + use<> {
        let primary = self.primary_index();
        std::iter::once(primary).chain((0..RASTER_FAMILIES.len()).filter(move |i| *i != primary))
    }

    fn face(&mut self, index: usize) -> Result<&mut PixelFace, FontError> {
        if self.faces[index].is_none() {
            self.faces[index] = Some(PixelFace::create(RASTER_FAMILIES[index], self.size_px)?);
        }
        self.faces[index].as_mut().ok_or(FontError::CreateFailed)
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
    cmap_attempted: bool,
    cmap: Option<Box<[u8]>>,
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
            cmap_attempted: false,
            cmap: None,
        })
    }

    fn glyph_index(&mut self, ch: char, utf16: &[u16]) -> Result<Option<u16>, FontError> {
        if utf16.len() == 1 {
            let mut glyph_index = 0u16;
            let mapped = unsafe {
                GetGlyphIndicesW(
                    self.dc,
                    utf16.as_ptr(),
                    1,
                    &mut glyph_index,
                    GGI_MARK_NONEXISTING_GLYPHS,
                )
            };
            return Ok(
                (mapped != GDI_ERROR as u32 && glyph_index != u16::MAX).then_some(glyph_index)
            );
        }
        if !self.cmap_attempted {
            self.cmap_attempted = true;
            let required = unsafe { GetFontData(self.dc, CMAP_TAG, 0, ptr::null_mut(), 0) };
            if required != GDI_ERROR as u32 && required != 0 && required <= MAX_CMAP_BYTES {
                let mut bytes = vec![0u8; required as usize];
                let copied = unsafe {
                    GetFontData(self.dc, CMAP_TAG, 0, bytes.as_mut_ptr().cast(), required)
                };
                if copied == GDI_ERROR as u32 || copied != required {
                    return Err(FontError::RasterFailed);
                }
                self.cmap = Some(bytes.into_boxed_slice());
            }
        }
        Ok(self
            .cmap
            .as_deref()
            .and_then(|cmap| cmap_format_12_glyph_index(cmap, ch as u32)))
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

/// The characters the primary face is judged on.
///
/// `i` and `W` are the narrowest and widest common Latin letters: if a face
/// gives them the same advance it is monospaced, and if it does not, no cell
/// grid can render it evenly. `中` is a canonical full-width character.
const NARROW_PROBE: u16 = b'i' as u16;
const WIDE_PROBE: u16 = b'W' as u16;
const ASCII_PROBE: u16 = b'A' as u16;
const FULL_WIDTH_PROBE: u16 = 0x4E2D;

/// What a candidate face actually measures, as opposed to what it was asked
/// to be.
#[derive(Clone, Copy)]
struct FaceShape {
    /// The advance every ASCII character shares, in pixels.
    cell_width: i32,
    /// Whether a full-width character advances exactly two cells. A face
    /// without the glyph reports `false`; the fallback chain covers it.
    full_width_is_double: bool,
}

fn char_advance(dc: *mut core::ffi::c_void, unit: u16) -> Option<i32> {
    let mut width = 0_i32;
    let read = unsafe {
        // SAFETY: dc is a live device context with the face selected, and
        // width is a valid out-pointer for one entry.
        GetCharWidth32W(dc, u32::from(unit), u32::from(unit), &mut width)
    };
    (read != 0 && width > 0).then_some(width)
}

/// Measures a face rather than trusting the name it was created from.
///
/// `CreateFontW` never fails on a missing family: GDI's font mapper silently
/// substitutes the closest installed face, and `FIXED_PITCH | FF_MODERN` is a
/// scoring hint, not a constraint. So the family list says what is *wanted*,
/// and only measurement says what arrived. Checking the resolved name instead
/// would not work either — `GetTextFaceW` returns the localized name, so
/// "NSimSun" comes back as "新宋体".
///
/// `None` means the face cannot back a character grid.
fn measure_face(face: &PixelFace) -> Option<FaceShape> {
    // TMPF_FIXED_PITCH is set for *variable* pitch fonts. The inverted sense
    // is a Win32 wart, not a mistake here.
    const TMPF_FIXED_PITCH: u8 = 0x01;
    if face.metrics.tmPitchAndFamily & TMPF_FIXED_PITCH != 0 {
        return None;
    }
    let narrow = char_advance(face.dc, NARROW_PROBE)?;
    let wide = char_advance(face.dc, WIDE_PROBE)?;
    let ascii = char_advance(face.dc, ASCII_PROBE)?;
    if narrow != wide || narrow != ascii {
        return None;
    }
    Some(FaceShape {
        cell_width: ascii,
        full_width_is_double: char_advance(face.dc, FULL_WIDTH_PROBE) == Some(ascii * 2),
    })
}

/// Chooses the family the cell grid is built on.
///
/// Preference order is measured, not positional: a family that is monospaced
/// *and* renders full-width characters at exactly two cells wins outright,
/// because then one face covers both halves and the ratio is exact by
/// construction. Failing that, any monospaced family will do and wide glyphs
/// come from the fallback chain.
///
/// Falling back to index zero when nothing measures well is deliberate: a
/// terminal that renders imperfectly is worth more than one that refuses to
/// open, and the shape is reported so a caller can say which happened.
fn select_primary(size_px: u16) -> Result<(usize, PixelFace, Option<FaceShape>), FontError> {
    let mut monospaced: Option<(usize, PixelFace, FaceShape)> = None;
    for (index, family) in RASTER_FAMILIES.iter().enumerate() {
        let Ok(face) = PixelFace::create(family, size_px) else {
            continue;
        };
        let Some(shape) = measure_face(&face) else {
            continue;
        };
        if shape.full_width_is_double {
            return Ok((index, face, Some(shape)));
        }
        if monospaced.is_none() {
            monospaced = Some((index, face, shape));
        }
    }
    if let Some((index, face, shape)) = monospaced {
        return Ok((index, face, Some(shape)));
    }
    Ok((0, PixelFace::create(RASTER_FAMILIES[0], size_px)?, None))
}

/// Measured cell geometry per size, so selection runs once and not per call.
///
/// `primary_metrics` is on the paint path — every chrome string asks for the
/// metrics of its own size — and selection creates and measures GDI faces
/// until one qualifies. Recomputing that per call turned a repaint into
/// dozens of `CreateFontW` calls and made rapid font-size changes visibly
/// unstable. The sizes are clamped to 8..=72, so one slot each is exact and
/// bounded.
const MIN_SIZE_PX: u16 = 8;
const MAX_SIZE_PX: u16 = 72;

thread_local! {
    static METRICS_CACHE: RefCell<[Option<(f32, f32, f32)>; (MAX_SIZE_PX - MIN_SIZE_PX + 1) as usize]> =
        const { RefCell::new([None; (MAX_SIZE_PX - MIN_SIZE_PX + 1) as usize]) };
}

pub(crate) fn primary_metrics(size_px: u16) -> Result<FontMetrics, FontError> {
    let size_px = size_px.clamp(MIN_SIZE_PX, MAX_SIZE_PX);
    let slot = usize::from(size_px - MIN_SIZE_PX);
    if let Ok(Some((cell_width, cell_height, ascent))) =
        METRICS_CACHE.try_with(|cache| cache.borrow().get(slot).copied().flatten())
    {
        return Ok(FontMetrics {
            family: None,
            size_px,
            cell_width,
            cell_height,
            ascent,
        });
    }
    let (_, face, shape) = select_primary(size_px)?;
    // The measured advance, not `tmAveCharWidth`: the average is a
    // font-wide statistic that equals the real advance only when the face is
    // monospaced, which is exactly the thing that was not being checked.
    let cell_width = shape.map_or_else(
        || face.metrics.tmAveCharWidth.max(1),
        |shape| shape.cell_width.max(1),
    );
    let measured = (
        cell_width as f32,
        face.metrics.tmHeight.max(1) as f32,
        face.metrics.tmAscent.max(1) as f32,
    );
    let _ = METRICS_CACHE.try_with(|cache| {
        if let Some(entry) = cache.borrow_mut().get_mut(slot) {
            *entry = Some(measured);
        }
    });
    Ok(FontMetrics {
        family: None,
        size_px,
        cell_width: measured.0,
        cell_height: measured.1,
        ascent: measured.2,
    })
}

pub(crate) fn probe_capability() -> Result<(), FontError> {
    Ok(())
}

pub(crate) fn rasterizer_name() -> Result<String, FontError> {
    // The face the grid was actually measured on, so the window title names
    // what is being rendered rather than what was asked for first.
    select_primary(16)?.1.actual_name()
}

pub(crate) fn rasterize(ch: char, size_px: u16) -> Result<Option<RasterGlyph>, FontError> {
    let mut utf16 = [0u16; 2];
    let units = ch.encode_utf16(&mut utf16);
    let size_px = size_px.clamp(8, 72);
    RASTER_FACES
        .try_with(|slot| {
            let mut renderer = slot.try_borrow_mut().map_err(|_| FontError::RasterFailed)?;
            renderer.reset(size_px);
            for index in renderer.lookup_order().collect::<Vec<_>>() {
                let face = renderer.face(index)?;
                let Some(glyph_index) = face.glyph_index(ch, units)? else {
                    continue;
                };
                if let Some(glyph) = raster_face(face, glyph_index)? {
                    return Ok(Some(glyph));
                }
            }
            Ok(None)
        })
        .map_err(|_| FontError::RasterFailed)?
}

fn cmap_format_12_glyph_index(cmap: &[u8], codepoint: u32) -> Option<u16> {
    fn be_u16(bytes: &[u8], offset: usize) -> Option<u16> {
        let value = bytes.get(offset..offset.checked_add(2)?)?;
        Some(u16::from_be_bytes([value[0], value[1]]))
    }
    fn be_u32(bytes: &[u8], offset: usize) -> Option<u32> {
        let value = bytes.get(offset..offset.checked_add(4)?)?;
        Some(u32::from_be_bytes([value[0], value[1], value[2], value[3]]))
    }
    fn lookup(cmap: &[u8], offset: usize, codepoint: u32) -> Option<u16> {
        if be_u16(cmap, offset)? != 12 || be_u16(cmap, offset.checked_add(2)?)? != 0 {
            return None;
        }
        let length = usize::try_from(be_u32(cmap, offset.checked_add(4)?)?).ok()?;
        let end = offset.checked_add(length)?;
        if length < 16 || end > cmap.len() {
            return None;
        }
        let groups = usize::try_from(be_u32(cmap, offset.checked_add(12)?)?).ok()?;
        let group_bytes = groups.checked_mul(12)?;
        if 16usize.checked_add(group_bytes)? > length {
            return None;
        }
        let mut low = 0usize;
        let mut high = groups;
        while low < high {
            let middle = low + (high - low) / 2;
            let group = offset
                .checked_add(16)?
                .checked_add(middle.checked_mul(12)?)?;
            let start = be_u32(cmap, group)?;
            let finish = be_u32(cmap, group.checked_add(4)?)?;
            if codepoint < start {
                high = middle;
            } else if codepoint > finish {
                low = middle + 1;
            } else {
                let first_glyph = be_u32(cmap, group.checked_add(8)?)?;
                let glyph = first_glyph.checked_add(codepoint - start)?;
                return u16::try_from(glyph).ok().filter(|glyph| *glyph != 0);
            }
        }
        None
    }

    if be_u16(cmap, 0)? != 0 {
        return None;
    }
    let records = usize::from(be_u16(cmap, 2)?);
    let records_end = 4usize.checked_add(records.checked_mul(8)?)?;
    if records_end > cmap.len() {
        return None;
    }
    // Prefer the Windows UCS-4 encoding, then accept a Unicode-platform
    // format-12 table. Both offsets are relative to the start of `cmap`.
    for unicode_platform in [false, true] {
        for index in 0..records {
            let record = 4 + index * 8;
            let platform = be_u16(cmap, record)?;
            let encoding = be_u16(cmap, record + 2)?;
            if (!unicode_platform && !(platform == 3 && encoding == 10))
                || (unicode_platform && platform != 0)
            {
                continue;
            }
            let offset = usize::try_from(be_u32(cmap, record + 4)?).ok()?;
            if let Some(glyph) = lookup(cmap, offset, codepoint) {
                return Some(glyph);
            }
        }
    }
    None
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

    /// The invariant a character grid is built on, asserted on the face that
    /// is actually selected rather than on the one that was asked for first.
    ///
    /// Nothing tested this before, and that is why the defect shipped: a
    /// terminal is only monospaced if the *resolved* face is, and `CreateFontW`
    /// resolves a missing family to whatever the GDI mapper prefers without
    /// reporting that it did. On the machine this was written on, "NSimSun"
    /// and a deliberately nonexistent family produce identical results.
    #[test]
    fn the_selected_face_is_monospaced_at_every_size_the_product_offers() {
        for size in [8_u16, 12, 15, 16, 24, 48, 72] {
            let (index, face, shape) = select_primary(size).expect("a face is always selected");
            let shape = shape.unwrap_or_else(|| {
                panic!(
                    "no monospaced face found at {size}px; fell back to {:?}",
                    RASTER_FAMILIES[index]
                )
            });
            assert!(shape.cell_width > 0, "{size}px has no measurable advance");
            assert_eq!(
                char_advance(face.dc, NARROW_PROBE),
                char_advance(face.dc, WIDE_PROBE),
                "{:?} at {size}px is not monospaced: 'i' and 'W' differ",
                RASTER_FAMILIES[index]
            );
        }
    }

    /// Half width and full width must be exactly one and two cells. A face
    /// that renders CJK at any other ratio makes column alignment impossible,
    /// and the clipping blitter hides it by cutting the glyph instead of
    /// reporting it.
    #[test]
    fn the_selected_face_renders_full_width_characters_at_exactly_two_cells() {
        let (index, face, shape) = select_primary(16).expect("a face is always selected");
        let shape = shape.expect("a monospaced face");
        assert!(
            shape.full_width_is_double,
            "{:?} does not render 中 at two cells (ascii={}, full={:?})",
            RASTER_FAMILIES[index],
            shape.cell_width,
            char_advance(face.dc, FULL_WIDTH_PROBE)
        );
    }

    /// Selecting the grid face costs GDI work, and `primary_metrics` is on
    /// the paint path — every chrome string asks for the metrics of its own
    /// size. Measuring per call turned one repaint into dozens of
    /// `CreateFontW` calls and made rapid font-size changes visibly unstable,
    /// which no test noticed because none measured cost.
    #[test]
    fn repeated_metrics_queries_do_not_create_native_faces() {
        METRICS_CACHE.with(|cache| *cache.borrow_mut() = [None; 65]);
        FACE_CREATIONS.with(|count| count.set(0));
        primary_metrics(16).expect("metrics");
        let after_first = FACE_CREATIONS.with(Cell::get);
        assert!(after_first > 0, "the first query must actually measure");

        for _ in 0..200 {
            primary_metrics(16).expect("metrics");
        }
        assert_eq!(
            FACE_CREATIONS.with(Cell::get),
            after_first,
            "a repaint must not re-measure the font for every string it draws"
        );
    }

    /// Each size is measured on its own, so a cache hit for one size can never
    /// answer for another. A shared slot would report the terminal font's
    /// geometry for chrome text and misalign every label.
    #[test]
    fn each_size_is_measured_separately() {
        METRICS_CACHE.with(|cache| *cache.borrow_mut() = [None; 65]);
        let small = primary_metrics(10).expect("metrics");
        let large = primary_metrics(40).expect("metrics");
        assert_eq!(small.size_px, 10);
        assert_eq!(large.size_px, 40);
        assert!(
            large.cell_width > small.cell_width,
            "a larger size must measure wider: {} vs {}",
            large.cell_width,
            small.cell_width
        );
        // Re-reading returns the same numbers rather than a neighbour's.
        assert_eq!(primary_metrics(10).expect("metrics").cell_width, small.cell_width);
        assert_eq!(primary_metrics(40).expect("metrics").cell_width, large.cell_width);
    }

    /// A proportional face must be refused, or the selection is decorative.
    /// Microsoft YaHei is the case that matters: it is in the family list for
    /// CJK coverage and is variable pitch, so it must never win the primary
    /// slot even though `CreateFontW` returns it happily.
    #[test]
    fn a_proportional_face_is_refused_as_the_grid_face() {
        let face = PixelFace::create("Microsoft YaHei", 16).expect("YaHei or its substitute");
        // Only meaningful where the mapper really produced a variable-pitch
        // face; elsewhere this asserts nothing and says so rather than
        // pretending to have tested it.
        const TMPF_FIXED_PITCH: u8 = 0x01;
        if face.metrics.tmPitchAndFamily & TMPF_FIXED_PITCH == 0 {
            eprintln!("skipped: this system resolved Microsoft YaHei to a fixed-pitch face");
            return;
        }
        assert!(
            measure_face(&face).is_none(),
            "a variable-pitch face was accepted as the grid face"
        );
    }

    /// The measured advance and the reported cell width are the same number.
    /// They used to be different: the cell width came from `tmAveCharWidth`,
    /// a font-wide average that equals the real advance only when the face is
    /// monospaced -- exactly the property that was never checked.
    #[test]
    fn reported_cell_width_is_the_measured_advance_not_the_average() {
        let metrics = primary_metrics(16).expect("metrics");
        let (_, face, shape) = select_primary(16).expect("selection");
        let shape = shape.expect("a monospaced face");
        assert_eq!(metrics.cell_width, shape.cell_width as f32);
        assert_eq!(
            char_advance(face.dc, ASCII_PROBE),
            Some(shape.cell_width),
            "the reported cell width must be an advance a glyph actually has"
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
    fn format_12_maps_supplementary_scalars_without_utf16_splitting() {
        let mut cmap = Vec::new();
        cmap.extend_from_slice(&0u16.to_be_bytes());
        cmap.extend_from_slice(&1u16.to_be_bytes());
        cmap.extend_from_slice(&3u16.to_be_bytes());
        cmap.extend_from_slice(&10u16.to_be_bytes());
        cmap.extend_from_slice(&12u32.to_be_bytes());
        cmap.extend_from_slice(&12u16.to_be_bytes());
        cmap.extend_from_slice(&0u16.to_be_bytes());
        cmap.extend_from_slice(&28u32.to_be_bytes());
        cmap.extend_from_slice(&0u32.to_be_bytes());
        cmap.extend_from_slice(&1u32.to_be_bytes());
        cmap.extend_from_slice(&0x1_0000u32.to_be_bytes());
        cmap.extend_from_slice(&0x1_0002u32.to_be_bytes());
        cmap.extend_from_slice(&400u32.to_be_bytes());

        assert_eq!(cmap_format_12_glyph_index(&cmap, 0x1_0001), Some(401));
        assert_eq!(cmap_format_12_glyph_index(&cmap, 0xffff), None);
        assert_eq!(
            cmap_format_12_glyph_index(&cmap[..cmap.len() - 1], 0x1_0001),
            None
        );
    }

    #[test]
    fn native_rasterizer_produces_a_supplementary_outline_glyph() {
        let glyph = rasterize('𝄞', 20)
            .expect("GDI supplementary raster call")
            .expect("installed Windows symbol font covers musical symbol");
        assert!(glyph.width <= MAX_GLYPH_DIM);
        assert!(glyph.height <= MAX_GLYPH_DIM);
        assert_eq!(glyph.alpha.len(), (glyph.width * glyph.height) as usize);
        assert!(glyph.alpha.iter().any(|alpha| *alpha != 0));
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
    /// Rasterizing the whole printable ASCII range must cost no more native
    /// faces than rasterizing one character.
    ///
    /// The absolute count is deliberately not asserted: choosing the grid face
    /// measures candidates until one qualifies, so the fixed cost depends on
    /// which fonts a machine has. What must hold on every machine is that the
    /// cost is *fixed* — a face per character would rebuild GDI state 94 times
    /// per repaint.
    #[test]
    fn printable_ascii_reuses_one_native_face() {
        RASTER_FACES.with(|slot| *slot.borrow_mut() = RasterFaces::empty());
        FACE_CREATIONS.with(|count| count.set(0));
        rasterize('!', 16)
            .expect("GDI raster call")
            .expect("primary face covers printable ASCII");
        let after_first = FACE_CREATIONS.with(Cell::get);

        for byte in b'"'..=b'~' {
            rasterize(char::from(byte), 16)
                .expect("GDI raster call")
                .expect("primary face covers printable ASCII");
        }
        assert_eq!(
            FACE_CREATIONS.with(Cell::get),
            after_first,
            "the remaining 93 printable characters must not create a single face"
        );
    }
}
