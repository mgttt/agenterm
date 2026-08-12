//! Windows atomic file publication adapter.

use std::{
    ffi::OsString,
    os::windows::ffi::{OsStrExt as _, OsStringExt as _},
    path::{Path, PathBuf},
    ptr::null_mut,
    time::Duration,
};

use windows_sys::Win32::{
    Foundation::{ERROR_ACCESS_DENIED, ERROR_LOCK_VIOLATION, ERROR_SHARING_VIOLATION},
    Storage::FileSystem::{
        FILE_ATTRIBUTE_DIRECTORY, GetFileAttributesW, GetFullPathNameW, INVALID_FILE_ATTRIBUTES,
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    },
};

pub fn replace_file(source: &Path, destination: &Path) -> std::io::Result<()> {
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

pub fn normalize_owned_destination(destination: &Path) -> std::io::Result<PathBuf> {
    let destination = full_path(destination)?;
    let parent = destination
        .parent()
        .ok_or_else(|| std::io::Error::other("destination parent required"))?;
    let parent = parent
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let attributes = unsafe {
        // SAFETY: parent is a live NUL-terminated UTF-16 path.
        GetFileAttributesW(parent.as_ptr())
    };
    if attributes == INVALID_FILE_ATTRIBUTES {
        return Err(std::io::Error::last_os_error());
    }
    if attributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotADirectory,
            "destination parent is not a directory",
        ));
    }
    Ok(destination)
}

fn full_path(path: &Path) -> std::io::Result<PathBuf> {
    const INITIAL_UNITS: usize = 512;
    const MAX_UNITS: usize = 32_768;
    let input = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let mut output = vec![0_u16; INITIAL_UNITS];
    loop {
        let length = unsafe {
            // SAFETY: input is NUL-terminated; output is initialized writable
            // storage and no file-part pointer is requested.
            GetFullPathNameW(
                input.as_ptr(),
                output.len() as u32,
                output.as_mut_ptr(),
                null_mut(),
            )
        } as usize;
        if length == 0 {
            return Err(std::io::Error::last_os_error());
        }
        if length < output.len() {
            output.truncate(length);
            return Ok(PathBuf::from(OsString::from_wide(&output)));
        }
        let capacity = (length + 1).max(output.len().saturating_mul(2));
        if capacity > MAX_UNITS {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "absolute destination path exceeds the Windows path bound",
            ));
        }
        output.resize(capacity, 0);
    }
}
