//! Compact dependency-free checksums for platform data contracts.

use std::sync::OnceLock;

const ADLER_MODULUS: u32 = 65_521;
const ADLER_BLOCK_LEN: usize = 16;
const ADLER_REDUCTION_CHUNK: usize = 5_552;
const ADLER_WEIGHTS_16: [u8; ADLER_BLOCK_LEN] =
    [16, 15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1];

type AdlerUpdate = fn(u32, u32, &[u8]) -> (u32, u32);

static ADLER_UPDATE: OnceLock<AdlerUpdate> = OnceLock::new();

/// Incremental Adler-32 checksum.
///
/// The state is kept as the two Adler sums rather than as a packed checksum so
/// that every update can combine an arbitrary chunk without depending on its
/// caller's chunking. The first sum starts at one, as required by Adler-32.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Adler32 {
    s1: u32,
    s2: u32,
}

impl Default for Adler32 {
    fn default() -> Self {
        Self::new()
    }
}

impl Adler32 {
    pub const fn new() -> Self {
        Self { s1: 1, s2: 0 }
    }

    pub fn update(&mut self, bytes: &[u8]) {
        let (s1, s2) = adler_update()(self.s1, self.s2, bytes);
        self.s1 = s1;
        self.s2 = s2;
    }

    pub const fn finish(self) -> u32 {
        (self.s2 << 16) | self.s1
    }
}

#[inline]
fn adler_update() -> AdlerUpdate {
    *ADLER_UPDATE.get_or_init(select_adler_update)
}

fn adler_update_scalar(s1: u32, s2: u32, bytes: &[u8]) -> (u32, u32) {
    let modulus = u64::from(ADLER_MODULUS);
    let mut s1 = u64::from(s1) % modulus;
    let mut s2 = u64::from(s2) % modulus;
    for chunk in bytes.chunks(ADLER_REDUCTION_CHUNK) {
        for &byte in chunk {
            s1 += u64::from(byte);
            s2 += s1;
        }
        s1 %= modulus;
        s2 %= modulus;
    }
    (s1 as u32, s2 as u32)
}

#[cfg(target_arch = "x86_64")]
fn adler_update_ssse3(s1: u32, s2: u32, bytes: &[u8]) -> (u32, u32) {
    // The wrapper keeps the target-feature call behind the runtime check in
    // select_adler_update. SSSE3 is the smallest x86_64 kernel that computes
    // both byte sums and weighted byte sums without a second scalar pass.
    if bytes.len() < ADLER_BLOCK_LEN {
        adler_update_scalar(s1, s2, bytes)
    } else {
        unsafe { adler_update_ssse3_inner(s1, s2, bytes) }
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "ssse3")]
unsafe fn adler_update_ssse3_inner(s1: u32, s2: u32, bytes: &[u8]) -> (u32, u32) {
    use core::arch::x86_64::{
        __m128i, _mm_cvtsi128_si32, _mm_cvtsi128_si64, _mm_loadu_si128, _mm_madd_epi16,
        _mm_maddubs_epi16, _mm_sad_epu8, _mm_set1_epi16, _mm_setzero_si128, _mm_srli_si128,
    };

    let modulus = u64::from(ADLER_MODULUS);
    let mut s1 = u64::from(s1) % modulus;
    let mut s2 = u64::from(s2) % modulus;
    let weights = unsafe { _mm_loadu_si128(ADLER_WEIGHTS_16.as_ptr() as *const __m128i) };
    let zero = _mm_setzero_si128();
    let ones = _mm_set1_epi16(1);
    for chunk in bytes.chunks(ADLER_REDUCTION_CHUNK) {
        let mut offset = 0;
        while chunk.len() - offset >= ADLER_BLOCK_LEN {
            let data = unsafe { _mm_loadu_si128(chunk.as_ptr().add(offset) as *const __m128i) };
            let byte_sums = _mm_sad_epu8(data, zero);
            let byte_sums_high = _mm_srli_si128(byte_sums, 8);
            let sum =
                (_mm_cvtsi128_si64(byte_sums) as u64) + (_mm_cvtsi128_si64(byte_sums_high) as u64);

            let pair_sums = _mm_maddubs_epi16(data, weights);
            let groups = _mm_madd_epi16(pair_sums, ones);
            let weighted = u64::from(_mm_cvtsi128_si32(groups) as u32)
                + u64::from(_mm_cvtsi128_si32(_mm_srli_si128(groups, 4)) as u32)
                + u64::from(_mm_cvtsi128_si32(_mm_srli_si128(groups, 8)) as u32)
                + u64::from(_mm_cvtsi128_si32(_mm_srli_si128(groups, 12)) as u32);

            s2 += s1 * ADLER_BLOCK_LEN as u64 + weighted;
            s1 += sum;
            offset += ADLER_BLOCK_LEN;
        }
        for &byte in &chunk[offset..] {
            s1 += u64::from(byte);
            s2 += s1;
        }
        s1 %= modulus;
        s2 %= modulus;
    }
    (s1 as u32, s2 as u32)
}

