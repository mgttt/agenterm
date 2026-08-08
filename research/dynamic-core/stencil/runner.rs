//! Runner — builds the env table and drives the copy-and-patch applier. NOT part of
//! X_new (X_new = patch.rs). Identical env construction to Q2's runner.rs so the
//! payloads' reachability (kernel primitives + Q0 adapters via sysv64 shims) matches.

use crate::abi::Kernel;
use crate::ir::ENV_LEN;

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
    crate::patch::lower_and_run(k, ir, env.as_ptr())
}
