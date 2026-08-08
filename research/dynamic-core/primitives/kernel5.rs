//! Byte-measurement variant: the four primitives + a 5th `declare`.
//! Identical to kernel4.rs except for the added `declare` primitive, so the
//! .text delta isolates the in-kernel cost of Declare (RESULTS ③).
//!
//! `declare` is bidirectional metadata:
//!   query = QueryKind (Describe-machine: page size via GetSystemInfo;
//!           Query-layout: a BAKED offset table — no BTF on Windows).
//!   publish = a stub RtlAddFunctionTable-style call (Publish half).
#![no_std]
#![allow(warnings)]

#[panic_handler]
fn ph(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

extern "system" {
    fn LoadLibraryA(name: *const u8) -> *mut u8;
    fn GetProcAddress(module: *mut u8, name: *const u8) -> *mut u8;
}

#[no_mangle]
pub extern "win64" fn mem_alloc(size: usize) -> *mut u8 {
    let f = sym(b"kernel32.dll\0".as_ptr(), b"VirtualAlloc\0".as_ptr());
    let args = [0usize, size, 0x3000, 0x04];
    call(f, 4, args.as_ptr()) as *mut u8
}
#[no_mangle]
pub extern "win64" fn mem_protect(ptr: *mut u8, size: usize, exec: bool) {
    let prot = if exec { 0x20usize } else { 0x04 };
    let mut old: u32 = 0;
    let f = sym(b"kernel32.dll\0".as_ptr(), b"VirtualProtect\0".as_ptr());
    let args = [ptr as usize, size, prot, core::ptr::addr_of_mut!(old) as usize];
    call(f, 4, args.as_ptr());
}
#[no_mangle]
pub extern "win64" fn raw_syscall(_n: usize, _a: usize, _b: usize, _c: usize, _d: usize, _e: usize, _f: usize) -> isize {
    -1
}
#[no_mangle]
pub extern "win64" fn sym(module: *const u8, name: *const u8) -> *mut u8 {
    unsafe {
        let h = LoadLibraryA(module);
        GetProcAddress(h, name)
    }
}
#[no_mangle]
pub extern "win64" fn call(addr: *mut u8, nargs: usize, args: *const usize) -> usize {
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
            7 => (t!(usize, usize, usize, usize, usize, usize, usize))(a[0], a[1], a[2], a[3], a[4], a[5], a[6]),
            8 => (t!(usize, usize, usize, usize, usize, usize, usize, usize))(a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7]),
            9 => (t!(usize, usize, usize, usize, usize, usize, usize, usize, usize))(a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8]),
            10 => (t!(usize, usize, usize, usize, usize, usize, usize, usize, usize, usize))(a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8], a[9]),
            _ => (t!(usize, usize, usize, usize, usize, usize, usize, usize, usize, usize, usize))(a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8], a[9], a[10]),
        }
    }
}

// ---------------------------------------------------------------------------
// ⑤ declare — the candidate fifth primitive.
//   what: 0 = describe page size (host-answerable via GetSystemInfo)
//         1 = query struct-field offset from the BAKED table (no BTF on Win)
//         2 = publish generated-code metadata (RtlAddFunctionTable stub)
// The offset table is the honest cost: it is TRUST baked into the kernel,
// because the Windows host publishes no machine-readable struct layout.
// ---------------------------------------------------------------------------
static LAYOUT_TABLE: [(u32, u32); 4] = [
    (0, 44), // WIN32_FIND_DATAA.cFileName
    (1, 0),  // sockaddr_in.sin_family
    (2, 2),  // sockaddr_in.sin_port
    (3, 4),  // sockaddr_in.sin_addr
];

#[no_mangle]
pub extern "win64" fn declare(what: usize, key: usize, arg: usize) -> usize {
    match what {
        0 => {
            // Describe-machine: ask the host.
            let mut si = [0u8; 48];
            let f = sym(b"kernel32.dll\0".as_ptr(), b"GetSystemInfo\0".as_ptr());
            let args = [si.as_mut_ptr() as usize];
            call(f, 1, args.as_ptr());
            unsafe { *(si.as_ptr().add(4) as *const u32) as usize }
        }
        1 => {
            // Query-layout: baked table (no host oracle on Windows).
            let mut i = 0;
            while i < LAYOUT_TABLE.len() {
                if LAYOUT_TABLE[i].0 as usize == key {
                    return LAYOUT_TABLE[i].1 as usize;
                }
                i += 1;
            }
            usize::MAX
        }
        _ => {
            // Publish: register generated code's unwind info with the host.
            let f = sym(b"kernel32.dll\0".as_ptr(), b"RtlAddFunctionTable\0".as_ptr());
            if f.is_null() {
                return 0;
            }
            let args = [key, 1usize, arg];
            call(f, 3, args.as_ptr())
        }
    }
}
