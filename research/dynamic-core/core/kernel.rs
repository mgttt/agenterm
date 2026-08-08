//! The dynamic core (kernel) — Linux/x86_64 implementation of the four primitives.
//!
//! Included ONLY by kernel-bearing artifacts (variant A binaries and the variant B
//! loader). Payload blobs do NOT include this file; they only see `crate::abi::Kernel`.
//!
//! Contains no cross-platform semantics: only ① memory, ② jump (as the loader),
//! ③ raw_syscall + sym, ④ call. File/IO semantics live in adapters, not here.

use crate::abi::Kernel;

// ---------------------------------------------------------------------------
// Freestanding memory intrinsics. With no libc, codegen still emits calls to
// these for bulk copies/inits (e.g. the loader's blob copy). Tiny, self-contained.
// ---------------------------------------------------------------------------
#[no_mangle]
pub unsafe extern "C" fn memcpy(dst: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    let mut i = 0;
    while i < n {
        *dst.add(i) = *src.add(i);
        i += 1;
    }
    dst
}
#[no_mangle]
pub unsafe extern "C" fn memset(dst: *mut u8, c: i32, n: usize) -> *mut u8 {
    let mut i = 0;
    while i < n {
        *dst.add(i) = c as u8;
        i += 1;
    }
    dst
}
#[no_mangle]
pub unsafe extern "C" fn memmove(dst: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    if (dst as usize) < (src as usize) {
        let mut i = 0;
        while i < n {
            *dst.add(i) = *src.add(i);
            i += 1;
        }
    } else {
        let mut i = n;
        while i > 0 {
            i -= 1;
            *dst.add(i) = *src.add(i);
        }
    }
    dst
}
#[no_mangle]
pub unsafe extern "C" fn memcmp(a: *const u8, b: *const u8, n: usize) -> i32 {
    let mut i = 0;
    while i < n {
        let (x, y) = (*a.add(i), *b.add(i));
        if x != y {
            return x as i32 - y as i32;
        }
        i += 1;
    }
    0
}

// ===========================================================================
// LINUX x86_64 implementation of the four primitives.
// ===========================================================================

// ---------------------------------------------------------------------------
// ③ reachability — raw syscall (Linux path)
// ---------------------------------------------------------------------------
#[cfg(target_os = "linux")]
pub extern "sysv64" fn raw_syscall(
    n: usize,
    a: usize,
    b: usize,
    c: usize,
    d: usize,
    e: usize,
    f: usize,
) -> isize {
    let ret: isize;
    unsafe {
        core::arch::asm!(
            "syscall",
            inout("rax") n => ret,
            in("rdi") a,
            in("rsi") b,
            in("rdx") c,
            in("r10") d,
            in("r8") e,
            in("r9") f,
            out("rcx") _,
            out("r11") _,
            options(nostack),
        );
    }
    ret
}

// Linux syscall numbers used by the kernel's own primitives.
#[cfg(target_os = "linux")]
const SYS_MMAP: usize = 9;
#[cfg(target_os = "linux")]
const SYS_MPROTECT: usize = 10;
#[cfg(target_os = "linux")]
const SYS_EXIT: usize = 60;

// ---------------------------------------------------------------------------
// ① memory — reserve+commit (RW) and protect (RW <-> RX)
// ---------------------------------------------------------------------------
#[cfg(target_os = "linux")]
pub extern "sysv64" fn mem_alloc(size: usize) -> *mut u8 {
    // mmap(NULL, size, PROT_READ|WRITE, MAP_PRIVATE|MAP_ANONYMOUS, -1, 0)
    const PROT_RW: usize = 0x3;
    const MAP_PRIVATE_ANON: usize = 0x22;
    let p = raw_syscall(SYS_MMAP, 0, size, PROT_RW, MAP_PRIVATE_ANON, usize::MAX, 0);
    p as *mut u8
}

#[cfg(target_os = "linux")]
pub extern "sysv64" fn mem_protect(ptr: *mut u8, size: usize, exec: bool) {
    const PROT_RX: usize = 0x5; // READ|EXEC
    const PROT_RW: usize = 0x3;
    let prot = if exec { PROT_RX } else { PROT_RW };
    raw_syscall(SYS_MPROTECT, ptr as usize, size, prot, 0, 0, 0);
}

// ---------------------------------------------------------------------------
// ③ symbol resolution (dlsym / GetProcAddress). Unused on the Linux path
// (file access goes through raw_syscall) — present as a primitive placeholder.
// ---------------------------------------------------------------------------
#[cfg(target_os = "linux")]
pub extern "sysv64" fn sym(_module: *const u8, _name: *const u8) -> *mut u8 {
    core::ptr::null_mut()
}

