//! "烤了就验" (bake-and-detect) layout facts — ported from the PATTERN
//! `research/dynamic-core/declare/RESULTS.md` (Q13) established, not from any
//! literal source file (Q13's own code was a clean-room driver for its own
//! three facts; `STARTUPINFOA`/`PROCESS_INFORMATION` weren't among them —
//! Q22 baked those two structs' offsets directly into `seam.rs`'s constants
//! with no self-check at all, which is exactly the "裸烤" this crate's design
//! doc calls out as the thing to fix from day one).
//!
//! Q13's verdict: on Windows there is no `offsetof` oracle, so baking a
//! struct-layout constant is unavoidable — but a WRONG bake is detectable
//! whenever the field participates in some Win32 API's semantic round trip
//! (constructive: you choose an input, you know the exact output; or
//! OS-round-trip: the kernel validates/writes the value back). This module
//! applies that pattern to the two structs `seam.rs`'s `SpawnWait`/
//! `FileWrite` mechanism depends on:
//!
//!   1. `STARTUPINFOA` — constructive round trip via `dwFlags`/`hStdOutput`/
//!      `hStdError` (`STARTF_USESTDHANDLES`): redirect a real child's stdout
//!      through a real pipe, and require the exact marker string we chose
//!      comes back. A wrong offset for `dwFlags` or `hStdOutput` either
//!      breaks `CreateProcessA` outright or silently fails to redirect (the
//!      child's output goes to the real console instead of our pipe, so the
//!      pipe read returns nothing) — both are observable failures, not a
//!      silent wrong answer.
//!   2. `PROCESS_INFORMATION.hProcess` — OS-round-trip: extract the handle
//!      at the assumed offset and hand it back to the OS
//!      (`WaitForSingleObject`/`GetExitCodeProcess`); the kernel validates
//!      handle values against the process's handle table, so a wrong offset
//!      yields a rejected handle (`WAIT_FAILED`/`GetExitCodeProcess` failure)
//!      — safe (no AV; Win32 handles are checked, not raw pointers) and loud.
//!
//! `seam.rs` imports the offset/size constants from THIS module (not its own
//! copies) so the offsets actually used in the real `SpawnWait`/`FileWrite`
//! seam bindings are the same ones this file's self-checks exercise — no
//! drift between "what we tested" and "what we ship" is possible by
//! construction.

#![allow(dead_code)]

// ============================================================================
// STARTUPINFOA — x86_64 layout (baked; see the self-check below for why this
// is detectably correct rather than merely asserted).
// ============================================================================

pub const STARTUPINFOA_SIZE: usize = 104;

pub mod startupinfoa_offset {
    pub const CB: usize = 0;
    // lpReserved@8, lpDesktop@16, lpTitle@24, dwX@32, dwY@36, dwXSize@40,
    // dwYSize@44, dwXCountChars@48, dwYCountChars@52, dwFillAttribute@56 —
    // unused by this crate's seam bindings, left zeroed, not individually
    // self-checked (Q13 §④'s named residue: fields with no round trip this
    // crate exercises).
    pub const DW_FLAGS: usize = 60;
    pub const W_SHOW_WINDOW: usize = 64;
    // cbReserved2@66 (2), padding@68..72, lpReserved2@72 — unused, zeroed.
    pub const H_STD_INPUT: usize = 80;
    pub const H_STD_OUTPUT: usize = 88;
    pub const H_STD_ERROR: usize = 96;
}

pub const STARTF_USESTDHANDLES: u32 = 0x0000_0100;

/// Build a zeroed `STARTUPINFOA` byte buffer with `cb` baked at its declared
/// offset. The seam's `SpawnWait` binding (no stdio redirection) and this
/// module's self-check (with redirection layered on top) both start from
/// this same builder — one baked layout fact, one place it is written.
pub fn build_startupinfoa_bytes() -> [u8; STARTUPINFOA_SIZE] {
    let mut b = [0u8; STARTUPINFOA_SIZE];
    b[startupinfoa_offset::CB..startupinfoa_offset::CB + 4]
        .copy_from_slice(&(STARTUPINFOA_SIZE as u32).to_le_bytes());
    b
}

