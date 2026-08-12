//! Bounded whole-file reads without importing broader filesystem authority.

use std::{io, path::Path};

/// Reads one existing file while refusing content beyond `max_bytes`.
pub fn read_bounded(path: &Path, max_bytes: usize) -> io::Result<Vec<u8>> {
    crate::selected::filesystem_read::read_bounded(path, max_bytes)
}

pub(crate) fn limit_error(max_bytes: usize) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("file exceeds {max_bytes} byte limit"),
    )
}

#[cfg(test)]
mod tests {
    use super::read_bounded;

    #[test]
    fn exact_limit_succeeds_and_one_byte_over_fails() {
        let path =
            std::env::temp_dir().join(format!("agenterm-bounded-read-{}", std::process::id()));
        std::fs::write(&path, b"12345").expect("fixture");
        assert_eq!(read_bounded(&path, 5).expect("exact read"), b"12345");
        let error = read_bounded(&path, 4).expect_err("over limit");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        let _ = std::fs::remove_file(path);
    }
}
