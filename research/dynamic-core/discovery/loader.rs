//! Q18 DISCOVERY LOADER — the `name -> hash` mechanism under test.
//!
//! ONE source, packed TWICE (isolate the discovery cost as a clean Δ, per SPEC §2):
//!
//!   default (loader_hash):  reads `manifest.txt` of 16-hex HASHES, exactly like Q3's
//!                           content-addressed loader. This is the BUILD-TIME-PINNED
//!                           baseline — there is NO name->hash step at runtime at all
//!                           (candidate 3). The hashes in manifest.txt are the frozen
//!                           output of whatever resolved them when the program was built.
//!
//!   --cfg dc_discover (loader_disc): reads `trust.txt` (one line: the directory file
//!                           THIS consumer trusts), reads that directory (lines
//!                           `name <ws> hash`), reads `prog.txt` (line 1 = payload NAME,
//!                           rest = adapter NAMES), and resolves each name to a hash via
//!                           the trusted directory (candidate 1: multiple directories,
//!                           consumer picks whom to trust). Then it is byte-for-byte the
//!                           same hash->store->map->assemble path as the baseline.
//!
//! Everything below the resolved-hash line is shared, so the Δ between the two builds is
//! precisely the `name->hash` layer (criterion ③). The store, blobs and hashes are Q3's.
#![no_std]
#![no_main]

#[path = "../reuse/abi.rs"]
mod abi;
#[path = "../reuse/prims.rs"]
mod prims;
#[path = "../reuse/caps.rs"]
mod caps;

use abi::Kernel;
use caps::FileCaps;

const MAX_ENTRIES: usize = 8;
const HASHLEN: usize = 16;

// ---- in-kernel file read of a NUL-terminated path (primitives ③④), from Q3. ----
#[cfg(dc_os = "linux")]
fn store_read(k: &Kernel, path: *const u8, buf: *mut u8, cap: usize) -> isize {
    const SYS_READ: usize = 0;
    const SYS_OPEN: usize = 2;
    const SYS_CLOSE: usize = 3;
    let fd = (k.raw_syscall)(SYS_OPEN, path as usize, 0, 0, 0, 0, 0);
    if fd < 0 {
        return -1;
    }
    let n = (k.raw_syscall)(SYS_READ, fd as usize, buf as usize, cap, 0, 0, 0);
    (k.raw_syscall)(SYS_CLOSE, fd as usize, 0, 0, 0, 0, 0);
    n
}
#[cfg(dc_os = "windows")]
fn store_read(k: &Kernel, path: *const u8, buf: *mut u8, cap: usize) -> isize {
    const GENERIC_READ: usize = 0x8000_0000;
    const FILE_SHARE_READ: usize = 0x1;
    const OPEN_EXISTING: usize = 3;
    const INVALID_HANDLE: usize = usize::MAX;
    let create = (k.sym)(b"kernel32.dll\0".as_ptr(), b"CreateFileA\0".as_ptr());
    let cargs = [path as usize, GENERIC_READ, FILE_SHARE_READ, 0, OPEN_EXISTING, 0, 0];
    let h = (k.call)(create, 7, cargs.as_ptr());
    if h == INVALID_HANDLE {
        return -1;
    }
    let mut got: u32 = 0;
    let read = (k.sym)(b"kernel32.dll\0".as_ptr(), b"ReadFile\0".as_ptr());
    let rargs = [h, buf as usize, cap, core::ptr::addr_of_mut!(got) as usize, 0];
    (k.call)(read, 5, rargs.as_ptr());
    let close = (k.sym)(b"kernel32.dll\0".as_ptr(), b"CloseHandle\0".as_ptr());
    (k.call)(close, 1, [h].as_ptr());
    got as isize
}

fn store_path(hash: &[u8; HASHLEN], out: &mut [u8]) {
    let p = b"store/";
    let s = b".bin\0";
    let o = out.as_mut_ptr();
    let mut i = 0usize;
    unsafe {
        for &b in p {
            *o.add(i) = b;
            i += 1;
        }
        for &b in hash.iter() {
            *o.add(i) = b;
            i += 1;
        }
        for &b in s {
            *o.add(i) = b;
            i += 1;
        }
    }
}

fn load_blob(k: &Kernel, hash: &[u8; HASHLEN]) -> *mut u8 {
    const CAP: usize = 65536;
    let mut pathbuf = [0u8; 6 + HASHLEN + 5];
    store_path(hash, &mut pathbuf);
    let mem = (k.mem_alloc)(CAP);
    let n = store_read(k, pathbuf.as_ptr(), mem, CAP);
    if n <= 0 {
        return core::ptr::null_mut();
    }
    (k.mem_protect)(mem, n as usize, true);
    mem
}

