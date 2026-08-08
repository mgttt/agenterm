//! Payload ② — read a file, hash it (FNV-1a/64), print the hash as hex + newline.
//! Used to measure "total delivery" (criterion ②). OS-agnostic: all platform
//! access goes through `crate::adapter`, which the pack crate root wires to the
//! Linux or Windows adapter. The file path is fixed ("input.txt") to keep the
//! freestanding entry free of argv parsing — the experiment measures bytes, not CLI.

use crate::abi::Kernel;

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 14695981039346656037; // FNV-1a/64 offset basis (0xcbf29ce484222325)
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211); // FNV prime (0x100000001b3)
    }
    h
}

fn hex16(mut v: u64, out: &mut [u8; 17]) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut i = 16usize;
    while i > 0 {
        i -= 1;
        out[i] = HEX[(v & 0xf) as usize];
        v >>= 4;
    }
    out[16] = b'\n';
}

pub fn run(k: &Kernel) -> ! {
    let path = b"input.txt\0";
    // Request a RW read buffer from primitive ① (a payload in a flat, RX-mapped
    // blob cannot rely on static .bss/.data — that region is not writable).
    const CAP: usize = 65536;
    let buf_ptr = (k.mem_alloc)(CAP);
    let n = crate::adapter::read_file(k, path.as_ptr(), buf_ptr, CAP);
    let slice = if n > 0 {
        unsafe { core::slice::from_raw_parts(buf_ptr, n as usize) }
    } else {
        &[]
    };
    let h = fnv1a64(slice);
    let mut out = [0u8; 17];
    hex16(h, &mut out);
    crate::adapter::write_stdout(k, out.as_ptr(), 17);
    (k.exit)(0)
}