#[cfg(target_arch = "aarch64")]
fn adler_update_neon(s1: u32, s2: u32, bytes: &[u8]) -> (u32, u32) {
    if bytes.len() < ADLER_BLOCK_LEN {
        adler_update_scalar(s1, s2, bytes)
    } else {
        unsafe { adler_update_neon_inner(s1, s2, bytes) }
    }
}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn adler_update_neon_inner(s1: u32, s2: u32, bytes: &[u8]) -> (u32, u32) {
    use core::arch::aarch64::{
        vaddlvq_u8, vaddlvq_u16, vget_high_u8, vget_low_u8, vld1q_u8, vmull_u8,
    };

    let modulus = u64::from(ADLER_MODULUS);
    let mut s1 = u64::from(s1) % modulus;
    let mut s2 = u64::from(s2) % modulus;
    let weights = unsafe { vld1q_u8(ADLER_WEIGHTS_16.as_ptr()) };
    for chunk in bytes.chunks(ADLER_REDUCTION_CHUNK) {
        let mut offset = 0;
        while chunk.len() - offset >= ADLER_BLOCK_LEN {
            let (sum, weighted) = unsafe {
                let data = vld1q_u8(chunk.as_ptr().add(offset));
                let sum = u64::from(vaddlvq_u8(data));
                let weighted = u64::from(
                    vaddlvq_u16(vmull_u8(vget_low_u8(data), vget_low_u8(weights)))
                        + vaddlvq_u16(vmull_u8(vget_high_u8(data), vget_high_u8(weights))),
                );
                (sum, weighted)
            };
            s2 += s1 * ADLER_BLOCK_LEN as u64 + weighted;
            s1 += sum;
            offset += ADLER_BLOCK_LEN;
        }
        for &byte in &chunk[offset..] {
            s1 += u64::from(byte);
            s2 += s1;
        }
        s1 %= modulus;
        s2 %= modulus;
    }
    (s1 as u32, s2 as u32)
}

#[cfg(target_arch = "x86_64")]
fn select_adler_update() -> AdlerUpdate {
    if std::is_x86_feature_detected!("ssse3") {
        adler_update_ssse3
    } else {
        adler_update_scalar
    }
}

