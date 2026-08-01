//! Lightweight executable-path lookup without inventory or process ownership.

use std::path::PathBuf;

pub use crate::contract::process_image::{ProcessImageError, ProcessImageErrorKind};

/// Return the executable path for one live host process.
pub fn executable_path(pid: u32) -> Result<PathBuf, ProcessImageError> {
    crate::selected::process_image::executable_path(pid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_the_current_process_image() {
        let path = executable_path(std::process::id()).expect("current process image");
        assert!(path.is_absolute());
        assert!(path.file_name().is_some());
    }

    #[test]
    fn rejects_zero_and_a_missing_process() {
        assert_eq!(
            executable_path(0).unwrap_err().kind(),
            ProcessImageErrorKind::InvalidId
        );
        assert_eq!(
            executable_path(i32::MAX as u32).unwrap_err().kind(),
            ProcessImageErrorKind::NotFound
        );
    }
}