// ============================================================================
// PROCESS_INFORMATION — x86_64 layout.
// ============================================================================

pub const PROCESS_INFORMATION_SIZE: usize = 24;

pub mod process_information_offset {
    pub const H_PROCESS: usize = 0;
    pub const H_THREAD: usize = 8;
    pub const DW_PROCESS_ID: usize = 16;
    pub const DW_THREAD_ID: usize = 20;
}

pub fn zeroed_process_information_bytes() -> [u8; PROCESS_INFORMATION_SIZE] {
    [0u8; PROCESS_INFORMATION_SIZE]
}

// ============================================================================
// Self-checks — real Win32 calls, cfg(windows) only (this crate targets
// x86_64/Windows exclusively per the design doc §3, same as the rest of the
// track). Each returns Ok on a round trip that corroborates the offsets
// passed in, or Err with a reason on any failure — a caller drives both a
// correct-offset call (must be Ok) and a corrupted-offset call (must be Err)
// to get Q13's PASS/FIRE pair; see tests/declare_selfcheck.rs.
// ============================================================================

#[cfg(windows)]
mod ffi {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn CreatePipe(read_h: *mut *mut u8, write_h: *mut *mut u8, sa: *mut u8, size: u32) -> i32;
        pub fn SetHandleInformation(h: *mut u8, mask: u32, flags: u32) -> i32;
        pub fn CreateProcessA(
            app: *const u8,
            cmd: *mut u8,
            pa: *mut u8,
            ta: *mut u8,
            inherit: i32,
            flags: u32,
            env: *mut u8,
            dir: *const u8,
            si: *mut u8,
            pi: *mut u8,
        ) -> i32;
        pub fn WaitForSingleObject(h: *mut u8, ms: u32) -> u32;
        pub fn GetExitCodeProcess(h: *mut u8, code: *mut u32) -> i32;
        pub fn ReadFile(h: *mut u8, buf: *mut u8, n: u32, got: *mut u32, ov: *mut u8) -> i32;
        pub fn CloseHandle(h: *mut u8) -> i32;
    }
}

const HANDLE_FLAG_INHERIT: u32 = 0x0000_0001;
const WAIT_OBJECT_0: u32 = 0;
const WAIT_TIMEOUT_MS: u32 = 5_000;

