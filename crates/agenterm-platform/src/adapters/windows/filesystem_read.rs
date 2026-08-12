//! Windows bounded whole-file reader backed directly by Win32 handles.

use std::{
    io,
    os::windows::{ffi::OsStrExt as _, io::FromRawHandle as _},
    path::Path,
};

use windows_sys::Win32::{
    Foundation::{GENERIC_READ, INVALID_HANDLE_VALUE},
    Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        GetFileSizeEx, OPEN_EXISTING, ReadFile,
    },
};

const READ_CHUNK: usize = 8 * 1024;

pub fn read_bounded(path: &Path, max_bytes: usize) -> io::Result<Vec<u8>> {
    let path = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let raw = unsafe {
        CreateFileW(
            path.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        )
    };
    if raw == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let handle = unsafe {
        // SAFETY: CreateFileW returned a unique owned handle, transferred once.
        std::os::windows::io::OwnedHandle::from_raw_handle(raw)
    };

    let mut size = 0_i64;
    if unsafe { GetFileSizeEx(raw, &mut size) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let size = usize::try_from(size).map_err(|_| crate::filesystem_read::limit_error(max_bytes))?;
    if size > max_bytes {
        return Err(crate::filesystem_read::limit_error(max_bytes));
    }

    let mut bytes = Vec::with_capacity(size);
    let mut chunk = [0_u8; READ_CHUNK];
    loop {
        let mut read = 0_u32;
        if unsafe {
            ReadFile(
                raw,
                chunk.as_mut_ptr(),
                READ_CHUNK as u32,
                &mut read,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read as usize]);
        if bytes.len() > max_bytes {
            return Err(crate::filesystem_read::limit_error(max_bytes));
        }
    }
    drop(handle);
    Ok(bytes)
}