// ---------------------------------------------------------------------------
// ④ call — data-driven native call for the integer/pointer subset.
// The "signature description" is `nargs`; every argument is a machine word
// (int or pointer). Floats and structs-by-value are out of scope. On Linux the
// native convention is sysv64.
//
// Phase 3 (the "+1 capability" step, criterion ④): the arg-count ceiling was
// raised from 7 to 11. The spawn capability needs CreateProcessA (10 args) on
// Windows; the register-only 7-arm table could not express it. This is an
// in-kernel cost of adding a capability — see RESULTS §④. It is a ONE-TIME
// completion of ④ toward its libffi model (stack args), not per-capability
// growth: a fully general (asm, variadic) ④ would absorb any arity at zero
// further in-kernel cost. Recorded honestly rather than hidden.
// ---------------------------------------------------------------------------
#[cfg(target_os = "linux")]
pub extern "sysv64" fn call(addr: *mut u8, nargs: usize, args: *const usize) -> usize {
    unsafe {
        let a = core::slice::from_raw_parts(args, nargs);
        macro_rules! t {
            ($($p:ty),*) => { core::mem::transmute::<_, extern "sysv64" fn($($p),*) -> usize>(addr) };
        }
        match nargs {
            0 => (t!())(),
            1 => (t!(usize))(a[0]),
            2 => (t!(usize, usize))(a[0], a[1]),
            3 => (t!(usize, usize, usize))(a[0], a[1], a[2]),
            4 => (t!(usize, usize, usize, usize))(a[0], a[1], a[2], a[3]),
            5 => (t!(usize, usize, usize, usize, usize))(a[0], a[1], a[2], a[3], a[4]),
            6 => (t!(usize, usize, usize, usize, usize, usize))(a[0], a[1], a[2], a[3], a[4], a[5]),
            7 => (t!(usize, usize, usize, usize, usize, usize, usize))(
                a[0], a[1], a[2], a[3], a[4], a[5], a[6],
            ),
            8 => (t!(usize, usize, usize, usize, usize, usize, usize, usize))(
                a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7],
            ),
            9 => (t!(usize, usize, usize, usize, usize, usize, usize, usize, usize))(
                a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8],
            ),
            10 => (t!(usize, usize, usize, usize, usize, usize, usize, usize, usize, usize))(
                a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8], a[9],
            ),
            _ => (t!(usize, usize, usize, usize, usize, usize, usize, usize, usize, usize, usize))(
                a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8], a[9], a[10],
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// process exit (bootstrap)
// ---------------------------------------------------------------------------
#[cfg(target_os = "linux")]
pub extern "sysv64" fn exit(code: usize) -> ! {
    raw_syscall(SYS_EXIT, code, 0, 0, 0, 0, 0);
    loop {}
}

// ===========================================================================
// WINDOWS x86_64 implementation — added in Phase 2 (the "+1 OS" step).
// This is the ENTIRE in-kernel cost of adding Windows. Note what is NOT here:
// no file abstraction, no portable I/O — only raw reachability (sym) and the
// platform's own memory + call mechanisms. Reachability, not semantics.
// ===========================================================================

// ③ On Windows there is no stable user-mode `syscall` numbering; the raw-syscall
// path is not used. Everything goes through symbol resolution + call.
#[cfg(windows)]
pub extern "sysv64" fn raw_syscall(
    _n: usize,
    _a: usize,
    _b: usize,
    _c: usize,
    _d: usize,
    _e: usize,
    _f: usize,
) -> isize {
    -1
}

// ③ symbol resolution — bootstrap via the two loader entrypoints (the OS's own
// symbol resolver), then reach anything else dynamically. This import table is
// the minimal bootstrap; it is linkage, not encapsulation.
#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryA(name: *const u8) -> *mut u8;
    fn GetProcAddress(module: *mut u8, name: *const u8) -> *mut u8;
}

#[cfg(windows)]
pub extern "sysv64" fn sym(module: *const u8, name: *const u8) -> *mut u8 {
    unsafe {
        let h = LoadLibraryA(module);
        GetProcAddress(h, name)
    }
}

// ④ call — data-driven native call, Windows (win64) convention.
// Arg ceiling raised 7 -> 11 in Phase 3 for CreateProcessA (10 args). See the
// Linux `call` note above for why this in-kernel growth is recorded, not hidden.
#[cfg(windows)]
pub extern "sysv64" fn call(addr: *mut u8, nargs: usize, args: *const usize) -> usize {
    unsafe {
        let a = core::slice::from_raw_parts(args, nargs);
        macro_rules! t {
            ($($p:ty),*) => { core::mem::transmute::<_, extern "win64" fn($($p),*) -> usize>(addr) };
        }
        match nargs {
            0 => (t!())(),
            1 => (t!(usize))(a[0]),
            2 => (t!(usize, usize))(a[0], a[1]),
            3 => (t!(usize, usize, usize))(a[0], a[1], a[2]),
            4 => (t!(usize, usize, usize, usize))(a[0], a[1], a[2], a[3]),
            5 => (t!(usize, usize, usize, usize, usize))(a[0], a[1], a[2], a[3], a[4]),
            6 => (t!(usize, usize, usize, usize, usize, usize))(a[0], a[1], a[2], a[3], a[4], a[5]),
            7 => (t!(usize, usize, usize, usize, usize, usize, usize))(
                a[0], a[1], a[2], a[3], a[4], a[5], a[6],
            ),
            8 => (t!(usize, usize, usize, usize, usize, usize, usize, usize))(
                a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7],
            ),
            9 => (t!(usize, usize, usize, usize, usize, usize, usize, usize, usize))(
                a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8],
            ),
            10 => (t!(usize, usize, usize, usize, usize, usize, usize, usize, usize, usize))(
                a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8], a[9],
            ),
            _ => (t!(usize, usize, usize, usize, usize, usize, usize, usize, usize, usize, usize))(
                a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8], a[9], a[10],
            ),
        }
    }
}

