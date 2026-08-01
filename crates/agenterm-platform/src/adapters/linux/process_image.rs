//! Linux executable-path lookup for one process.

use std::path::PathBuf;

use crate::contract::process_image::{ProcessImageError, ProcessImageErrorKind};

pub(crate) fn executable_path(pid: u32) -> Result<PathBuf, ProcessImageError> {
    if pid == 0 {
        return Err(ProcessImageError::new(
            ProcessImageErrorKind::InvalidId,
            "process ID zero does not identify one process",
        ));
    }
    std::fs::read_link(format!("/proc/{pid}/exe")).map_err(|source| {
        let kind = if source.kind() == std::io::ErrorKind::NotFound {
            ProcessImageErrorKind::NotFound
        } else {
            ProcessImageErrorKind::Query
        };
        ProcessImageError::new(kind, source.to_string())
    })
}
