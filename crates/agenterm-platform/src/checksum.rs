//! Compact dependency-free checksums for platform data contracts.

const IEEE_POLYNOMIAL: u32 = 0xedb8_8320;

const fn nibble_table() -> [u32; 16] {
    let mut table = [0; 16];
    let mut index = 0;
    while index < table.len() {
        let mut value = index as u32;
        let mut bit = 0;
        while bit < 4 {
            value = (value >> 1) ^ (IEEE_POLYNOMIAL & 0u32.wrapping_sub(value & 1));
            bit += 1;
        }
        table[index] = value;
        index += 1;
    }
    table
}

const IEEE_NIBBLE_TABLE: [u32; 16] = nibble_table();

/// Incremental IEEE CRC-32 used by PNG, ZIP and Ethernet.
///
/// This is deliberately distinct from the CRC-32C polynomial implemented by
/// x86 SSE4.2 and Arm CRC instructions. The 16-entry nibble table keeps code
/// and read-only data compact while reducing each byte from eight polynomial
/// rounds to two.
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
            crc ^= u32::from(byte);
            crc = (crc >> 4) ^ IEEE_NIBBLE_TABLE[(crc & 0x0f) as usize];
            crc = (crc >> 4) ^ IEEE_NIBBLE_TABLE[(crc & 0x0f) as usize];
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

    #[test]
    fn standard_ieee_vector_matches() {
        let mut crc = IeeeCrc32::new();
        crc.update(b"123456789");
        assert_eq!(crc.finish(), 0xcbf4_3926);
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
