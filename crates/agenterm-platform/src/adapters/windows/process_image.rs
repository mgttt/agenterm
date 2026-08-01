//! Windows executable-path lookup for one process.

use std::{ffi::OsString, os::windows::ffi::OsStringExt, path::PathBuf};

use crate::contract::process_image::{ProcessImageError, ProcessImageErrorKind};

pub(crate) fn executable_path(pid: u32) -> Result<PathBuf, ProcessImageError> {
    if pid == 0 {
        return Err(error(
            ProcessImageErrorKind::InvalidId,
            "process ID zero does not identify one process",
        ));
    }
    use windows_sys::Win32::{
        Foundation::{CloseHandle, ERROR_INVALID_PARAMETER, GetLastError},
        System::Threading::{
            OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
            QueryFullProcessImageNameW,
        },
    };
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        let native = unsafe { GetLastError() };
        let kind = if native == ERROR_INVALID_PARAMETER {
            ProcessImageErrorKind::NotFound
        } else {
            ProcessImageErrorKind::Open
        };
        return Err(error(
            kind,
            std::io::Error::from_raw_os_error(native as i32).to_string(),
        ));
    }
    let mut path = vec![0_u16; 32_768];
    let mut length = path.len() as u32;
    let queried = unsafe {
        QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, path.as_mut_ptr(), &mut length)
    } != 0;
    let native = (!queried).then(|| unsafe { GetLastError() });
    unsafe { CloseHandle(process) };
    if let Some(native) = native {
        return Err(error(
            ProcessImageErrorKind::Query,
            std::io::Error::from_raw_os_error(native as i32).to_string(),
        ));
    }
    let length = usize::try_from(length).map_err(|source| {
        error(
            ProcessImageErrorKind::InvalidData,
            format!("image path length is invalid: {source}"),
        )
    })?;
    if length == 0 || length > path.len() {
        return Err(error(
            ProcessImageErrorKind::InvalidData,
            "native image path is empty or exceeds its buffer",
        ));
    }
    path.truncate(length);
    Ok(PathBuf::from(OsString::from_wide(&path)))
}

fn error(kind: ProcessImageErrorKind, detail: impl Into<String>) -> ProcessImageError {
    ProcessImageError::new(kind, detail)
}
