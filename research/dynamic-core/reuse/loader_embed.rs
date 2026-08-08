//! BASELINE LOADER (criterion ③ reference) — the Q0 variant-B loader: embeds ONE
//! blob via include_bytes, maps it RX, jumps in. No store, no manifest, no file I/O.
//! Its size vs the content-addressed loader.rs is the in-kernel cost of the reuse
//! mechanism (③). The embedded blob is a baked self-contained payload (entry = fn(k)).
#![no_std]
#![no_main]

#[path = "abi.rs"]
mod abi;
#[path = "prims.rs"]
mod prims;

use abi::Kernel;

static DC_BLOB: &[u8] = include_bytes!(env!("DC_BLOB"));

fn load_and_run(blob: &[u8], k: &Kernel) -> ! {
    let n = blob.len();
    let mem = (k.mem_alloc)(n);
    unsafe {
        core::ptr::copy_nonoverlapping(blob.as_ptr(), mem, n);
    }
    (k.mem_protect)(mem, n, true);
    let entry: extern "sysv64" fn(*const Kernel) -> ! = unsafe { core::mem::transmute(mem) };
    entry(k as *const Kernel)
}

fn run() -> ! {
    let k = prims::native_table();
    load_and_run(DC_BLOB, &k)
}

#[cfg(target_os = "linux")]
#[no_mangle]
pub extern "C" fn _start() -> ! {
    run()
}
#[cfg(windows)]
#[no_mangle]
pub extern "C" fn mainCRTStartup() -> ! {
    run()
}
