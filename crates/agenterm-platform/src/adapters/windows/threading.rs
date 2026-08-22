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
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    #[cfg(test)]
    fn GetThreadDescription(thread: Handle, description: *mut *mut u16) -> i32;
    #[cfg(test)]
    fn LocalFree(memory: *mut c_void) -> *mut c_void;
}

/// `SetThreadDescription`, resolved at run time rather than imported.
///
/// The documentation says Windows 10 version 1607, and 1607 is exactly
/// Windows Server 2016 — but on 1607 the function is only implemented in
/// `KernelBase.dll`; the `kernel32` forwarder did not appear until 1703. A
/// static import is therefore resolved by the PE loader against a `kernel32`
/// that does not export it, and the process dies before `main` with an
/// entry-point dialog. For a call whose entire purpose is to put a readable
/// label on a thread in a debugger, that is an absurd price.
///
/// Both modules are already loaded into every Win32 process, so
/// `GetModuleHandleW` borrows the loader's references and nothing is freed.
mod thread_naming {
    use super::Handle;
    use std::ffi::c_void;
    use std::sync::OnceLock;

    type SetDescription = unsafe extern "system" fn(Handle, *const u16) -> i32;
    /// What `GetProcAddress` returns before it is given a signature.
    type Resolved = *mut c_void;

    /// `None` records a system without the export. Resolution is attempted
    /// once: a missing export does not appear later in the same process.
    static ENTRY: OnceLock<Option<SetDescription>> = OnceLock::new();

    fn resolve() -> Option<SetDescription> {
        // kernel32 first because that is where the forwarder lives on every
        // system new enough to have one; KernelBase is the 1607 fallback.
        for module in ["kernel32.dll\0", "KernelBase.dll\0"] {
            let wide = module.encode_utf16().collect::<Vec<_>>();
            // SAFETY: both names are NUL terminated, and GetModuleHandleW
            // borrows an existing loader reference without adding one.
            let handle = unsafe { super::GetModuleHandleW(wide.as_ptr()) };
            if handle.is_null() {
                continue;
            }
            // SAFETY: the handle is live for the process lifetime and the name
            // is a NUL-terminated C string. The transmute target is the
            // documented signature of `SetThreadDescription`.
            let address =
                unsafe { super::GetProcAddress(handle, c"SetThreadDescription".as_ptr().cast()) };
            if !address.is_null() {
                return Some(unsafe { std::mem::transmute::<Resolved, SetDescription>(address) });
            }
        }
        None
    }

    /// Labels the calling thread where the system can. Naming a thread is a
    /// diagnostic aid with no observable effect on behavior, so a system that
    /// cannot do it simply does not — there is nothing for a caller to handle.
    pub(super) fn describe(thread: Handle, name: *const u16) {
        if let Some(set) = ENTRY.get_or_init(resolve) {
            // SAFETY: `thread` is a live thread handle and `name` points at a
            // NUL-terminated wide string owned by the caller for this call.
            unsafe {
                let _ = set(thread, name);
            }
        }
    }
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
    // SAFETY: GetCurrentThread returns the pseudo-handle for this thread and
    // `start` owns the NUL-terminated name for the duration of the call.
    unsafe { thread_naming::describe(GetCurrentThread(), start.name.as_ptr()) };
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
