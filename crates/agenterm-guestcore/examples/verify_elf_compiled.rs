//! Verification program (d) -- the real bar this round's task set, stronger
//! than (a)/(b)/(c): loads and runs a REAL, genuinely-compiled x86_64 Linux
//! ELF binary (not hand-encoded machine code) through this crate's real
//! `elf::load_elf` loader and the completely UNCHANGED interpreter pipeline.
//! Takes the compiled binary's path as `argv[1]` (see the crate README's
//! ELF section for exactly how that binary was built and what it does).
//!
//! `ExitMode::Real` -- same reason every other `verify_*` example uses it
//! (the guest's real `exit` syscall really calls `ExitProcess`, which would
//! kill an in-process `cargo test` harness; see `tests/verify.rs`'s header).

use std::env;
use std::fs;

use agenterm_guestcore::elf;
use agenterm_guestcore::intent_map::ExitMode;

fn main() {
    let path = env::args().nth(1).expect("usage: verify_elf_compiled <path-to-compiled-elf>");
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("failed to read {path}: {e}"));
    let image = elf::load_elf(&bytes, &[b"tiny"], 64 * 1024).unwrap_or_else(|e| panic!("load_elf failed on {path}: {e}"));
    // Never returns: guest reaches `exit`, which really calls `ExitProcess`.
    let _ = agenterm_guestcore::run_guest(&image, ExitMode::Real);
    unreachable!("run_guest(ExitMode::Real) diverges on exit -- guest must not have reached it");
}
