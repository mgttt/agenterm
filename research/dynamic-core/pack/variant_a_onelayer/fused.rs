//! Variant A (1 layer), FUSED PRODUCT — the honest "one product, all capabilities"
//! model of 1-layer (spec §0/§1.3: "机制与平台适配同为一个产物"). Unlike the
//! per-payload A_* binaries (which compile one artifact per payload and thus dead-
//! strip unused capabilities), this is a SINGLE artifact that can do BOTH file I/O
//! and spawn, dispatching at run time. Because dispatch is a runtime decision, both
//! capabilities are reachable and therefore linked in — so a file-only invocation
//! STILL ships the spawn adapter + spawn payload.
//!
//! This exists to measure criterion ④'s (b): "how much does a non-user of the new
//! capability grow?" (b) for true 1-layer = size(fused) - size(A_rhp file-only).
//! (2-layer's (b) is 0 structurally: rhp stays its own blob; spawn is a new blob.)
#![no_std]
#![no_main]

#[path = "../../core/abi.rs"]
mod abi;
#[path = "../../core/kernel.rs"]
mod kernel;

// One combined adapter surface. Both platform adapters define write_stdout with
// identical semantics; we re-export exactly one. read_file / spawn_wait are unique.
mod adapter {
    // NOTE: an inline module shifts the #[path] base into an `adapter/` subdir,
    // so these paths carry one extra `../` relative to the top-level pack roots.
    #[cfg(dc_os = "linux")]
    #[path = "../../../adapters/linux/readfile.rs"]
    mod file;
    #[cfg(dc_os = "windows")]
    #[path = "../../../adapters/windows/readfile.rs"]
    mod file;
    #[cfg(dc_os = "linux")]
    #[path = "../../../adapters/linux/spawn.rs"]
    mod spawn;
    #[cfg(dc_os = "windows")]
    #[path = "../../../adapters/windows/spawn.rs"]
    mod spawn;
    pub use file::{read_file, write_stdout};
    pub use spawn::spawn_wait;
}

#[path = "../../payloads/read_hash_print/logic.rs"]
mod rhp;
#[path = "../../payloads/spawn_echo/logic.rs"]
mod spawn_payload;

use abi::Kernel;

#[no_mangle]
pub extern "sysv64" fn agent_main(k: *const Kernel) -> ! {
    let kr = unsafe { &*k };
    // Runtime selector the optimizer cannot fold away -> BOTH capabilities are
    // reachable and linked. The "normal" path (sel != 1) runs the file-only
    // capability, yet the spawn capability is carried regardless.
    let sel = core::hint::black_box(0u8);
    if sel == 1 {
        spawn_payload::run(kr)
    } else {
        rhp::run(kr)
    }
}
