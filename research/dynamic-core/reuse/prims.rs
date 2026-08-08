//! Reuse experiment — the four primitives (same primitive set as Q0's core/kernel.rs).
//! Included ONLY by the loader binary (blobs receive the table, they don't define it).
//! Re-expressed locally so `reuse/` builds standalone; the primitive *kinds* and ABI
//! are identical to Q0, keeping byte counts comparable (Q0 口径).

use crate::abi::Kernel;

// Freestanding memory intrinsics (no libc). Codegen emits calls to these for bulk copies.
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
// LINUX x86_64
// ===========================================================================
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
            in("rdi") a, in("rsi") b, in("rdx") c,
            in("r10") d, in("r8") e, in("r9") f,
            out("rcx") _, out("r11") _,
            options(nostack),
        );
    }
    ret
}
#[cfg(target_os = "linux")]
const SYS_MMAP: usize = 9;
#[cfg(target_os = "linux")]
const SYS_MPROTECT: usize = 10;
#[cfg(target_os = "linux")]
const SYS_EXIT: usize = 60;

#[cfg(target_os = "linux")]
pub extern "sysv64" fn mem_alloc(size: usize) -> *mut u8 {
    const PROT_RW: usize = 0x3;
    const MAP_PRIVATE_ANON: usize = 0x22;
    let p = raw_syscall(SYS_MMAP, 0, size, PROT_RW, MAP_PRIVATE_ANON, usize::MAX, 0);
    p as *mut u8
}
#[cfg(target_os = "linux")]
pub extern "sysv64" fn mem_protect(ptr: *mut u8, size: usize, exec: bool) {
    const PROT_RX: usize = 0x5;
    const PROT_RW: usize = 0x3;
    let prot = if exec { PROT_RX } else { PROT_RW };
    raw_syscall(SYS_MPROTECT, ptr as usize, size, prot, 0, 0, 0);
}
#[cfg(target_os = "linux")]
pub extern "sysv64" fn sym(_module: *const u8, _name: *const u8) -> *mut u8 {
    core::ptr::null_mut()
}
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
            _ => (t!(usize, usize, usize, usize, usize, usize))(a[0], a[1], a[2], a[3], a[4], a[5]),
        }
    }
}
#[cfg(target_os = "linux")]
pub extern "sysv64" fn exit(code: usize) -> ! {
    raw_syscall(SYS_EXIT, code, 0, 0, 0, 0, 0);
    loop {}
}

// ===========================================================================
// WINDOWS x86_64
// ===========================================================================
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
            _ => (t!(usize, usize, usize, usize, usize, usize))(a[0], a[1], a[2], a[3], a[4], a[5]),
        }
    }
}
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
#[cfg(windows)]
pub extern "sysv64" fn exit(code: usize) -> ! {
    let f = sym(b"kernel32.dll\0".as_ptr(), b"ExitProcess\0".as_ptr());
    call(f, 1, [code].as_ptr());
    loop {}
}

/// Build the primitive table for this host.
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
