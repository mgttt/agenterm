//! CONTENT-ADDRESSED LOADER — the reuse mechanism under test (in-kernel, criterion ③).
//!
//! It is a UNIVERSAL, content-blind, frozen loader: the same binary runs any program.
//! A "program" is data — a `manifest.txt` in the CWD listing content hashes (16 hex
//! chars each, one per line): line 1 = the payload blob, the rest = adapter blobs.
//! The loader resolves each hash to `store/<hash>.bin`, reads it with primitives ③④,
//! maps it RX (①), fills a FileCaps from the adapter blobs, then jumps into the
//! payload passing (Kernel, caps).
//!
//! This is deliberately NOT a package manager: no registry, no version solving, no
//! dependency graph. Just hash -> file -> map -> assemble. Its code is O(1) in the
//! number of adapters (one loop), per §1.1.
//!
//! Two builds:
//!   - default : trusts the store (filename = content). No hashing in the TCB.
//!   - dc_verify: recomputes FNV-1a/64 over each loaded blob and compares to the
//!     requested hash (the integrity property). Measures ③'s "verify" column.
#![no_std]
#![no_main]

#[path = "abi.rs"]
mod abi;
#[path = "prims.rs"]
mod prims;
#[path = "caps.rs"]
mod caps;

use abi::Kernel;
use caps::FileCaps;

const MAX_ENTRIES: usize = 8;
const HASHLEN: usize = 16; // FNV-1a/64 as 16 hex chars

// --- in-kernel file read (the store lives on disk; reached via primitives ③④). ---
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

// Build "store/<hash>.bin\0" into `out` (>= 6+16+4+1). Returns nothing.
fn store_path(hash: &[u8; HASHLEN], out: &mut [u8]) {
    let p = b"store/";
    let s = b".bin\0";
    let o = out.as_mut_ptr();
    let mut i = 0usize;
    // Unchecked writes: freestanding, no libcore panic machinery linked (also keeps
    // the loader free of panic_bounds_check, which the direct-lld link cannot resolve).
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

#[cfg(dc_verify)]
fn hex_to_u64(h: &[u8; HASHLEN]) -> u64 {
    let mut v: u64 = 0;
    for &c in h.iter() {
        let d = match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => 0,
        };
        v = (v << 4) | d as u64;
    }
    v
}
#[cfg(dc_verify)]
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 14695981039346656037;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    h
}

// Read + map a blob RX; returns its base pointer, or null on failure.
fn load_blob(k: &Kernel, hash: &[u8; HASHLEN]) -> *mut u8 {
    const CAP: usize = 65536;
    let mut pathbuf = [0u8; 6 + HASHLEN + 5];
    store_path(hash, &mut pathbuf);
    let mem = (k.mem_alloc)(CAP);
    let n = store_read(k, pathbuf.as_ptr(), mem, CAP);
    if n <= 0 {
        return core::ptr::null_mut();
    }
    #[cfg(dc_verify)]
    {
        let got = fnv1a64(unsafe { core::slice::from_raw_parts(mem, n as usize) });
        if got != hex_to_u64(hash) {
            return core::ptr::null_mut(); // integrity failure
        }
    }
    (k.mem_protect)(mem, n as usize, true);
    mem
}

// Parse manifest bytes into up to MAX_ENTRIES 16-hex-char hashes. Returns count.
fn parse_manifest(buf: &[u8], out: &mut [[u8; HASHLEN]; MAX_ENTRIES]) -> usize {
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

fn run() -> ! {
    let k = prims::native_table();
    // 1. read manifest.txt from CWD.
    let mbuf = (k.mem_alloc)(4096);
    let mn = store_read(&k, b"manifest.txt\0".as_ptr(), mbuf, 4096);
    if mn <= 0 {
        (k.exit)(10)
    }
    let manifest = unsafe { core::slice::from_raw_parts(mbuf, mn as usize) };
    let mut hashes = [[0u8; HASHLEN]; MAX_ENTRIES];
    let count = parse_manifest(manifest, &mut hashes);
    if count < 1 {
        (k.exit)(11)
    }
    // 2. load adapter blobs (entries 1..count), filling one shared FileCaps.
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
    // 3. load payload blob (entry 0) and jump in with (Kernel, caps).
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
