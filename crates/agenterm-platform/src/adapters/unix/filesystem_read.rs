//! Unix bounded whole-file reader.

use std::{io, io::Read as _, path::Path};

pub fn read_bounded(path: &Path, max_bytes: usize) -> io::Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(max_bytes.saturating_add(1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > max_bytes {
        return Err(crate::filesystem_read::limit_error(max_bytes));
    }
    Ok(bytes)
}
