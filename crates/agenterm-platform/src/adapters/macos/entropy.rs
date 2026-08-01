//! macOS cryptographic entropy adapter.

use crate::contract::entropy::EntropyError;

pub(crate) fn fill_secure_random(buffer: &mut [u8]) -> Result<(), EntropyError> {
    unsafe {
        libc::arc4random_buf(buffer.as_mut_ptr().cast(), buffer.len());
    }
    Ok(())
}
