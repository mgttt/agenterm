//! Runner — builds the env table and drives the lowerer. Part of the delivered
//! artifact but NOT part of X (X = `lower.rs` only). Identical for both packagings
//! (in-kernel and out-of-kernel).
//!
//! The env table gives lowered code its reachability: kernel primitives (mem_alloc,
//! exit) directly, and the Q0 OS adapters (read_file / write_stdout / spawn_wait)
//! through thin sysv64 shims so every call across the boundary is machine-word ABI.
//! The adapters are Q0's, unchanged: OS-reachability neutrality is Q1's question.

use crate::abi::Kernel;
use crate::ir::ENV_LEN;

// sysv64 shims wrapping the (default-ABI) Q0 adapters.
extern "sysv64" fn sh_read_file(k: *const Kernel, path: *const u8, buf: *mut u8, cap: usize) -> usize {
    crate::fio::read_file(unsafe { &*k }, path, buf, cap) as usize
}
extern "sysv64" fn sh_write(k: *const Kernel, buf: *const u8, len: usize) -> usize {
    crate::fio::write_stdout(unsafe { &*k }, buf, len);
    0
}
extern "sysv64" fn sh_spawn(k: *const Kernel) -> usize {
    crate::proc::spawn_wait(unsafe { &*k }) as i64 as usize
}

static PATH: [u8; 10] = *b"input.txt\0";
static HEX: [u8; 16] = *b"0123456789abcdef";

/// Build env, lower the given IR, and run it. Never returns.
pub fn run(k: &Kernel, ir: &[u8]) -> ! {
    let env: [usize; ENV_LEN] = [
        k as *const Kernel as usize,
        k.mem_alloc as usize,
        sh_read_file as *const () as usize,
        sh_write as *const () as usize,
        sh_spawn as *const () as usize,
        k.exit as usize,
        PATH.as_ptr() as usize,
        HEX.as_ptr() as usize,
    ];
    crate::lower::lower_and_run(k, ir, env.as_ptr())
}
