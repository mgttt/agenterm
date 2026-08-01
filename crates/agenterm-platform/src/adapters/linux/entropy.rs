//! Linux cryptographic entropy adapter.

use crate::contract::entropy::{EntropyError, EntropyErrorKind};

pub(crate) fn fill_secure_random(buffer: &mut [u8]) -> Result<(), EntropyError> {
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
