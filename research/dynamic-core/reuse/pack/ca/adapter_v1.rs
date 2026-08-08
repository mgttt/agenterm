//! CONTENT-ADDRESSED — file adapter v1 as its OWN blob. Entry at offset 0 fills the
//! caller's FileCaps with pointers into this blob's mapping. Stored once in the
//! content store as store/<hash>.bin; any number of payloads reference it by hash, so
//! its bytes exist ONCE on disk regardless of how many payloads use it (criterion ①).
#![no_std]
#![no_main]

#[path = "../../abi.rs"]
mod abi;
#[path = "../../caps.rs"]
mod caps;
#[path = "../../adapters/fileio_v1_logic.rs"]
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
