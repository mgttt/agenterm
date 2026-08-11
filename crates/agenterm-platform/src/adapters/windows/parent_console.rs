//! Windows parent-console output adapter.

pub(crate) fn write_stderr(message: &str) -> bool {
    write(message, true)
}

pub(crate) fn write_stdout(message: &str) -> bool {
    write(message, false)
}

fn write(message: &str, to_stderr: bool) -> bool {
    use std::{
        fs::{File, OpenOptions},
        io::Write as _,
        mem::ManuallyDrop,
        os::windows::{fs::OpenOptionsExt, io::FromRawHandle as _},
    };
    use windows_sys::Win32::{
        Foundation::INVALID_HANDLE_VALUE,
        System::Console::{GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE},
    };

    let payload = format!("{message}\n");
    let std_handle = if to_stderr {
        STD_ERROR_HANDLE
    } else {
        STD_OUTPUT_HANDLE
    };
    let handle = unsafe { GetStdHandle(std_handle) };
    if !handle.is_null() && handle != INVALID_HANDLE_VALUE {
        // A windows-subsystem process can have a valid inherited pipe/file
        // handle while Rust's stdout/stderr singleton was initialized as a
        // sink. Write through the real Win32 handle without owning it.
        let mut stream = ManuallyDrop::new(unsafe { File::from_raw_handle(handle) });
        if stream.write_all(payload.as_bytes()).is_ok() && stream.flush().is_ok() {
            return true;
        }
    }
    let Ok(_guard) = super::console::ConsoleGuard::attach_parent() else {
        return false;
    };
    let mut opts = OpenOptions::new();
    opts.read(true).write(true);
    opts.share_mode(
        windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ
            | windows_sys::Win32::Storage::FileSystem::FILE_SHARE_WRITE,
    );
    opts.open("CONOUT$").is_ok_and(|mut console| {
        console.write_all(payload.as_bytes()).is_ok() && console.flush().is_ok()
    })
}
