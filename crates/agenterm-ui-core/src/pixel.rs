//! Compact architecture kernels for host-neutral XRGB pixel composition.

/// Blends an 8-bit alpha mask over XRGB pixels with one packed `0x00RRGGBB`
/// foreground color. A mismatched input length safely processes only the common
/// prefix; callers can therefore reject or retain the untouched suffix without
/// risking an out-of-bounds architecture call.
pub fn blend_mask_xrgb(destination: &mut [u32], alpha: &[u8], foreground: u32) {
    let length = destination.len().min(alpha.len());
    if length == 0 {
        return;
    }
    let foreground = foreground & 0x00ff_ffff;
    unsafe {
        selected_kernel()(destination.as_mut_ptr(), alpha.as_ptr(), length, foreground);
    }
}

type Kernel = unsafe fn(*mut u32, *const u8, usize, u32);

#[cfg(target_arch = "x86_64")]
fn selected_kernel() -> Kernel {
    use std::sync::OnceLock;
    static KERNEL: OnceLock<Kernel> = OnceLock::new();
    *KERNEL.get_or_init(|| {
        if std::is_x86_feature_detected!("avx2") {
            avx2_kernel
        } else {
            sse2_kernel
        }
    })
}

#[cfg(target_arch = "aarch64")]
fn selected_kernel() -> Kernel {
    neon_kernel
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn selected_kernel() -> Kernel {
    scalar_kernel
}

#[inline]
fn blend_one(existing: u32, foreground: u32, alpha: u8) -> u32 {
    let alpha = u32::from(alpha);
    let inverse = 255 - alpha;
    let red = (((existing >> 16) & 0xff) * inverse + ((foreground >> 16) & 0xff) * alpha) / 255;
    let green = (((existing >> 8) & 0xff) * inverse + ((foreground >> 8) & 0xff) * alpha) / 255;
    let blue = ((existing & 0xff) * inverse + (foreground & 0xff) * alpha) / 255;
    red << 16 | green << 8 | blue
}

unsafe fn scalar_kernel(destination: *mut u32, alpha: *const u8, length: usize, foreground: u32) {
    for index in 0..length {
        unsafe {
            let slot = destination.add(index);
            *slot = blend_one(*slot, foreground, *alpha.add(index));
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn sse2_kernel(destination: *mut u32, alpha: *const u8, length: usize, foreground: u32) {
    use core::arch::x86_64::*;

    let mut index = 0;
    let zero = _mm_setzero_si128();
    let maximum = _mm_set1_epi16(255);
    let one = _mm_set1_epi16(1);
    let divisor = _mm_set1_epi16(257);
    let foreground8 = _mm_set1_epi32(foreground as i32);
    let foreground_low = _mm_unpacklo_epi8(foreground8, zero);
    let foreground_high = _mm_unpackhi_epi8(foreground8, zero);
    let xrgb_mask = _mm_set1_epi32(0x00ff_ffff);

    while index + 4 <= length {
        unsafe {
            let pixels = _mm_loadu_si128(destination.add(index).cast());
            let alpha_word = alpha.add(index).cast::<u32>().read_unaligned();
            let alpha4 = _mm_cvtsi32_si128(alpha_word as i32);
            let alpha_pairs = _mm_unpacklo_epi8(alpha4, alpha4);
            let alpha8 = _mm_unpacklo_epi16(alpha_pairs, alpha_pairs);
            let alpha_low = _mm_unpacklo_epi8(alpha8, zero);
            let alpha_high = _mm_unpackhi_epi8(alpha8, zero);
            let inverse_low = _mm_sub_epi16(maximum, alpha_low);
            let inverse_high = _mm_sub_epi16(maximum, alpha_high);
            let pixels_low = _mm_unpacklo_epi8(pixels, zero);
            let pixels_high = _mm_unpackhi_epi8(pixels, zero);
            let mixed_low = _mm_add_epi16(
                _mm_mullo_epi16(pixels_low, inverse_low),
                _mm_mullo_epi16(foreground_low, alpha_low),
            );
            let mixed_high = _mm_add_epi16(
                _mm_mullo_epi16(pixels_high, inverse_high),
                _mm_mullo_epi16(foreground_high, alpha_high),
            );
            let divided_low = _mm_mulhi_epu16(_mm_add_epi16(mixed_low, one), divisor);
            let divided_high = _mm_mulhi_epu16(_mm_add_epi16(mixed_high, one), divisor);
            let packed = _mm_and_si128(_mm_packus_epi16(divided_low, divided_high), xrgb_mask);
            _mm_storeu_si128(destination.add(index).cast(), packed);
        }
        index += 4;
    }
    if index < length {
        unsafe {
            scalar_kernel(
                destination.add(index),
                alpha.add(index),
                length - index,
                foreground,
            );
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn avx2_kernel(destination: *mut u32, alpha: *const u8, length: usize, foreground: u32) {
    use core::arch::x86_64::*;

    let mut index = 0;
    let zero = _mm256_setzero_si256();
    let maximum = _mm256_set1_epi16(255);
    let one = _mm256_set1_epi16(1);
    let divisor = _mm256_set1_epi16(257);
    let foreground8 = _mm256_set1_epi32(foreground as i32);
    let foreground_low = _mm256_unpacklo_epi8(foreground8, zero);
    let foreground_high = _mm256_unpackhi_epi8(foreground8, zero);
    let xrgb_mask = _mm256_set1_epi32(0x00ff_ffff);
    let replicate = _mm256_set1_epi32(0x0101_0101);

    while index + 8 <= length {
        unsafe {
            let pixels = _mm256_loadu_si256(destination.add(index).cast());
            let alpha_word = alpha.add(index).cast::<u64>().read_unaligned();
            let alpha8 = _mm_cvtsi64_si128(alpha_word as i64);
            let alpha32 = _mm256_cvtepu8_epi32(alpha8);
            let replicated = _mm256_mullo_epi32(alpha32, replicate);
            let alpha_low = _mm256_unpacklo_epi8(replicated, zero);
            let alpha_high = _mm256_unpackhi_epi8(replicated, zero);
            let inverse_low = _mm256_sub_epi16(maximum, alpha_low);
            let inverse_high = _mm256_sub_epi16(maximum, alpha_high);
            let pixels_low = _mm256_unpacklo_epi8(pixels, zero);
            let pixels_high = _mm256_unpackhi_epi8(pixels, zero);
            let mixed_low = _mm256_add_epi16(
                _mm256_mullo_epi16(pixels_low, inverse_low),
                _mm256_mullo_epi16(foreground_low, alpha_low),
            );
            let mixed_high = _mm256_add_epi16(
                _mm256_mullo_epi16(pixels_high, inverse_high),
                _mm256_mullo_epi16(foreground_high, alpha_high),
            );
            let divided_low = _mm256_mulhi_epu16(_mm256_add_epi16(mixed_low, one), divisor);
            let divided_high = _mm256_mulhi_epu16(_mm256_add_epi16(mixed_high, one), divisor);
            let packed =
                _mm256_and_si256(_mm256_packus_epi16(divided_low, divided_high), xrgb_mask);
            _mm256_storeu_si256(destination.add(index).cast(), packed);
        }
        index += 8;
    }
    if index < length {
        unsafe {
            sse2_kernel(
                destination.add(index),
                alpha.add(index),
                length - index,
                foreground,
            );
        }
    }
}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn neon_kernel(destination: *mut u32, alpha: *const u8, length: usize, foreground: u32) {
    use core::arch::aarch64::*;

    #[inline]
    unsafe fn divide_255(value: uint16x8_t) -> uint16x8_t {
        unsafe {
            let adjusted = vaddq_u16(value, vdupq_n_u16(1));
            let low = vmull_n_u16(vget_low_u16(adjusted), 257);
            let high = vmull_n_u16(vget_high_u16(adjusted), 257);
            vcombine_u16(vshrn_n_u32::<16>(low), vshrn_n_u32::<16>(high))
        }
    }

    let foreground_bytes = foreground.to_le_bytes();
    let foreground8 = unsafe { vld1q_dup_u32(foreground_bytes.as_ptr().cast()) };
    let foreground8 = vreinterpretq_u8_u32(foreground8);
    let xrgb_mask = vreinterpretq_u8_u32(vdupq_n_u32(0x00ff_ffff));
    let maximum = vdupq_n_u8(255);
    let mut index = 0;
    while index + 4 <= length {
        unsafe {
            let a0 = *alpha.add(index);
            let a1 = *alpha.add(index + 1);
            let a2 = *alpha.add(index + 2);
            let a3 = *alpha.add(index + 3);
            let alpha_bytes = [
                a0, a0, a0, a0, a1, a1, a1, a1, a2, a2, a2, a2, a3, a3, a3, a3,
            ];
            let alpha8 = vld1q_u8(alpha_bytes.as_ptr());
            let inverse8 = vsubq_u8(maximum, alpha8);
            let pixels8 = vld1q_u8(destination.add(index).cast());
            let pixels_low = vmovl_u8(vget_low_u8(pixels8));
            let pixels_high = vmovl_u8(vget_high_u8(pixels8));
            let alpha_low = vmovl_u8(vget_low_u8(alpha8));
            let alpha_high = vmovl_u8(vget_high_u8(alpha8));
            let inverse_low = vmovl_u8(vget_low_u8(inverse8));
            let inverse_high = vmovl_u8(vget_high_u8(inverse8));
            let foreground_low = vmovl_u8(vget_low_u8(foreground8));
            let foreground_high = vmovl_u8(vget_high_u8(foreground8));
            let mixed_low = vaddq_u16(
                vmulq_u16(pixels_low, inverse_low),
                vmulq_u16(foreground_low, alpha_low),
            );
            let mixed_high = vaddq_u16(
                vmulq_u16(pixels_high, inverse_high),
                vmulq_u16(foreground_high, alpha_high),
            );
            let packed = vcombine_u8(
                vqmovn_u16(divide_255(mixed_low)),
                vqmovn_u16(divide_255(mixed_high)),
            );
            vst1q_u8(destination.add(index).cast(), vandq_u8(packed, xrgb_mask));
        }
        index += 4;
    }
    if index < length {
        unsafe {
            scalar_kernel(
                destination.add(index),
                alpha.add(index),
                length - index,
                foreground,
            );
        }
    }
}

/// Packs little-endian `0x00RRGGBB` pixels into tightly packed RGB8 bytes.
/// Returns the number of pixels written; a short destination safely truncates
/// at a complete pixel rather than writing a partial RGB triplet.
pub fn pack_xrgb_to_rgb8(source: &[u32], destination: &mut [u8]) -> usize {
    let length = source.len().min(destination.len() / 3);
    if length != 0 {
        unsafe {
            selected_pack_kernel()(source.as_ptr(), destination.as_mut_ptr(), length);
        }
    }
    length
}

type PackKernel = unsafe fn(*const u32, *mut u8, usize);

#[cfg(target_arch = "x86_64")]
fn selected_pack_kernel() -> PackKernel {
    use std::sync::OnceLock;
    static KERNEL: OnceLock<PackKernel> = OnceLock::new();
    *KERNEL.get_or_init(|| {
        if std::is_x86_feature_detected!("ssse3") {
            ssse3_pack_kernel
        } else {
            scalar_pack_kernel
        }
    })
}

#[cfg(target_arch = "aarch64")]
fn selected_pack_kernel() -> PackKernel {
    neon_pack_kernel
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn selected_pack_kernel() -> PackKernel {
    scalar_pack_kernel
}

unsafe fn scalar_pack_kernel(source: *const u32, destination: *mut u8, length: usize) {
    for index in 0..length {
        unsafe {
            let pixel = *source.add(index);
            let output = destination.add(index * 3);
            *output = (pixel >> 16) as u8;
            *output.add(1) = (pixel >> 8) as u8;
            *output.add(2) = pixel as u8;
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "ssse3")]
unsafe fn ssse3_pack_kernel(source: *const u32, destination: *mut u8, length: usize) {
    use core::arch::x86_64::*;

    let shuffle = _mm_setr_epi8(2, 1, 0, 6, 5, 4, 10, 9, 8, 14, 13, 12, -1, -1, -1, -1);
    let mut index = 0;
    while index + 4 <= length {
        unsafe {
            let pixels = _mm_loadu_si128(source.add(index).cast());
            let packed = _mm_shuffle_epi8(pixels, shuffle);
            let output = destination.add(index * 3);
            _mm_storel_epi64(output.cast(), packed);
            output
                .add(8)
                .cast::<u32>()
                .write_unaligned(_mm_cvtsi128_si32(_mm_srli_si128::<8>(packed)) as u32);
        }
        index += 4;
    }
    if index < length {
        unsafe {
            scalar_pack_kernel(
                source.add(index),
                destination.add(index * 3),
                length - index,
            );
        }
    }
}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn neon_pack_kernel(source: *const u32, destination: *mut u8, length: usize) {
    use core::arch::aarch64::*;

    let shuffle = unsafe {
        vld1q_u8(
            [
                2, 1, 0, 6, 5, 4, 10, 9, 8, 14, 13, 12, 0xff, 0xff, 0xff, 0xff,
            ]
            .as_ptr(),
        )
    };
    let mut index = 0;
    while index + 4 <= length {
        unsafe {
            let pixels = vld1q_u8(source.add(index).cast());
            let packed = vqtbl1q_u8(pixels, shuffle);
            let output = destination.add(index * 3);
            vst1_u8(output, vget_low_u8(packed));
            vst1_lane_u32::<0>(
                output.add(8).cast(),
                vreinterpret_u32_u8(vget_high_u8(packed)),
            );
        }
        index += 4;
    }
    if index < length {
        unsafe {
            scalar_pack_kernel(
                source.add(index),
                destination.add(index * 3),
                length - index,
            );
        }
    }
}

/// Fills a clipped rectangle in a row-major XRGB framebuffer.
///
/// `stride` is the physical pixel width of a row. A partial trailing row is
/// ignored, zero geometry is a no-op, and overflowing rectangles clamp to the
/// complete framebuffer. Full-width rectangles collapse to one contiguous ISA
/// call instead of dispatching once per row.
pub fn fill_xrgb_rect(
    destination: &mut [u32],
    stride: u32,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    color: u32,
) {
    let stride = stride as usize;
    if stride == 0 || width == 0 || height == 0 {
        return;
    }
    let rows = destination.len() / stride;
    let x = (x as usize).min(stride);
    let y = (y as usize).min(rows);
    let right = x.saturating_add(width as usize).min(stride);
    let bottom = y.saturating_add(height as usize).min(rows);
    let span = right - x;
    let row_count = bottom - y;
    if span == 0 || row_count == 0 {
        return;
    }

    if x == 0 && span == stride {
        destination[y * stride..bottom * stride].fill(color);
    } else {
        for row in y..bottom {
            let start = row * stride + x;
            destination[start..start + span].fill(color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expected(source: &[u32], alpha: &[u8], foreground: u32) -> Vec<u32> {
        source
            .iter()
            .zip(alpha)
            .map(|(&pixel, &mask)| blend_one(pixel, foreground, mask))
            .collect()
    }

    fn exercise(kernel: Kernel) {
        let foregrounds = [0, 0x00ff_ffff, 0x0012_9acf, 0x00ff_0040];
        for length in 0..35 {
            for &foreground in &foregrounds {
                let source: Vec<u32> = (0..length)
                    .map(|index| 0xa500_0000 | ((index as u32 * 0x0007_1329) & 0x00ff_ffff))
                    .collect();
                let alpha: Vec<u8> = (0..length)
                    .map(|index| ((index * 73 + length * 19) & 0xff) as u8)
                    .collect();
                let expected = expected(&source, &alpha, foreground);
                let mut actual = source;
                unsafe {
                    kernel(actual.as_mut_ptr(), alpha.as_ptr(), length, foreground);
                }
                assert_eq!(
                    actual, expected,
                    "length={length} foreground={foreground:#x}"
                );
            }
        }
    }

    fn exercise_pack(kernel: PackKernel) {
        for length in 0..35 {
            let source: Vec<u32> = (0..length)
                .map(|index| 0xa500_0000 | ((index as u32 * 0x0007_1329) & 0x00ff_ffff))
                .collect();
            let expected: Vec<u8> = source
                .iter()
                .flat_map(|pixel| [(*pixel >> 16) as u8, (*pixel >> 8) as u8, *pixel as u8])
                .collect();
            let mut actual = vec![0xcc; length * 3];
            unsafe {
                kernel(source.as_ptr(), actual.as_mut_ptr(), length);
            }
            assert_eq!(actual, expected, "length={length}");
        }
    }

    #[test]
    fn scalar_is_the_bit_exact_reference() {
        exercise(scalar_kernel);
    }

    #[test]
    fn public_api_uses_common_prefix_without_panicking() {
        let mut pixels = [0x0010_2030, 0x0040_5060, 0x0070_8090];
        blend_mask_xrgb(&mut pixels, &[255, 0], 0x00ab_cdef);
        assert_eq!(pixels, [0x00ab_cdef, 0x0040_5060, 0x0070_8090]);
    }

    #[test]
    fn public_pack_api_writes_only_complete_pixels() {
        let source = [0xaa12_3456, 0xbbab_cdef];
        let mut output = [0xcc; 5];
        assert_eq!(pack_xrgb_to_rgb8(&source, &mut output), 1);
        assert_eq!(output, [0x12, 0x34, 0x56, 0xcc, 0xcc]);
    }

    #[test]
    fn scalar_pack_is_the_byte_exact_reference() {
        exercise_pack(scalar_pack_kernel);
    }

    #[test]
    fn clipped_fill_preserves_offsets_and_partial_trailing_row() {
        let mut pixels = vec![0xaaaa_aaaa; 22];
        fill_xrgb_rect(&mut pixels, 5, 3, 1, u32::MAX, u32::MAX, 0x0012_3456);
        for row in 0..4 {
            for column in 0..5 {
                let expected = if row >= 1 && column >= 3 {
                    0x0012_3456
                } else {
                    0xaaaa_aaaa
                };
                assert_eq!(pixels[row * 5 + column], expected, "row={row} col={column}");
            }
        }
        assert_eq!(&pixels[20..], &[0xaaaa_aaaa; 2]);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn sse2_is_bit_exact() {
        exercise(sse2_kernel);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn avx2_is_bit_exact_when_available() {
        if std::is_x86_feature_detected!("avx2") {
            exercise(avx2_kernel);
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn ssse3_pack_is_byte_exact_when_available() {
        if std::is_x86_feature_detected!("ssse3") {
            exercise_pack(ssse3_pack_kernel);
        }
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn neon_is_bit_exact() {
        exercise(neon_kernel);
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn neon_pack_is_byte_exact() {
        exercise_pack(neon_pack_kernel);
    }
}
