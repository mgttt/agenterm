//! Fail-closed access to the host cryptographic random number generator.

pub use crate::contract::entropy::{EntropyError, EntropyErrorKind};

/// Fill the complete buffer from the host CSPRNG.
///
/// An empty buffer succeeds. Native short reads are completed, interruptions
/// are retried, and failures are returned without a weaker fallback.
pub fn fill_secure_random(buffer: &mut [u8]) -> Result<(), EntropyError> {
    crate::selected::entropy::fill_secure_random(buffer)
}

/// Return a fixed-size array filled by the host CSPRNG.
pub fn secure_random_array<const N: usize>() -> Result<[u8; N], EntropyError> {
    let mut output = [0_u8; N];
    fill_secure_random(&mut output)?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_empty_and_odd_length_buffers() {
        fill_secure_random(&mut []).unwrap();
        for length in [1_usize, 7, 63, 4096] {
            let mut bytes = vec![0_u8; length];
            fill_secure_random(&mut bytes).unwrap();
            assert_eq!(bytes.len(), length);
        }
    }

    #[test]
    fn independent_arrays_are_not_repeated() {
        let first = secure_random_array::<32>().unwrap();
        let second = secure_random_array::<32>().unwrap();
        assert_ne!(first, second);
    }
}
