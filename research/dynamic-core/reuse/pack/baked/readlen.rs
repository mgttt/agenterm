//! BASELINE (baked-in) — read_len with the file adapter v1 compiled INTO the same
//! blob. The second file-reading payload; in the baked model it carries its OWN copy
//! of the identical adapter (the duplication criterion ① measures).
#![no_std]
#![no_main]

#[path = "../../abi.rs"]
mod abi;
#[path = "../../caps.rs"]
mod caps;
#[path = "../../adapters/fileio_v1_logic.rs"]
mod adapter;
#[path = "../../payloads/readlen_logic.rs"]
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
