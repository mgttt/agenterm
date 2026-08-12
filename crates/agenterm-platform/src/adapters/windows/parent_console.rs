//! Windows parent-console output adapter.

pub(crate) fn write_stderr(message: &str) -> bool {
    write(message, true)
}

pub(crate) fn write_stdout(message: &str) -> bool {
    write(message, false)
}

fn write(message: &str, to_stderr: bool) -> bool {
    use std::os::windows::io::FromRawHandle as _;
    use windows_sys::Win32::{
        Foundation::{GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE},
        Storage::FileSystem::{
            CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
        },
        System::Console::{GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE},
    };

    let std_handle = if to_stderr {
        STD_ERROR_HANDLE
    } else {
        STD_OUTPUT_HANDLE
    };
    let handle = unsafe { GetStdHandle(std_handle) };
    if !handle.is_null() && handle != INVALID_HANDLE_VALUE {
        // A windows-subsystem process can have a valid inherited pipe/file
        // handle while Rust's stdout/stderr singleton was initialized as a
        // sink. Write through the borrowed Win32 handle without owning it.
        if write_handle(handle, message) {
            return true;
        }
    }
    let Ok(_guard) = super::console::ConsoleGuard::attach_parent() else {
        return false;
    };
    const CONOUT: &[u16] = &[b'C' as u16, b'O' as u16, b'N' as u16, b'O' as u16, b'U' as u16, b'T' as u16, b'$' as u16, 0];
    let raw = unsafe {
        CreateFileW(
            CONOUT.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    };
    if raw == INVALID_HANDLE_VALUE {
        return false;
    }
    let console = unsafe {
        // SAFETY: CreateFileW returned a unique owned handle, transferred once.
        std::os::windows::io::OwnedHandle::from_raw_handle(raw)
    };
    let written = write_handle(raw, message);
    drop(console);
    written
}

fn write_handle(handle: windows_sys::Win32::Foundation::HANDLE, message: &str) -> bool {
    use windows_sys::Win32::System::Console::GetConsoleMode;

    let mut mode = 0_u32;
    if unsafe { GetConsoleMode(handle, &mut mode) } != 0 {
        write_console(handle, message)
    } else {
        write_bytes(handle, message.as_bytes()) && write_bytes(handle, b"\n")
    }
}

fn write_bytes(mut_handle: windows_sys::Win32::Foundation::HANDLE, mut bytes: &[u8]) -> bool {
    use windows_sys::Win32::Storage::FileSystem::WriteFile;

    while !bytes.is_empty() {
        let length = bytes.len().min(u32::MAX as usize) as u32;
        let mut written = 0_u32;
        if unsafe {
            WriteFile(
                mut_handle,
                bytes.as_ptr(),
                length,
                &mut written,
                std::ptr::null_mut(),
            )
        } == 0
            || written == 0
        {
            return false;
        }
        bytes = &bytes[written as usize..];
    }
    true
}

fn write_console(handle: windows_sys::Win32::Foundation::HANDLE, message: &str) -> bool {
    use windows_sys::Win32::System::Console::WriteConsoleW;

    let units = message
        .encode_utf16()
        .chain(std::iter::once(b'\n' as u16))
        .collect::<Vec<_>>();
    let mut remaining = units.as_slice();
    while !remaining.is_empty() {
        let length = remaining.len().min(u32::MAX as usize) as u32;
        let mut written = 0_u32;
        if unsafe {
            WriteConsoleW(
                handle,
                remaining.as_ptr().cast(),
                length,
                &mut written,
                std::ptr::null_mut(),
            )
        } == 0
            || written == 0
        {
            return false;
        }
        remaining = &remaining[written as usize..];
    }
    true
}
