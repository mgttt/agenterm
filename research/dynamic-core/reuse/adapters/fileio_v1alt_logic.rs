//! File-I/O adapter — an INDEPENDENT implementation that is BEHAVIORALLY EQUIVALENT
//! to v1 (returns the whole file) but written differently: it reads in a loop until
//! EOF instead of a single read. For the experiment's small input it yields the exact
//! same bytes and count as v1, yet compiles to DIFFERENT bytes => a DIFFERENT content
//! hash. Used by criterion ④(b) to show content addressing dedups identical *bytes*,
//! not equivalent *behavior*.

use crate::abi::Kernel;

#[cfg(dc_os = "linux")]
mod os {
    use crate::abi::Kernel;
    const SYS_READ: usize = 0;
    const SYS_WRITE: usize = 1;
    const SYS_OPEN: usize = 2;
    const SYS_CLOSE: usize = 3;
    const O_RDONLY: usize = 0;
    const STDOUT: usize = 1;
    pub fn read_file(k: &Kernel, path: *const u8, buf: *mut u8, cap: usize) -> isize {
        let fd = (k.raw_syscall)(SYS_OPEN, path as usize, O_RDONLY, 0, 0, 0, 0);
        if fd < 0 {
            return -1;
        }
        let mut total: usize = 0;
        loop {
            if total >= cap {
                break;
            }
            let n = (k.raw_syscall)(
                SYS_READ,
                fd as usize,
                buf as usize + total,
                cap - total,
                0,
                0,
                0,
            );
            if n <= 0 {
                break;
            }
            total += n as usize;
        }
        (k.raw_syscall)(SYS_CLOSE, fd as usize, 0, 0, 0, 0, 0);
        total as isize
    }
    pub fn write_stdout(k: &Kernel, buf: *const u8, len: usize) {
        (k.raw_syscall)(SYS_WRITE, STDOUT, buf as usize, len, 0, 0, 0);
    }
}

#[cfg(dc_os = "windows")]
mod os {
    use crate::abi::Kernel;
    const GENERIC_READ: usize = 0x8000_0000;
    const FILE_SHARE_READ: usize = 0x1;
    const OPEN_EXISTING: usize = 3;
    const INVALID_HANDLE: usize = usize::MAX;
    const STD_OUTPUT_HANDLE: usize = (-11i32) as u32 as usize;
    #[inline]
    fn k32(k: &Kernel, name: *const u8) -> *mut u8 {
        (k.sym)(b"kernel32.dll\0".as_ptr(), name)
    }
    pub fn read_file(k: &Kernel, path: *const u8, buf: *mut u8, cap: usize) -> isize {
        let create = k32(k, b"CreateFileA\0".as_ptr());
        let cargs = [path as usize, GENERIC_READ, FILE_SHARE_READ, 0, OPEN_EXISTING, 0, 0];
        let h = (k.call)(create, 7, cargs.as_ptr());
        if h == INVALID_HANDLE {
            return -1;
        }
        let read = k32(k, b"ReadFile\0".as_ptr());
        let mut total: usize = 0;
        loop {
            if total >= cap {
                break;
            }
            let mut got: u32 = 0;
            let rargs = [
                h,
                buf as usize + total,
                cap - total,
                core::ptr::addr_of_mut!(got) as usize,
                0,
            ];
            (k.call)(read, 5, rargs.as_ptr());
            if got == 0 {
                break;
            }
            total += got as usize;
        }
        let close = k32(k, b"CloseHandle\0".as_ptr());
        (k.call)(close, 1, [h].as_ptr());
        total as isize
    }
    pub fn write_stdout(k: &Kernel, buf: *const u8, len: usize) {
        let gsh = k32(k, b"GetStdHandle\0".as_ptr());
        let h = (k.call)(gsh, 1, [STD_OUTPUT_HANDLE].as_ptr());
        let mut wrote: u32 = 0;
        let write = k32(k, b"WriteFile\0".as_ptr());
        let wargs = [h, buf as usize, len, core::ptr::addr_of_mut!(wrote) as usize, 0];
        (k.call)(write, 5, wargs.as_ptr());
    }
}

pub extern "sysv64" fn read_file(
    k: *const Kernel,
    path: *const u8,
    buf: *mut u8,
    cap: usize,
) -> isize {
    os::read_file(unsafe { &*k }, path, buf, cap)
}
pub extern "sysv64" fn write_stdout(k: *const Kernel, buf: *const u8, len: usize) {
    os::write_stdout(unsafe { &*k }, buf, len)
}
