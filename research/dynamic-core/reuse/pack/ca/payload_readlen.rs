//! CONTENT-ADDRESSED — read_len payload as its OWN blob (NO adapter baked in).
//! References the SAME file-adapter hash as payload_rhp, so on disk the adapter is
//! shared (one store file), not duplicated. Stored as store/<hash>.bin.
#![no_std]
#![no_main]

#[path = "../../abi.rs"]
mod abi;
#[path = "../../caps.rs"]
mod caps;
#[path = "../../payloads/readlen_logic.rs"]
mod payload;

use abi::Kernel;
use caps::FileCaps;

#[no_mangle]
#[link_section = ".text.entry"]
pub extern "sysv64" fn entry(k: *const Kernel, caps: *const FileCaps) -> ! {
    payload::run(k, caps)
}