#[cfg(target_arch = "aarch64")]
fn select_adler_update() -> AdlerUpdate {
    adler_update_neon
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn select_adler_update() -> AdlerUpdate {
    adler_update_scalar
}

const IEEE_POLYNOMIAL: u32 = 0xedb8_8320;

const fn byte_table() -> [u32; 256] {
    let mut table = [0; 256];
    let mut index = 0;
    while index < table.len() {
        let mut value = index as u32;
        let mut bit = 0;
        while bit < 8 {
            value = (value >> 1) ^ (IEEE_POLYNOMIAL & 0u32.wrapping_sub(value & 1));
            bit += 1;
        }
        table[index] = value;
        index += 1;
    }
    table
}

const IEEE_BYTE_TABLE: [u32; 256] = byte_table();

/// Incremental IEEE CRC-32 used by PNG, ZIP and Ethernet.
///
/// This is deliberately distinct from the CRC-32C polynomial implemented by
/// x86 SSE4.2 and Arm CRC instructions. The 256-entry byte table remains a
/// compact 1 KiB while reducing each byte to one indexed reduction.
//
// There is intentionally no PCLMULQDQ/PMULL folding path here. A reflected
// IEEE CRC implementation needs independently verified reduction constants and
// chunk-combination logic; the available CRC instructions are CRC-32C, not
// this polynomial. Keep the compact scalar truth path until that proof and a
// measured size/throughput win exist.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IeeeCrc32(u32);

impl Default for IeeeCrc32 {
    fn default() -> Self {
        Self::new()
    }
}

impl IeeeCrc32 {
    pub const fn new() -> Self {
        Self(!0)
    }

    pub fn update(&mut self, bytes: &[u8]) {
        let mut crc = self.0;
        for &byte in bytes {
            crc = (crc >> 8) ^ IEEE_BYTE_TABLE[((crc ^ u32::from(byte)) & 0xff) as usize];
        }
        self.0 = crc;
    }

    pub const fn finish(self) -> u32 {
        !self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ieee_bitwise(bytes: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &byte in bytes {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (IEEE_POLYNOMIAL & 0u32.wrapping_sub(crc & 1));
            }
        }
        !crc
    }

    fn checksum_with(update: AdlerUpdate, bytes: &[u8]) -> u32 {
        let (s1, s2) = update(1, 0, bytes);
        (s2 << 16) | s1
    }

    fn checksum_split(update: AdlerUpdate, bytes: &[u8], split: usize) -> u32 {
        let (s1, s2) = update(1, 0, &bytes[..split]);
        let (s1, s2) = update(s1, s2, &bytes[split..]);
        (s2 << 16) | s1
    }

    #[cfg(target_arch = "x86_64")]
    fn isa_update_for_tests() -> Option<AdlerUpdate> {
        std::is_x86_feature_detected!("ssse3").then_some(adler_update_ssse3)
    }

    #[cfg(target_arch = "aarch64")]
    fn isa_update_for_tests() -> Option<AdlerUpdate> {
        Some(adler_update_neon)
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    fn isa_update_for_tests() -> Option<AdlerUpdate> {
        None
    }

    #[test]
    fn adler_standard_vectors_and_state_api() {
        assert_eq!(Adler32::new().finish(), 1);

        let mut wikipedia = Adler32::new();
        wikipedia.update(b"Wikipedia");
        assert_eq!(wikipedia.finish(), 0x11e6_0398);

        let mut split = Adler32::new();
        split.update(b"Wiki");
        split.update(b"pedia");
        assert_eq!(split.finish(), wikipedia.finish());
    }

    #[test]
    fn adler_all_lengths_and_every_split_are_bit_exact() {
        let mut storage = vec![0u8; 4096 + 32];
        let mut random = 0x9e37_79b9u32;
        for byte in &mut storage {
            random = random.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *byte = (random >> 24) as u8;
        }

        let offset = if (storage.as_ptr() as usize & 15) == 0 {
            1
        } else {
            0
        };
        let base = storage[offset..].as_ptr() as usize;
        assert_ne!(base & 15, 0);

        let selected = adler_update();
        let isa = isa_update_for_tests();
        for length in 0..=4096 {
            let bytes = &storage[offset..offset + length];
            let expected = checksum_with(adler_update_scalar, bytes);
            assert_eq!(checksum_with(selected, bytes), expected);
            if let Some(isa) = isa {
                assert_eq!(checksum_with(isa, bytes), expected);
            }
        }

        for length in [0, 1, 15, 16, 17, 31, 32, 33, 255, 256, 1023, 4096] {
            let bytes = &storage[offset..offset + length];
            let expected = checksum_with(adler_update_scalar, bytes);
            for split in 0..=length {
                assert_eq!(checksum_split(adler_update_scalar, bytes, split), expected);
                assert_eq!(checksum_split(selected, bytes, split), expected);
                if let Some(isa) = isa {
                    assert_eq!(checksum_split(isa, bytes, split), expected);
                }
            }
        }
    }

    #[test]
    fn adler_state_arithmetic_does_not_overflow() {
        let bytes = [255u8; 4096];
        for &(s1, s2) in &[
            (0, 0),
            (ADLER_MODULUS - 1, ADLER_MODULUS - 1),
            (u32::MAX, u32::MAX),
        ] {
            let scalar = adler_update_scalar(s1, s2, &bytes);
            assert!(scalar.0 < ADLER_MODULUS);
            assert!(scalar.1 < ADLER_MODULUS);
            if let Some(isa) = isa_update_for_tests() {
                assert_eq!(isa(s1, s2, &bytes), scalar);
            }
        }
    }

    #[test]
    fn standard_ieee_vector_matches() {
        let mut crc = IeeeCrc32::new();
        crc.update(b"123456789");
        assert_eq!(crc.finish(), 0xcbf4_3926);
    }

    #[test]
    fn ieee_byte_table_matches_bitwise_reference_for_all_lengths() {
        let mut bytes = vec![0u8; 4096];
        let mut state = 0x243f_6a88u32;
        for byte in &mut bytes {
            state = state.rotate_left(5).wrapping_add(0x9e37_79b9);
            *byte = (state >> 17) as u8;
        }
        for length in 0..=bytes.len() {
            let mut crc = IeeeCrc32::new();
            crc.update(&bytes[..length]);
            assert_eq!(crc.finish(), ieee_bitwise(&bytes[..length]));
        }
    }

    #[test]
    fn incremental_updates_equal_one_shot_and_empty_is_zero() {
        assert_eq!(IeeeCrc32::new().finish(), 0);
        let mut split = IeeeCrc32::new();
        split.update(b"agenterm-");
        split.update(b"con");
        let mut whole = IeeeCrc32::new();
        whole.update(b"agenterm-con");
        assert_eq!(split.finish(), whole.finish());
    }
}
