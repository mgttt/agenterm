//! Windows spawn adapter — OUT of the kernel. Composes ③ sym + ④ call into
//! "start a child, wait, return its exit code". This is the SECOND capability
//! (criterion ④). It exposed a real cost: CreateProcessA takes 10 arguments, but
//! the kernel's ④ `call` shipped with a 7-arg ceiling — so adding this capability
//! FORCED an in-kernel change (call's arm table 7 -> 11). That is recorded in
//! RESULTS §④ as an honest finding, not hidden. Everything else is glue: no new
//! primitive KIND was added, only the existing ④ was completed toward its model.
//!
//! Writable structs (STARTUPINFOA, PROCESS_INFORMATION, the mutable command line
//! CreateProcessA may edit in place) cannot live in the flat RX blob's static
//! data (not writable in variant B), so they are requested from primitive ①.

use crate::abi::Kernel;

const STD_OUTPUT_HANDLE: usize = (-11i32) as u32 as usize;
const INFINITE: usize = 0xFFFF_FFFF;

#[inline]
fn k32(k: &Kernel, name: *const u8) -> *mut u8 {
    (k.sym)(b"kernel32.dll\0".as_ptr(), name)
}

/// Spawn `cmd.exe /c exit 7`, wait for it, and return its exit code.
pub fn spawn_wait(k: &Kernel) -> i32 {
    // STARTUPINFOA = 104 bytes on x64; PROCESS_INFORMATION = 24 bytes. Zero them,
    // set STARTUPINFOA.cb, and give CreateProcessA a writable command-line copy.
    let si = (k.mem_alloc)(104);
    let pi = (k.mem_alloc)(24);
    unsafe {
        core::ptr::write_bytes(si, 0, 104);
        core::ptr::write_bytes(pi, 0, 24);
        *(si as *mut u32) = 104; // cb
    }
    // CreateProcessA may modify lpCommandLine in place -> copy to a writable buffer.
    let cmd = b"cmd.exe /c exit 7\0";
    let cmdbuf = (k.mem_alloc)(cmd.len());
    unsafe {
        core::ptr::copy_nonoverlapping(cmd.as_ptr(), cmdbuf, cmd.len());
    }

    // CreateProcessA(NULL, cmd, NULL, NULL, FALSE, 0, NULL, NULL, &si, &pi) — 10 args.
    let create = k32(k, b"CreateProcessA\0".as_ptr());
    let cargs = [
        0usize,             // lpApplicationName
        cmdbuf as usize,    // lpCommandLine (writable)
        0,                  // lpProcessAttributes
        0,                  // lpThreadAttributes
        0,                  // bInheritHandles = FALSE
        0,                  // dwCreationFlags
        0,                  // lpEnvironment
        0,                  // lpCurrentDirectory
        si as usize,        // lpStartupInfo
        pi as usize,        // lpProcessInformation
    ];
    let ok = (k.call)(create, 10, cargs.as_ptr());
    if ok == 0 {
        return -1;
    }
    // PROCESS_INFORMATION.hProcess is the first field (offset 0); hThread at 8.
    let hproc = unsafe { *(pi as *const usize) };
    let hthread = unsafe { *((pi as *const usize).add(1)) };

    let wait = k32(k, b"WaitForSingleObject\0".as_ptr());
    (k.call)(wait, 2, [hproc, INFINITE].as_ptr());

    let mut code: u32 = 0;
    let getexit = k32(k, b"GetExitCodeProcess\0".as_ptr());
    (k.call)(
        getexit,
        2,
        [hproc, core::ptr::addr_of_mut!(code) as usize].as_ptr(),
    );

    let close = k32(k, b"CloseHandle\0".as_ptr());
    (k.call)(close, 1, [hproc].as_ptr());
    (k.call)(close, 1, [hthread].as_ptr());

    code as i32
}

/// Write `len` bytes to stdout (so the payload can print the child's exit code).
pub fn write_stdout(k: &Kernel, buf: *const u8, len: usize) {
    let gsh = k32(k, b"GetStdHandle\0".as_ptr());
    let h = (k.call)(gsh, 1, [STD_OUTPUT_HANDLE].as_ptr());
    let mut wrote: u32 = 0;
    let write = k32(k, b"WriteFile\0".as_ptr());
    let wargs = [h, buf as usize, len, core::ptr::addr_of_mut!(wrote) as usize, 0];
    (k.call)(write, 5, wargs.as_ptr());
}
