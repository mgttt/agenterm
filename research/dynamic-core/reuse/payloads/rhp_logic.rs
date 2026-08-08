//! Payload — read_hash_print (caps version). Reads "input.txt" through the FileCaps
//! table (whichever file adapter was wired in), FNV-1a/64 hashes it, prints hex.
//! Never touches the adapter's code directly — only the caps table. Same semantics
//! as Q0's read_hash_print, so the hash output is comparable.

use crate::abi::Kernel;
use crate::caps::FileCaps;

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 14695981039346656037;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
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

pub fn run(k: *const Kernel, caps: *const FileCaps) -> ! {
    let kk = unsafe { &*k };
    let c = unsafe { &*caps };
    let path = b"input.txt\0";
    const CAP: usize = 65536;
    let buf = (kk.mem_alloc)(CAP);
    let n = (c.read_file)(k, path.as_ptr(), buf, CAP);
    let slice = if n > 0 {
        unsafe { core::slice::from_raw_parts(buf, n as usize) }
    } else {
        &[]
    };
    let h = fnv1a64(slice);
    let mut out = [0u8; 17];
    hex16(h, &mut out);
    (c.write_stdout)(k, out.as_ptr(), 17);
    (kk.exit)(0)
}
