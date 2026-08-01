//! Fail-closed access to the host cryptographic random number generator.

pub use crate::contract::entropy::{EntropyError, EntropyErrorKind};

/// Fill the complete buffer from the host CSPRNG.
///
/// An empty buffer succeeds. Native short reads are completed, interruptions
/// are retried, and failures are returned without a weaker fallback.
pub fn fill_secure_random(buffer: &mut [u8]) -> Result<(), EntropyError> {
    fill_native(buffer)
}

/// Return a fixed-size array filled by the host CSPRNG.
pub fn secure_random_array<const N: usize>() -> Result<[u8; N], EntropyError> {
    let mut output = [0_u8; N];
    fill_secure_random(&mut output)?;
    Ok(output)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn fill_native(buffer: &mut [u8]) -> Result<(), EntropyError> {
    let mut offset = 0;
    while offset < buffer.len() {
        let written = unsafe {
            libc::getrandom(
                buffer[offset..].as_mut_ptr().cast(),
                buffer.len() - offset,
                0,
            )
        };
        if written > 0 {
            offset += written as usize;
            continue;
        }
        if written == 0 {
            return Err(EntropyError {
                kind: EntropyErrorKind::NoProgress,
                native_code: None,
                message: "getrandom returned zero bytes for a non-empty request".to_owned(),
            });
        }
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        return Err(EntropyError {
            kind: EntropyErrorKind::NativeFailure,
            native_code: error.raw_os_error().map(i64::from),
            message: error.to_string(),
        });
    }
    Ok(())
}

#[cfg(target_vendor = "apple")]
fn fill_native(buffer: &mut [u8]) -> Result<(), EntropyError> {
    unsafe {
        libc::arc4random_buf(buffer.as_mut_ptr().cast(), buffer.len());
    }
    Ok(())
}

#[cfg(windows)]
fn fill_native(buffer: &mut [u8]) -> Result<(), EntropyError> {
    use windows_sys::Win32::Security::Cryptography::{
        BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom,
    };

    for chunk in buffer.chunks_mut(u32::MAX as usize) {
        let status = unsafe {
            BCryptGenRandom(
                std::ptr::null_mut(),
                chunk.as_mut_ptr(),
                chunk.len() as u32,
                BCRYPT_USE_SYSTEM_PREFERRED_RNG,
            )
        };
        if status < 0 {
            return Err(EntropyError {
                kind: EntropyErrorKind::NativeFailure,
                native_code: Some(i64::from(status)),
                message: format!("BCryptGenRandom returned NTSTATUS 0x{:08x}", status as u32),
            });
        }
    }
    Ok(())
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_vendor = "apple",
    windows
)))]
fn fill_native(_buffer: &mut [u8]) -> Result<(), EntropyError> {
    Err(EntropyError {
        kind: EntropyErrorKind::Unavailable,
        native_code: None,
        message: "the current target has no entropy adapter".to_owned(),
    })
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
