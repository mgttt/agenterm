//! Byte-measurement baseline: the FOUR primitives as a no_std blob.
//! Compile: rustc --edition 2021 -O --crate-type=lib --emit=obj -A warnings
//!          --target x86_64-pc-windows-msvc kernel4.rs -o out/kernel4.o
//! Then measure the .text of the primitive symbols (RESULTS ③).
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

// ① memory
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

// ③ reach: raw_syscall (unused on Windows) + sym
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

// ④ call — 0..=11 args
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