/// FACT 1 (constructive round trip): redirect a real child's stdout/stderr
/// through a real pipe using `STARTF_USESTDHANDLES` at `dw_flags_off`, with
/// the write end planted at `h_stdout_off`/`h_stderr_off`. Spawns
/// `cmd.exe /c echo <marker>` and requires the EXACT marker text comes back
/// through the pipe. Pass the real declared offsets to prove the layout;
/// pass deliberately wrong ones to prove detection (see the test module).
#[cfg(windows)]
pub fn selfcheck_startupinfoa_stdout_roundtrip(
    dw_flags_off: usize,
    h_stdout_off: usize,
    h_stderr_off: usize,
) -> Result<String, String> {
    use ffi::*;
    const MARKER: &str = "Q13_NATIVECORE_SELFCHECK_MARKER";
    unsafe {
        // SECURITY_ATTRIBUTES: nLength@0(4), lpSecurityDescriptor@8(8), bInheritHandle@16(4)
        let mut sa = [0u8; 24];
        sa[0..4].copy_from_slice(&24u32.to_le_bytes());
        sa[16..20].copy_from_slice(&1u32.to_le_bytes()); // bInheritHandle = TRUE

        let mut read_h: *mut u8 = std::ptr::null_mut();
        let mut write_h: *mut u8 = std::ptr::null_mut();
        if CreatePipe(&mut read_h, &mut write_h, sa.as_mut_ptr(), 0) == 0 {
            return Err("CreatePipe failed".to_string());
        }
        // our end of the pipe must not be inherited by the child, or ReadFile
        // never sees EOF (the child would hold its own copy of the read end open)
        SetHandleInformation(read_h, HANDLE_FLAG_INHERIT, 0);

        let mut startup = build_startupinfoa_bytes();
        let write_bytes = |buf: &mut [u8], off: usize, v: &[u8]| -> Result<(), String> {
            let len = buf.len();
            buf.get_mut(off..off + v.len())
                .ok_or_else(|| format!("offset {off} out of range for a {len}-byte struct"))?
                .copy_from_slice(v);
            Ok(())
        };
        write_bytes(&mut startup, dw_flags_off, &STARTF_USESTDHANDLES.to_le_bytes())?;
        write_bytes(&mut startup, h_stdout_off, &(write_h as u64).to_le_bytes())?;
        write_bytes(&mut startup, h_stderr_off, &(write_h as u64).to_le_bytes())?;

        let mut pi = zeroed_process_information_bytes();
        let mut cmdline = format!("cmd.exe /c echo {MARKER}\0").into_bytes();
        let ok = CreateProcessA(
            std::ptr::null(),
            cmdline.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            1, // bInheritHandles = TRUE: the child must inherit write_h
            0,
            std::ptr::null_mut(),
            std::ptr::null(),
            startup.as_mut_ptr(),
            pi.as_mut_ptr(),
        );
        // our copy of the write end must close regardless, or ReadFile below
        // blocks waiting for a EOF that our own open handle prevents
        CloseHandle(write_h);
        if ok == 0 {
            CloseHandle(read_h);
            return Err("CreateProcessA failed".to_string());
        }

        let h_process = u64::from_le_bytes(
            pi[process_information_offset::H_PROCESS..process_information_offset::H_PROCESS + 8]
                .try_into()
                .unwrap(),
        ) as *mut u8;
        let h_thread = u64::from_le_bytes(
            pi[process_information_offset::H_THREAD..process_information_offset::H_THREAD + 8]
                .try_into()
                .unwrap(),
        ) as *mut u8;
        WaitForSingleObject(h_process, WAIT_TIMEOUT_MS);

        let mut collected = Vec::new();
        let mut buf = [0u8; 256];
        loop {
            let mut got: u32 = 0;
            let r = ReadFile(read_h, buf.as_mut_ptr(), buf.len() as u32, &mut got, std::ptr::null_mut());
            if r == 0 || got == 0 {
                break;
            }
            collected.extend_from_slice(&buf[..got as usize]);
        }
        CloseHandle(read_h);
        CloseHandle(h_process);
        CloseHandle(h_thread);

        let text = String::from_utf8_lossy(&collected).to_string();
        if text.contains(MARKER) {
            Ok(text)
        } else {
            Err(format!("marker {MARKER:?} not found in redirected output: {text:?}"))
        }
    }
}