// ① memory — VirtualAlloc (reserve+commit RW) / VirtualProtect (RW <-> RX),
// reached through sym + call. No new primitive kind is introduced.
#[cfg(windows)]
pub extern "sysv64" fn mem_alloc(size: usize) -> *mut u8 {
    const MEM_COMMIT_RESERVE: usize = 0x3000;
    const PAGE_READWRITE: usize = 0x04;
    let f = sym(b"kernel32.dll\0".as_ptr(), b"VirtualAlloc\0".as_ptr());
    let args = [0usize, size, MEM_COMMIT_RESERVE, PAGE_READWRITE];
    call(f, 4, args.as_ptr()) as *mut u8
}

#[cfg(windows)]
pub extern "sysv64" fn mem_protect(ptr: *mut u8, size: usize, exec: bool) {
    const PAGE_EXECUTE_READ: usize = 0x20;
    const PAGE_READWRITE: usize = 0x04;
    let prot = if exec { PAGE_EXECUTE_READ } else { PAGE_READWRITE };
    let mut old: u32 = 0;
    let f = sym(b"kernel32.dll\0".as_ptr(), b"VirtualProtect\0".as_ptr());
    let args = [ptr as usize, size, prot, core::ptr::addr_of_mut!(old) as usize];
    call(f, 4, args.as_ptr());
}

// process exit (bootstrap)
#[cfg(windows)]
pub extern "sysv64" fn exit(code: usize) -> ! {
    let f = sym(b"kernel32.dll\0".as_ptr(), b"ExitProcess\0".as_ptr());
    let args = [code];
    call(f, 1, args.as_ptr());
    loop {}
}

/// Build the primitive table pointing at this host's native implementations.
pub fn native_table() -> Kernel {
    Kernel {
        mem_alloc,
        mem_protect,
        raw_syscall,
        sym,
        call,
        exit,
    }
}

// ---------------------------------------------------------------------------
// ② jump — the loader (variant B). Copy a raw code blob into fresh memory,
// flip it to RX, and jump in, handing over the primitive table.
// ---------------------------------------------------------------------------
pub fn load_and_run(blob: &[u8], k: &Kernel) -> ! {
    let n = blob.len();
    let mem = mem_alloc(n);
    unsafe {
        core::ptr::copy_nonoverlapping(blob.as_ptr(), mem, n);
    }
    mem_protect(mem, n, true);
    let entry: extern "sysv64" fn(*const Kernel) -> ! = unsafe { core::mem::transmute(mem) };
    entry(k as *const Kernel)
}

// ---------------------------------------------------------------------------
// process entry. All OS-specific bootstrap stays in the kernel (in-kernel cost).
//
//   variant A: static — build the table, call the statically linked payload
//              (crate root supplies `agent_main`).
//   variant B: loader — embed a payload blob, load it (①), jump into it (②).
// ---------------------------------------------------------------------------
#[cfg(dc_variant = "a")]
extern "sysv64" {
    fn agent_main(k: *const Kernel) -> !;
}

#[cfg(all(dc_variant = "a", target_os = "linux"))]
#[no_mangle]
pub extern "C" fn _start() -> ! {
    let k = native_table();
    unsafe { agent_main(&k as *const Kernel) }
}

#[cfg(all(dc_variant = "a", windows))]
#[no_mangle]
pub extern "C" fn mainCRTStartup() -> ! {
    let k = native_table();
    unsafe { agent_main(&k as *const Kernel) }
}

#[cfg(dc_variant = "b")]
static DC_BLOB: &[u8] = include_bytes!(env!("DC_BLOB"));

#[cfg(all(dc_variant = "b", target_os = "linux"))]
#[no_mangle]
pub extern "C" fn _start() -> ! {
    let k = native_table();
    load_and_run(DC_BLOB, &k)
}

#[cfg(all(dc_variant = "b", windows))]
#[no_mangle]
pub extern "C" fn mainCRTStartup() -> ! {
    let k = native_table();
    load_and_run(DC_BLOB, &k)
}
