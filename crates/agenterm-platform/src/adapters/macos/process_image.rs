//! macOS executable-path lookup for one process.

use std::{
    ffi::{CStr, c_int, c_void},
    os::unix::ffi::OsStringExt,
    path::PathBuf,
};

use crate::contract::process_image::{ProcessImageError, ProcessImageErrorKind};

const PROC_PIDPATHINFO_MAXSIZE: usize = 4 * 1024;

#[link(name = "proc")]
unsafe extern "C" {
    fn proc_pidpath(pid: c_int, buffer: *mut c_void, buffer_size: u32) -> c_int;
}

pub(crate) fn executable_path(pid: u32) -> Result<PathBuf, ProcessImageError> {
    if pid == 0 {
        return Err(error(
            ProcessImageErrorKind::InvalidId,
            "process ID zero does not identify one process",
        ));
    }
    let pid = c_int::try_from(pid)
        .map_err(|source| error(ProcessImageErrorKind::IdOutOfRange, source.to_string()))?;
    let mut path = vec![0_u8; PROC_PIDPATHINFO_MAXSIZE];
    let length = unsafe {
        proc_pidpath(
            pid,
            path.as_mut_ptr().cast(),
            PROC_PIDPATHINFO_MAXSIZE as u32,
        )
    };
    if length <= 0 {
        let native = std::io::Error::last_os_error();
        let kind = match native.raw_os_error() {
            Some(libc::ESRCH) => ProcessImageErrorKind::NotFound,
            _ => ProcessImageErrorKind::Query,
        };
        return Err(error(kind, native.to_string()));
    }
    let c_path = CStr::from_bytes_until_nul(&path).map_err(|source| {
        error(
            ProcessImageErrorKind::InvalidData,
            format!("image path is not terminated: {source}"),
        )
    })?;
    if c_path.to_bytes().is_empty() {
        return Err(error(
            ProcessImageErrorKind::InvalidData,
            "native image path is empty",
        ));
    }
    Ok(PathBuf::from(std::ffi::OsString::from_vec(
        c_path.to_bytes().to_vec(),
    )))
}

fn error(kind: ProcessImageErrorKind, detail: impl Into<String>) -> ProcessImageError {
    ProcessImageError::new(kind, detail)
}
