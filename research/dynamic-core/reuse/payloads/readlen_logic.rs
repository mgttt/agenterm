//! Payload — read_len (new minimal payload for the reuse experiment). Reads
//! "input.txt" through the SAME FileCaps table the rhp payload uses, and prints the
//! byte count read as "len=XXXX\n" (4 hex digits). Its only reason to exist: create a
//! SECOND independent payload that needs the SAME file adapter, so criterion ① can
//! measure whether that adapter is shared or duplicated.

use crate::abi::Kernel;
use crate::caps::FileCaps;

pub fn run(k: *const Kernel, caps: *const FileCaps) -> ! {
    let kk = unsafe { &*k };
    let c = unsafe { &*caps };
    let path = b"input.txt\0";
    const CAP: usize = 65536;
    let buf = (kk.mem_alloc)(CAP);
    let n = (c.read_file)(k, path.as_ptr(), buf, CAP);
    let v = if n < 0 { 0u32 } else { n as u32 };
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = [b'l', b'e', b'n', b'=', b'0', b'0', b'0', b'0', b'\n'];
    let mut i = 7usize;
    let mut x = v;
    loop {
        out[i] = HEX[(x & 0xf) as usize];
        x >>= 4;
        if i == 4 {
            break;
        }
        i -= 1;
    }
    (c.write_stdout)(k, out.as_ptr(), out.len());
    (kk.exit)(0)
}
