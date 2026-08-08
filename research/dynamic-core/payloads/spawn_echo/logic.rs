//! Payload ③ (criterion ④'s new capability) — spawn a child process, wait for it,
//! print its exit code as "exit=NN\n", and exit with that same code.
//!
//! OS-agnostic: it spawns via `crate::adapter::spawn_wait`, which the pack crate
//! root wires to the Linux (fork/execve/wait4) or Windows (CreateProcess/Wait)
//! adapter. The child is fixed to return 7, so a third party can verify: the
//! program prints "exit=07" and exits 7.

use crate::abi::Kernel;

pub fn run(k: &Kernel) -> ! {
    let code = crate::adapter::spawn_wait(k);
    // Format "exit=NN\n" (two decimal digits; the fixed child returns 7).
    let mut out = [b'e', b'x', b'i', b't', b'=', b'0', b'0', b'\n'];
    let c = if code < 0 { 99 } else { code as u32 };
    out[5] = b'0' + ((c / 10) % 10) as u8;
    out[6] = b'0' + (c % 10) as u8;
    crate::adapter::write_stdout(k, out.as_ptr(), out.len());
    (k.exit)((code as i64 as usize) & 0xff)
}
