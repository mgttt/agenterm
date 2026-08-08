//! The four kernel primitives (Linux path), written to compile for BOTH x86-64 and
//! aarch64, to measure what a SECOND ISA costs the KERNEL itself (Q0 measured the
//! x86-64 Linux kernel at ~2.7 KB; this isolates the four-primitive core).
//!
//! The point: almost the whole kernel is PORTABLE Rust that rustc re-targets for
//! free. The ONLY hand-written ISA-specific surface is `raw_syscall` (the syscall
//! instruction + its register binding) and the `call` ABI string. mem_alloc /
//! mem_protect / exit / the loader are identical source across ISAs.
//!
//! Compile per ISA:
//!   rustc --edition 2021 -O -C panic=abort --crate-type staticlib --emit obj \
//!         --target <x86_64|aarch64>-unknown-linux-gnu kernel/prim.rs -o out/prim.<isa>.o

#![no_std]
#![allow(dead_code)]

#[panic_handler]
fn ph(_: &core::panic::PanicInfo) -> ! { loop {} }

// ---------------------------------------------------------------------------
// ③ reachability — raw syscall. THE ONLY hand-written per-ISA block.
// ---------------------------------------------------------------------------
#[cfg(target_arch = "x86_64")]
#[no_mangle]
pub extern "C" fn raw_syscall(n: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> isize {
    let ret: isize;
    unsafe {
        core::arch::asm!(
            "syscall",
            inout("rax") n => ret,
            in("rdi") a, in("rsi") b, in("rdx") c,
            in("r10") d, in("r8") e, in("r9") f,
            out("rcx") _, out("r11") _,
            options(nostack),
        );
    }
    ret
}

#[cfg(target_arch = "aarch64")]
#[no_mangle]
pub extern "C" fn raw_syscall(n: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> isize {
    let ret: isize;
    unsafe {
        core::arch::asm!(
            "svc #0",
            inout("x8") n => _,
            inout("x0") a => ret,
            in("x1") b, in("x2") c, in("x3") d, in("x4") e, in("x5") f,
            options(nostack),
        );
    }
    ret
}

// ---------------------------------------------------------------------------
// Per-ISA/OS syscall numbers (data, not code — they differ per (ISA,OS)).
// ---------------------------------------------------------------------------
#[cfg(target_arch = "x86_64")]
mod nr { pub const MMAP: usize = 9; pub const MPROTECT: usize = 10; pub const EXIT: usize = 60; }
#[cfg(target_arch = "aarch64")]
mod nr { pub const MMAP: usize = 222; pub const MPROTECT: usize = 226; pub const EXIT: usize = 93; }

// ---------------------------------------------------------------------------
// ① memory — PORTABLE Rust (identical source across ISAs).
// ---------------------------------------------------------------------------
#[no_mangle]
pub extern "C" fn mem_alloc(size: usize) -> *mut u8 {
    const PROT_RW: usize = 0x3;
    const MAP_PRIVATE_ANON: usize = 0x22;
    raw_syscall(nr::MMAP, 0, size, PROT_RW, MAP_PRIVATE_ANON, usize::MAX, 0) as *mut u8
}

#[no_mangle]
pub extern "C" fn mem_protect(ptr: *mut u8, size: usize, exec: bool) {
    let prot = if exec { 0x5usize } else { 0x3usize };
    raw_syscall(nr::MPROTECT, ptr as usize, size, prot, 0, 0, 0);
}

// ---------------------------------------------------------------------------
// ④ call — data-driven native call. PORTABLE: rustc emits the correct arg
// placement per target from `extern "C"` (AAPCS64 on aarch64, SysV on x86-64).
// The only per-ISA text is which registers rustc chooses — generated, not written.
// ---------------------------------------------------------------------------
#[no_mangle]
pub extern "C" fn call(addr: *mut u8, nargs: usize, args: *const usize) -> usize {
    unsafe {
        let a = core::slice::from_raw_parts(args, nargs.max(1));
        macro_rules! t { ($($p:ty),*) => { core::mem::transmute::<_, extern "C" fn($($p),*) -> usize>(addr) }; }
        match nargs {
            0 => (t!())(),
            1 => (t!(usize))(a[0]),
            2 => (t!(usize, usize))(a[0], a[1]),
            3 => (t!(usize, usize, usize))(a[0], a[1], a[2]),
            4 => (t!(usize, usize, usize, usize))(a[0], a[1], a[2], a[3]),
            5 => (t!(usize, usize, usize, usize, usize))(a[0], a[1], a[2], a[3], a[4]),
            6 => (t!(usize, usize, usize, usize, usize, usize))(a[0], a[1], a[2], a[3], a[4], a[5]),
            7 => (t!(usize, usize, usize, usize, usize, usize, usize))(a[0], a[1], a[2], a[3], a[4], a[5], a[6]),
            8 => (t!(usize, usize, usize, usize, usize, usize, usize, usize))(a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7]),
            9 => (t!(usize, usize, usize, usize, usize, usize, usize, usize, usize))(a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8]),
            10 => (t!(usize, usize, usize, usize, usize, usize, usize, usize, usize, usize))(a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8], a[9]),
            _ => (t!(usize, usize, usize, usize, usize, usize, usize, usize, usize, usize, usize))(a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8], a[9], a[10]),
        }
    }
}

// process exit — PORTABLE.
#[no_mangle]
pub extern "C" fn exit(code: usize) -> ! {
    raw_syscall(nr::EXIT, code, 0, 0, 0, 0, 0);
    loop {}
}

// ② jump — the loader (variant B). PORTABLE.
#[no_mangle]
pub extern "C" fn load_and_run(blob: *const u8, len: usize, ctx: *const usize) -> ! {
    let mem = mem_alloc(len);
    unsafe { core::ptr::copy_nonoverlapping(blob, mem, len); }
    mem_protect(mem, len, true);
    let entry: extern "C" fn(*const usize) -> ! = unsafe { core::mem::transmute(mem) };
    entry(ctx)
}
