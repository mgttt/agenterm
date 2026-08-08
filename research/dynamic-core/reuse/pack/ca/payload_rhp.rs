//! CONTENT-ADDRESSED — read_hash_print payload as its OWN blob (NO adapter baked in).
//! Entry takes the Kernel and a FileCaps the loader already filled from a separately
//! loaded adapter blob. Stored as store/<hash>.bin.
#![no_std]
#![no_main]

#[path = "../../abi.rs"]
mod abi;
#[path = "../../caps.rs"]
mod caps;
#[path = "../../payloads/rhp_logic.rs"]
mod payload;

use abi::Kernel;
use caps::FileCaps;

#[no_mangle]
#[link_section = ".text.entry"]
pub extern "sysv64" fn entry(k: *const Kernel, caps: *const FileCaps) -> ! {
    payload::run(k, caps)
}
