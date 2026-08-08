//! CONTENT-ADDRESSED — an independent, behaviorally-equivalent file adapter (loop
//! read). Same observable behavior as adapter_v1 for the experiment's input, but
//! different code => different content hash => a DIFFERENT store file. Criterion ④(b):
//! content addressing dedups identical bytes, NOT equivalent behavior.
#![no_std]
#![no_main]

#[path = "../../abi.rs"]
mod abi;
#[path = "../../caps.rs"]
mod caps;
#[path = "../../adapters/fileio_v1alt_logic.rs"]
mod adapter;

use caps::FileCaps;

#[no_mangle]
#[link_section = ".text.entry"]
pub extern "sysv64" fn entry(out: *mut FileCaps) {
    unsafe {
        (*out).read_file = adapter::read_file;
        (*out).write_stdout = adapter::write_stdout;
    }
}
