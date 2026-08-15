use std::path::Path;

use crate::contract::chassis_loader::NativeLoaderError;

pub(crate) fn native_cell() -> Option<&'static str> {
    match std::env::consts::ARCH {
        "x86_64" => Some("win-x86_64"),
        "aarch64" => Some("win-aarch64"),
        _ => None,
    }
}

pub(crate) fn validate_executable(_path: &Path, bytes: &[u8]) -> Result<(), NativeLoaderError> {
    if !bytes.starts_with(native_executable_header()) {
        return Err(NativeLoaderError::InvalidExecutableImage);
    }
    Ok(())
}

pub(crate) fn make_executable(_path: &Path) -> Result<(), NativeLoaderError> {
    Ok(())
}

pub(crate) const fn native_executable_header() -> &'static [u8] {
    b"MZ"
}
