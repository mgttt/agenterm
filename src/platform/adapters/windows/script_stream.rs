use std::{
    os::windows::io::AsRawHandle,
    process::{ChildStderr, ChildStdout},
};

pub(crate) fn stdout_probe_token(reader: &ChildStdout) -> Option<usize> {
    Some(reader.as_raw_handle() as usize)
}

pub(crate) fn stderr_probe_token(reader: &ChildStderr) -> Option<usize> {
    Some(reader.as_raw_handle() as usize)
}

pub(crate) fn pipe_available(token: usize) -> Result<usize, bool> {
    use windows_sys::Win32::{
        Foundation::{ERROR_BROKEN_PIPE, ERROR_NO_DATA, GetLastError},
        System::Pipes::PeekNamedPipe,
    };

    let mut available = 0_u32;
    if unsafe {
        PeekNamedPipe(
            token as windows_sys::Win32::Foundation::HANDLE,
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            &mut available,
            std::ptr::null_mut(),
        )
    } != 0
    {
        return Ok(available as usize);
    }
    let error = unsafe { GetLastError() };
    Err(error == ERROR_BROKEN_PIPE || error == ERROR_NO_DATA)
}
