//! BASELINE (baked-in) — read_hash_print with the file adapter v1 compiled INTO the
//! same blob. This is Q0 variant B's status quo: every file-reading payload carries
//! its own copy of the adapter. Flat PIC blob; entry at offset 0 takes only the
//! Kernel (self-contained). Used by criterion ① as the "adapter duplicated" side.
#![no_std]
#![no_main]

#[path = "../../abi.rs"]
mod abi;
#[path = "../../caps.rs"]
mod caps;
#[path = "../../adapters/fileio_v1_logic.rs"]
mod adapter;
#[path = "../../payloads/rhp_logic.rs"]
mod payload;

use abi::Kernel;
use caps::FileCaps;

#[no_mangle]
#[link_section = ".text.entry"]
pub extern "sysv64" fn entry(k: *const Kernel) -> ! {
    let c = FileCaps {
        read_file: adapter::read_file,
        write_stdout: adapter::write_stdout,
    };
    payload::run(k, &c as *const FileCaps)
}
