//! Windows file adapter — OUT of the kernel. Composes the kernel's ③ symbol
//! resolution + ④ call primitives into the two operations a payload needs.
//! Exports no abstraction upward; this is just Win32 glue the payload links.
//! Note: this code only ever calls through the primitive table (sysv64), so it
//! is ABI-uniform with the Linux adapter and needs no direct imports of its own.

use crate::abi::Kernel;

const GENERIC_READ: usize = 0x8000_0000;
const FILE_SHARE_READ: usize = 0x1;
const OPEN_EXISTING: usize = 3;
const INVALID_HANDLE: usize = usize::MAX; // (HANDLE)-1
const STD_OUTPUT_HANDLE: usize = (-11i32) as u32 as usize;

#[inline]
fn k32(k: &Kernel, name: *const u8) -> *mut u8 {
    (k.sym)(b"kernel32.dll\0".as_ptr(), name)
}

/// Read up to `cap` bytes of `path` (NUL-terminated ANSI) into `buf`. Returns count, or -1.
pub fn read_file(k: &Kernel, path: *const u8, buf: *mut u8, cap: usize) -> isize {
    // CreateFileA(path, GENERIC_READ, FILE_SHARE_READ, NULL, OPEN_EXISTING, 0, NULL)
    let create = k32(k, b"CreateFileA\0".as_ptr());
    let cargs = [
        path as usize,
        GENERIC_READ,
        FILE_SHARE_READ,
        0,
        OPEN_EXISTING,
        0,
        0,
    ];
    let h = (k.call)(create, 7, cargs.as_ptr());
    if h == INVALID_HANDLE {
        return -1;
    }
    // ReadFile(h, buf, cap, &read, NULL)
    let mut got: u32 = 0;
    let read = k32(k, b"ReadFile\0".as_ptr());
    let rargs = [h, buf as usize, cap, core::ptr::addr_of_mut!(got) as usize, 0];
    (k.call)(read, 5, rargs.as_ptr());
    let close = k32(k, b"CloseHandle\0".as_ptr());
    (k.call)(close, 1, [h].as_ptr());
    got as isize
}

/// Write `len` bytes to stdout.
pub fn write_stdout(k: &Kernel, buf: *const u8, len: usize) {
    let gsh = k32(k, b"GetStdHandle\0".as_ptr());
    let h = (k.call)(gsh, 1, [STD_OUTPUT_HANDLE].as_ptr());
    let mut wrote: u32 = 0;
    let write = k32(k, b"WriteFile\0".as_ptr());
    let wargs = [h, buf as usize, len, core::ptr::addr_of_mut!(wrote) as usize, 0];
    (k.call)(write, 5, wargs.as_ptr());
}
