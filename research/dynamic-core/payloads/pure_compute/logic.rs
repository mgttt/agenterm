//! Payload ① — pure computation. Touches no OS surface except process exit.
//! Used to measure the "floor" (criterion ①). Deterministic: the low byte of the
//! result is returned as the process exit code so a third party can verify it.

use crate::abi::Kernel;

#[inline(never)]
fn compute() -> u64 {
    // A fixed FNV-ish mix over a fixed range. No inputs, no OS.
    let mut acc: u64 = 1469598103934665603;
    let mut i: u64 = 0;
    while i < 1_000_000 {
        acc = acc.wrapping_mul(1099511628211).wrapping_add(i);
        i = i.wrapping_add(1);
    }
    acc
}

pub fn run(k: &Kernel) -> ! {
    let r = compute();
    (k.exit)((r & 0xff) as usize)
}
