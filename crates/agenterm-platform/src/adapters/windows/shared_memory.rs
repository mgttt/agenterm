//! Windows page-file-backed named shared memory.

use crate::contract::shared_memory::{SharedMemoryError, SharedMemoryErrorKind};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND, GetLastError, HANDLE,
        INVALID_HANDLE_VALUE,
    },
    System::Memory::{
        CreateFileMappingW, FILE_MAP_ALL_ACCESS, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile,
        OpenFileMappingW, PAGE_READWRITE, UnmapViewOfFile,
    },
};

pub(crate) struct SharedMemory {
    handle: HANDLE,
    view: MEMORY_MAPPED_VIEW_ADDRESS,
}

impl SharedMemory {
    pub(crate) fn create(name: &str, len: usize) -> Result<Self, SharedMemoryError> {
        let wide = native_name(name);
        let size = len as u64;
        let handle = unsafe {
            CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                std::ptr::null(),
                PAGE_READWRITE,
                (size >> 32) as u32,
                size as u32,
                wide.as_ptr(),
            )
        };
        if handle.is_null() {
            return Err(last_error(SharedMemoryErrorKind::Create));
        }
        let native_error = unsafe { GetLastError() };
        if native_error == ERROR_ALREADY_EXISTS {
            unsafe { CloseHandle(handle) };
            return Err(SharedMemoryError::new(
                SharedMemoryErrorKind::AlreadyExists,
                "a shared-memory object already uses this name",
            ));
        }
        Self::map(handle, len)
    }

    pub(crate) fn open(name: &str, len: usize) -> Result<Self, SharedMemoryError> {
        let wide = native_name(name);
        let handle = unsafe { OpenFileMappingW(FILE_MAP_ALL_ACCESS, 0, wide.as_ptr()) };
        if handle.is_null() {
            let native_error = unsafe { GetLastError() };
            let kind = if native_error == ERROR_FILE_NOT_FOUND {
                SharedMemoryErrorKind::NotFound
            } else {
                SharedMemoryErrorKind::Open
            };
            return Err(SharedMemoryError::new(
                kind,
                std::io::Error::from_raw_os_error(native_error as i32).to_string(),
            ));
        }
        Self::map(handle, len)
    }

    fn map(handle: HANDLE, len: usize) -> Result<Self, SharedMemoryError> {
        let view = unsafe { MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, len) };
        if view.Value.is_null() {
            let error = last_error(SharedMemoryErrorKind::Map);
            unsafe { CloseHandle(handle) };
            return Err(error);
        }
        Ok(Self { handle, view })
    }

    pub(crate) fn as_ptr(&self) -> *const u8 {
        self.view.Value.cast()
    }

    pub(crate) fn as_mut_ptr(&mut self) -> *mut u8 {
        self.view.Value.cast()
    }
}

impl Drop for SharedMemory {
    fn drop(&mut self) {
        unsafe {
            UnmapViewOfFile(self.view);
            CloseHandle(self.handle);
        }
    }
}

fn native_name(name: &str) -> Vec<u16> {
    format!("Local\\{name}")
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect()
}

fn last_error(kind: SharedMemoryErrorKind) -> SharedMemoryError {
    SharedMemoryError::new(kind, std::io::Error::last_os_error().to_string())
}
