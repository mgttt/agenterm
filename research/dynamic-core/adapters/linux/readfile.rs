//! Linux file adapter — OUT of the kernel. Composes the kernel's ③ raw_syscall
//! primitive into the two operations a payload needs. No abstraction is exported
//! upward; this is just Linux glue the payload happens to link.

use crate::abi::Kernel;

const SYS_READ: usize = 0;
const SYS_WRITE: usize = 1;
const SYS_OPEN: usize = 2;
const SYS_CLOSE: usize = 3;
const O_RDONLY: usize = 0;
const STDOUT: usize = 1;

/// Read up to `cap` bytes of `path` (NUL-terminated) into `buf`. Returns count, or -1.
pub fn read_file(k: &Kernel, path: *const u8, buf: *mut u8, cap: usize) -> isize {
    let fd = (k.raw_syscall)(SYS_OPEN, path as usize, O_RDONLY, 0, 0, 0, 0);
    if fd < 0 {
        return -1;
    }
    let n = (k.raw_syscall)(SYS_READ, fd as usize, buf as usize, cap, 0, 0, 0);
    (k.raw_syscall)(SYS_CLOSE, fd as usize, 0, 0, 0, 0, 0);
    n
}

/// Write `len` bytes to stdout.
pub fn write_stdout(k: &Kernel, buf: *const u8, len: usize) {
    (k.raw_syscall)(SYS_WRITE, STDOUT, buf as usize, len, 0, 0, 0);
}
