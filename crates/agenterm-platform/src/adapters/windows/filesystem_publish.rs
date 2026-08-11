//! Windows atomic file publication adapter.

use std::{os::windows::ffi::OsStrExt as _, path::Path, time::Duration};

use windows_sys::Win32::{
    Foundation::{ERROR_ACCESS_DENIED, ERROR_LOCK_VIOLATION, ERROR_SHARING_VIOLATION},
    Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW},
};

pub fn replace_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    let source = std::fs::canonicalize(source)?;
    let destination = std::fs::canonicalize(
        destination
            .parent()
            .ok_or_else(|| std::io::Error::other("destination parent required"))?,
    )?
    .join(
        destination
            .file_name()
            .ok_or_else(|| std::io::Error::other("destination name required"))?,
    );
    let source = source
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    const ATTEMPTS: usize = 32;
    for attempt in 0..ATTEMPTS {
        if unsafe {
            MoveFileExW(
                source.as_ptr(),
                destination.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } != 0
        {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        let retryable = matches!(
            error.raw_os_error(),
            Some(code)
                if code == ERROR_ACCESS_DENIED as i32
                    || code == ERROR_SHARING_VIOLATION as i32
                    || code == ERROR_LOCK_VIOLATION as i32
        );
        if !retryable || attempt + 1 == ATTEMPTS {
            return Err(error);
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    unreachable!("bounded replacement loop always returns")
}

pub fn sync_parent(_parent: &Path) -> std::io::Result<()> {
    // MOVEFILE_WRITE_THROUGH owns the Windows durability barrier.
    Ok(())
}