/// FACT 2 (OS round trip): spawn `cmd.exe /c exit 42`, extract `hProcess` at
/// `h_process_off`, and hand it straight back to the OS
/// (`WaitForSingleObject` + `GetExitCodeProcess`). Correct offset ⇒
/// `Ok(42)`; a wrong offset almost always yields an invalid-handle rejection
/// from the kernel (`Err`) — handle values are OS-validated, so this is safe
/// even when deliberately corrupted (no wild-pointer dereference).
#[cfg(windows)]
pub fn selfcheck_process_information_hprocess(h_process_off: usize) -> Result<u32, String> {
    use ffi::*;
    unsafe {
        let mut startup = build_startupinfoa_bytes();
        let mut pi = zeroed_process_information_bytes();
        let mut cmdline = b"cmd.exe /c exit 42\0".to_vec();
        let ok = CreateProcessA(
            std::ptr::null(),
            cmdline.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null(),
            startup.as_mut_ptr(),
            pi.as_mut_ptr(),
        );
        if ok == 0 {
            return Err("CreateProcessA failed".to_string());
        }
        // always close the REAL handles at the end (offsets 0/8), regardless
        // of which offset the caller is testing, so this self-check never
        // leaks a kernel handle even when deliberately probing a wrong one
        let real_h_process = u64::from_le_bytes(
            pi[process_information_offset::H_PROCESS..process_information_offset::H_PROCESS + 8]
                .try_into()
                .unwrap(),
        ) as *mut u8;
        let real_h_thread = u64::from_le_bytes(
            pi[process_information_offset::H_THREAD..process_information_offset::H_THREAD + 8]
                .try_into()
                .unwrap(),
        ) as *mut u8;

        let result = (|| -> Result<u32, String> {
            let candidate = pi
                .get(h_process_off..h_process_off + 8)
                .ok_or_else(|| format!("offset {h_process_off} out of range for a {PROCESS_INFORMATION_SIZE}-byte struct"))?;
            let h_process = u64::from_le_bytes(candidate.try_into().unwrap()) as *mut u8;
            let wait = WaitForSingleObject(h_process, WAIT_TIMEOUT_MS);
            if wait != WAIT_OBJECT_0 {
                return Err(format!("WaitForSingleObject returned {wait:#x} (expected WAIT_OBJECT_0)"));
            }
            let mut exit_code: u32 = 0;
            if GetExitCodeProcess(h_process, &mut exit_code) == 0 {
                return Err("GetExitCodeProcess failed (rejected handle)".to_string());
            }
            Ok(exit_code)
        })();

        // best-effort: if the tested offset WAS the real one, WaitForSingleObject
        // above already reaped it; closing the real handles here is still safe
        // (CloseHandle on an already-signaled-but-open handle is normal cleanup)
        CloseHandle(real_h_process);
        CloseHandle(real_h_thread);
        result
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn startupinfoa_roundtrip_passes_on_the_real_declared_offsets() {
        let result = selfcheck_startupinfoa_stdout_roundtrip(
            startupinfoa_offset::DW_FLAGS,
            startupinfoa_offset::H_STD_OUTPUT,
            startupinfoa_offset::H_STD_ERROR,
        );
        assert!(result.is_ok(), "PASS expected on correct offsets, got {result:?}");
    }

    #[test]
    fn startupinfoa_roundtrip_fires_on_a_corrupted_dw_flags_offset() {
        // dwFillAttribute@56 instead of the real dwFlags@60 -- STARTF_USESTDHANDLES
        // never actually lands in dwFlags, so CreateProcessA does not redirect
        // and the pipe read comes back empty (or CreateProcessA itself rejects
        // the garbage bit pattern it now reads as dwFillAttribute).
        let result = selfcheck_startupinfoa_stdout_roundtrip(56, startupinfoa_offset::H_STD_OUTPUT, startupinfoa_offset::H_STD_ERROR);
        assert!(result.is_err(), "FIRE expected on a corrupted dwFlags offset, got {result:?}");
    }

    #[test]
    fn startupinfoa_roundtrip_fires_on_a_corrupted_stdout_offset() {
        // lpReserved2@72 instead of the real hStdOutput@88 -- CreateProcessA
        // either rejects the resulting garbage handle outright or the child's
        // real stdout goes to the console instead of our pipe.
        let result = selfcheck_startupinfoa_stdout_roundtrip(startupinfoa_offset::DW_FLAGS, 72, startupinfoa_offset::H_STD_ERROR);
        assert!(result.is_err(), "FIRE expected on a corrupted hStdOutput offset, got {result:?}");
    }

    #[test]
    fn process_information_hprocess_roundtrip_passes_on_the_real_offset() {
        let result = selfcheck_process_information_hprocess(process_information_offset::H_PROCESS);
        assert_eq!(result, Ok(42), "PASS expected on the correct hProcess offset");
    }

    #[test]
    fn process_information_hprocess_roundtrip_fires_on_a_corrupted_offset() {
        // offset 4 straddles the upper 4 bytes of hProcess and the lower 4
        // bytes of hThread -- a garbage 64-bit value the kernel's handle
        // table almost certainly rejects.
        let result = selfcheck_process_information_hprocess(4);
        assert!(result.is_err(), "FIRE expected on a corrupted hProcess offset, got {result:?}");
    }
}
