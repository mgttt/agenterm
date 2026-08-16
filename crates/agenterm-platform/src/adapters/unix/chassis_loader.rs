use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;

use crate::contract::chassis_loader::NativeLoaderError;

pub(crate) fn native_cell() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some("lnx-x86_64"),
        ("linux", "aarch64") => Some("lnx-aarch64"),
        ("macos", "x86_64") => Some("osx-x86_64"),
        ("macos", "aarch64") => Some("osx-aarch64"),
        _ => None,
    }
}

pub(crate) fn validate_executable(path: &Path, bytes: &[u8]) -> Result<(), NativeLoaderError> {
    let metadata = std::fs::symlink_metadata(path).map_err(NativeLoaderError::Inspect)?;
    if metadata.permissions().mode() & 0o111 == 0 {
        return Err(NativeLoaderError::NotExecutable);
    }
    if !bytes.starts_with(native_executable_header()) {
        return Err(NativeLoaderError::InvalidExecutableImage);
    }
    Ok(())
}

pub(crate) fn make_executable(path: &Path) -> Result<(), NativeLoaderError> {
    let mut permissions = std::fs::metadata(path)
        .map_err(NativeLoaderError::Inspect)?
        .permissions();
    permissions.set_mode(permissions.mode() | 0o111);
    std::fs::set_permissions(path, permissions).map_err(NativeLoaderError::SetExecutable)
}

pub(crate) const fn native_executable_header() -> &'static [u8] {
    if cfg!(target_os = "macos") {
        &[0xcf, 0xfa, 0xed, 0xfe]
    } else {
        b"\x7fELF"
    }
}
