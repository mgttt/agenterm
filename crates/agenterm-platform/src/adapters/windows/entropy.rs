//! Windows cryptographic entropy adapter.

use windows_sys::Win32::Security::Cryptography::{
    BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom,
};

use crate::contract::entropy::{EntropyError, EntropyErrorKind};

pub(crate) fn fill_secure_random(buffer: &mut [u8]) -> Result<(), EntropyError> {
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
