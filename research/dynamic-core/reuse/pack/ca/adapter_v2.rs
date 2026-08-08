//! CONTENT-ADDRESSED — file adapter v2 (truncated read) as its OWN blob. Different
//! bytes than v1 => different hash => a DIFFERENT store file. Both coexist with no
//! registry and no anointing; each payload binds to the version it named (criterion ②).
#![no_std]
#![no_main]

#[path = "../../abi.rs"]
mod abi;
#[path = "../../caps.rs"]
mod caps;
#[path = "../../adapters/fileio_v2_logic.rs"]
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
