//! Windows detached-thread adapter using one native FFI entry point.

use crate::threading::ThreadTask;
use std::{
    ffi::c_void,
    io,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr,
};

type Handle = *mut c_void;

struct ThreadStart {
    name: Vec<u16>,
    task: Option<ThreadTask>,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateThread(
        attributes: *const c_void,
        stack_size: usize,
        start: Option<unsafe extern "system" fn(*mut c_void) -> u32>,
        parameter: *mut c_void,
        creation_flags: u32,
        thread_id: *mut u32,
    ) -> Handle;
    fn CloseHandle(handle: Handle) -> i32;
    fn GetCurrentThread() -> Handle;
    fn SetThreadDescription(thread: Handle, description: *const u16) -> i32;
    #[cfg(test)]
    fn GetThreadDescription(thread: Handle, description: *mut *mut u16) -> i32;
    #[cfg(test)]
    fn LocalFree(memory: *mut c_void) -> *mut c_void;
}

#[inline(never)]
pub(crate) fn spawn_named_detached(name: &'static str, task: ThreadTask) -> io::Result<()> {
    let start = Box::new(ThreadStart {
        name: name.encode_utf16().chain(Some(0)).collect(),
        task: Some(task),
    });
    let parameter = Box::into_raw(start);
    let handle = unsafe {
        CreateThread(
            ptr::null(),
            0,
            Some(thread_entry),
            parameter.cast(),
            0,
            ptr::null_mut(),
        )
    };
    if handle.is_null() {
        unsafe { drop(Box::from_raw(parameter)) };
        return Err(io::Error::last_os_error());
    }
    unsafe {
        let _ = CloseHandle(handle);
    }
    Ok(())
}

unsafe extern "system" fn thread_entry(parameter: *mut c_void) -> u32 {
    let mut start = unsafe { Box::from_raw(parameter.cast::<ThreadStart>()) };
    unsafe {
        let _ = SetThreadDescription(GetCurrentThread(), start.name.as_ptr());
    }
    if let Some(task) = start.task.take() {
        let _ = catch_unwind(AssertUnwindSafe(task));
    }
    0
}

#[cfg(test)]
pub(crate) fn current_name() -> Option<String> {
    let mut description = ptr::null_mut();
    let result = unsafe { GetThreadDescription(GetCurrentThread(), &raw mut description) };
    if result < 0 || description.is_null() {
        return None;
    }
    let mut length = 0;
    while unsafe { *description.add(length) } != 0 {
        length += 1;
    }
    let name = String::from_utf16(unsafe { std::slice::from_raw_parts(description, length) }).ok();
    unsafe {
        let _ = LocalFree(description.cast());
    }
    name
}