// Split `buf` into up to MAX_ENTRIES whitespace-separated tokens of exactly HASHLEN
// hex chars (used by the baseline manifest parser only).
#[cfg(not(dc_discover))]
fn parse_hashes(buf: &[u8], out: &mut [[u8; HASHLEN]; MAX_ENTRIES]) -> usize {
    let mut count = 0;
    let mut tok = [0u8; HASHLEN];
    let mut tl = 0usize;
    for &b in buf {
        let is_hex = matches!(b, b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F');
        if is_hex {
            if tl < HASHLEN {
                unsafe { *tok.get_unchecked_mut(tl) = b };
            }
            tl += 1;
        } else if tl > 0 {
            if tl == HASHLEN && count < MAX_ENTRIES {
                unsafe { *out.get_unchecked_mut(count) = tok };
                count += 1;
            }
            tl = 0;
        }
    }
    if tl == HASHLEN && count < MAX_ENTRIES {
        unsafe { *out.get_unchecked_mut(count) = tok };
        count += 1;
    }
    count
}

// =====================================================================================
// DISCOVERY LAYER (only compiled for loader_disc). This is the entire `name->hash`
// mechanism whose byte cost criterion ③ measures.
// =====================================================================================
#[cfg(dc_discover)]
mod discover {
    use super::{store_read, Kernel, HASHLEN, MAX_ENTRIES};

    const NAMELEN: usize = 16; // max chars in a name token
    const MAX_DIR: usize = 16; // max entries in a directory

    fn is_ws(b: u8) -> bool {
        matches!(b, b' ' | b'\t' | b'\r' | b'\n')
    }
    // Unchecked byte read — freestanding, no libcore panic/fmt machinery linked (the
    // msvc /nodefaultlib link cannot resolve panic_bounds_check's fmt/EH pull-ins).
    #[inline(always)]
    fn at(buf: &[u8], i: usize) -> u8 {
        unsafe { *buf.get_unchecked(i) }
    }

    // A parsed directory: name[i] (len namelen[i]) -> hash[i]. Anyone can author the file.
    pub struct Dir {
        pub names: [[u8; NAMELEN]; MAX_DIR],
        pub namelen: [usize; MAX_DIR],
        pub hashes: [[u8; HASHLEN]; MAX_DIR],
        pub count: usize,
    }

    // Parse `name <ws> 16-hex-hash` lines. Malformed lines are skipped.
    pub fn parse_dir(buf: &[u8]) -> Dir {
        let mut d = Dir {
            names: [[0u8; NAMELEN]; MAX_DIR],
            namelen: [0usize; MAX_DIR],
            hashes: [[0u8; HASHLEN]; MAX_DIR],
            count: 0,
        };
        let mut i = 0usize;
        let n = buf.len();
        while i < n && d.count < MAX_DIR {
            while i < n && is_ws(at(buf, i)) {
                i += 1;
            }
            // name token
            let mut nl = 0usize;
            let mut nm = [0u8; NAMELEN];
            while i < n && !is_ws(at(buf, i)) {
                if nl < NAMELEN {
                    nm[nl] = at(buf, i);
                }
                nl += 1;
                i += 1;
            }
            while i < n && is_ws(at(buf, i)) {
                i += 1;
            }
            // hash token
            let mut hl = 0usize;
            let mut hh = [0u8; HASHLEN];
            while i < n && !is_ws(at(buf, i)) {
                if hl < HASHLEN {
                    hh[hl] = at(buf, i);
                }
                hl += 1;
                i += 1;
            }
            if nl > 0 && nl <= NAMELEN && hl == HASHLEN {
                let c = d.count;
                unsafe {
                    *d.names.get_unchecked_mut(c) = nm;
                    *d.namelen.get_unchecked_mut(c) = nl;
                    *d.hashes.get_unchecked_mut(c) = hh;
                }
                d.count += 1;
            }
        }
        d
    }

    // name -> hash in THIS consumer's trusted directory. None = unresolved name.
    pub fn resolve(d: &Dir, name: &[u8]) -> Option<[u8; HASHLEN]> {
        let mut i = 0usize;
        while i < d.count {
            if unsafe { *d.namelen.get_unchecked(i) } == name.len() {
                let row = unsafe { d.names.get_unchecked(i) };
                let mut eq = true;
                let mut j = 0usize;
                while j < name.len() {
                    if at(row, j) != at(name, j) {
                        eq = false;
                        break;
                    }
                    j += 1;
                }
                if eq {
                    return Some(unsafe { *d.hashes.get_unchecked(i) });
                }
            }
            i += 1;
        }
        None
    }

    // Read prog.txt names (line1 payload, rest adapters) and resolve every one via `d`
    // into the hash array the shared loader path consumes. Returns count, or 0 on any
    // unresolved name (a directory that does not cover a requested name simply misses —
    // there is no global fallback, by design).
    pub fn resolve_prog(
        k: &Kernel,
        d: &Dir,
        prog: &[u8],
        out: &mut [[u8; HASHLEN]; MAX_ENTRIES],
    ) -> usize {
        let _ = k;
        let mut count = 0usize;
        let mut i = 0usize;
        let n = prog.len();
        while i < n && count < MAX_ENTRIES {
            while i < n && is_ws(at(prog, i)) {
                i += 1;
            }
            let start = i;
            while i < n && !is_ws(at(prog, i)) {
                i += 1;
            }
            if i > start {
                let tok = unsafe { core::slice::from_raw_parts(prog.as_ptr().add(start), i - start) };
                match resolve(d, tok) {
                    Some(h) => {
                        unsafe { *out.get_unchecked_mut(count) = h };
                        count += 1;
                    }
                    None => return 0,
                }
            }
        }
        count
    }

    // Read trust.txt -> first token = the directory filename this consumer trusts.
    pub fn read_trusted_dirname(k: &Kernel, buf: &mut [u8]) -> usize {
        let mut t = [0u8; 64];
        let n = store_read(k, b"trust.txt\0".as_ptr(), t.as_mut_ptr(), 64);
        if n <= 0 {
            return 0;
        }
        let n = n as usize;
        let mut i = 0usize;
        while i < n && is_ws(at(&t, i)) {
            i += 1;
        }
        let mut o = 0usize;
        while i < n && !is_ws(at(&t, i)) && o + 1 < buf.len() {
            unsafe { *buf.get_unchecked_mut(o) = *t.get_unchecked(i) };
            o += 1;
            i += 1;
        }
        unsafe { *buf.get_unchecked_mut(o) = 0 }; // NUL-terminate for store_read
        o
    }
}

fn run() -> ! {
    let k = prims::native_table();
    let mut hashes = [[0u8; HASHLEN]; MAX_ENTRIES];

    // ---- obtain the hash list. Baseline reads them; discovery RESOLVES them. ----
    #[cfg(not(dc_discover))]
    let count = {
        let mbuf = (k.mem_alloc)(4096);
        let mn = store_read(&k, b"manifest.txt\0".as_ptr(), mbuf, 4096);
        if mn <= 0 {
            (k.exit)(10)
        }
        let manifest = unsafe { core::slice::from_raw_parts(mbuf, mn as usize) };
        parse_hashes(manifest, &mut hashes)
    };

    #[cfg(dc_discover)]
    let count = {
        // 1. which directory does THIS consumer trust? (no global/default directory)
        let mut dirname = [0u8; 64];
        if discover::read_trusted_dirname(&k, &mut dirname) == 0 {
            (k.exit)(20)
        }
        // 2. read + parse that directory (anyone can author it).
        let dbuf = (k.mem_alloc)(4096);
        let dn = store_read(&k, dirname.as_ptr(), dbuf, 4096);
        if dn <= 0 {
            (k.exit)(21)
        }
        let dir = discover::parse_dir(unsafe { core::slice::from_raw_parts(dbuf, dn as usize) });
        // 3. read prog.txt (names) and resolve every name -> hash via the trusted dir.
        let pbuf = (k.mem_alloc)(4096);
        let pn = store_read(&k, b"prog.txt\0".as_ptr(), pbuf, 4096);
        if pn <= 0 {
            (k.exit)(22)
        }
        let prog = unsafe { core::slice::from_raw_parts(pbuf, pn as usize) };
        let c = discover::resolve_prog(&k, &dir, prog, &mut hashes);
        if c == 0 {
            (k.exit)(23)
        } // unresolved name
        c
    };

    if count < 1 {
        (k.exit)(11)
    }

    // ---- shared from here down: identical to Q3 (hash -> store -> map -> assemble). ----
    let caps_mem = (k.mem_alloc)(core::mem::size_of::<FileCaps>());
    unsafe { core::ptr::write_bytes(caps_mem, 0, core::mem::size_of::<FileCaps>()) };
    let caps = caps_mem as *mut FileCaps;
    let mut i = 1;
    while i < count {
        let base = load_blob(&k, unsafe { hashes.get_unchecked(i) });
        if base.is_null() {
            (k.exit)(12)
        }
        let fill: extern "sysv64" fn(*mut FileCaps) = unsafe { core::mem::transmute(base) };
        fill(caps);
        i += 1;
    }
    let pbase = load_blob(&k, unsafe { hashes.get_unchecked(0) });
    if pbase.is_null() {
        (k.exit)(13)
    }
    let entry: extern "sysv64" fn(*const Kernel, *const FileCaps) -> ! =
        unsafe { core::mem::transmute(pbase) };
    entry(&k as *const Kernel, caps as *const FileCaps)
}

#[cfg(target_os = "linux")]
#[no_mangle]
pub extern "C" fn _start() -> ! {
    run()
}
#[cfg(windows)]
#[no_mangle]
pub extern "C" fn mainCRTStartup() -> ! {
    run()
}
