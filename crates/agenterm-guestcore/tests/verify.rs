//! Real, executed verification. Two kinds of test here:
//!
//! * The three programs the task requires (loop/cond, file read round trip,
//!   mmap write) are spawned as REAL child processes (`examples/verify_*.rs`,
//!   each built with `ExitMode::Real` -- a real `exit`/`exit_group` syscall
//!   really calls `ExitProcess`, which would kill the `cargo test` harness if
//!   run in-process; spawning a subprocess is the honest way to exercise
//!   that path and still assert on the result from outside).
//! * The RIP-relative-`lea` regression test, and a handful of fault-path
//!   sanity checks, run in-process via `ExitMode::Return` (fast, no
//!   subprocess, and they don't reach `exit` at all in the regression case).

use std::io::Write;
use std::process::Command;

use agenterm_guestcore::cpu::{Cpu, RAX, RDI};
use agenterm_guestcore::decode_x86_64;
use agenterm_guestcore::elf;
use agenterm_guestcore::fault::GuestFault;
use agenterm_guestcore::intent_map::ExitMode;
use agenterm_guestcore::testutil::Emitter;
use agenterm_guestcore::GuestImage;

fn run_example(name: &str, extra_args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(env!("CARGO"));
    cmd.args(["run", "--quiet", "-p", "agenterm-guestcore", "--example", name]);
    if !extra_args.is_empty() {
        cmd.arg("--");
        cmd.args(extra_args);
    }
    cmd.output().unwrap_or_else(|e| panic!("failed to spawn `cargo run --example {name}`: {e}"))
}

#[test]
fn verify_a_loop_and_conditional_branch() {
    let out = run_example("verify_loop_cond", &[]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    eprintln!("[verify_a] real stdout: {stdout:?}");
    eprintln!("[verify_a] real exit status: {:?}", out.status);
    assert_eq!(stdout, "AAA\n", "guest loop should have written 'A' three times then a newline via three real WriteFile calls");
    assert_eq!(out.status.code(), Some(0), "guest should have called exit(0), routed to a real ExitProcess(0)");
}

#[test]
fn verify_b_real_file_read_round_trip() {
    let mut tmp = tempfile::NamedTempFile::new().expect("create real temp file");
    let content = b"hello from real disk file\n";
    tmp.write_all(content).expect("write real temp file content");
    tmp.flush().unwrap();
    // `CreateFileA` (behind `Intent::FileOpen`) opens with `FILE_SHARE_READ`
    // only -- Windows file sharing is mandatory, so the still-open `File`
    // handle `NamedTempFile` holds would cause a real sharing violation
    // (silently read-as-zero-bytes, not a crash) if left open across the
    // guest's own real `CreateFileA`. `into_temp_path()` closes the handle
    // but keeps delete-on-drop, so the real file on disk is untouched.
    let tmp = tmp.into_temp_path();
    let path = tmp.to_str().expect("temp path is valid utf8").to_string();

    let out = run_example("verify_file_roundtrip", &[&path]);
    let stdout = out.stdout.clone();
    eprintln!("[verify_b] real stdout: {:?}", String::from_utf8_lossy(&stdout));
    eprintln!("[verify_b] real exit status: {:?}", out.status);
    assert_eq!(stdout, content, "guest should have echoed back exactly the real bytes read off disk via openat->FileOpen, read->FileRead, close->FileClose");
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn verify_c_mmap_alloc_is_real_writable_memory() {
    let out = run_example("verify_mmap_write", &[]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    eprintln!("[verify_c] real stdout: {stdout:?}");
    eprintln!("[verify_c] real exit status: {:?}", out.status);
    assert_eq!(stdout, "MMAP-OK\n", "bytes written into the mmap'd (Intent::Alloc / VirtualAlloc) region should read back exactly via a real, separate write() syscall");
    assert_eq!(out.status.code(), Some(0));
}

/// A hand-rolled base64 decoder (no crate dependency added just for this) --
/// decodes `tests/fixtures/tiny_musl_elf.b64`, this round's REAL
/// compiled-binary verification fixture (see below).
fn base64_decode(s: &str) -> Vec<u8> {
    fn val(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let clean: Vec<u8> = s.bytes().filter(|&c| c != b'=' && val(c).is_some()).collect();
    let mut out = Vec::with_capacity(clean.len() * 3 / 4);
    for chunk in clean.chunks(4) {
        let v: Vec<u8> = chunk.iter().map(|&c| val(c).unwrap()).collect();
        out.push((v[0] << 2) | (v.get(1).copied().unwrap_or(0) >> 4));
        if v.len() > 2 {
            out.push((v[1] << 4) | (v[2] >> 2));
        }
        if v.len() > 3 {
            out.push((v[2] << 6) | v[3]);
        }
    }
    out
}

/// The REAL bar this round's task set, stronger than programs (a)/(b)/(c)
/// above (which all feed the interpreter hand-encoded machine code): a
/// genuinely compiled x86_64 Linux ELF binary, built with this box's real
/// `x86_64-unknown-linux-musl` Rust cross-compilation toolchain (source and
/// build command documented in the crate README's "ELF loading" section),
/// committed here as a base64 fixture (same precedent as `ape-vm`'s own
/// `nano_bubblesort.elf.b64`) so this verification is reproducible without
/// needing that toolchain installed on every future run.
#[test]
fn verify_d_real_compiled_elf_binary_parses_and_runs() {
    let b64 = include_str!("fixtures/tiny_musl_elf.b64");
    let bytes = base64_decode(b64);

    // First: this loader's OWN parser, introspecting the real compiled
    // output -- proves (not just claims) it is a static ET_EXEC with no
    // PT_INTERP (no dynamic linker needed), matching what `file` reports
    // for the same binary (see the README).
    let parsed = elf::parse_elf64(&bytes).expect("real compiled ELF should parse");
    assert_eq!(parsed.e_type, elf::ET_EXEC, "must be a static ET_EXEC (non-PIE) binary");
    assert!(!parsed.has_interp, "must have no PT_INTERP -- no dynamic linker needed");
    assert!(!parsed.segments.is_empty());

    // Then: write it to a real temp file and run it through the real ELF
    // loader + unchanged interpreter pipeline, exactly the way a real ELF
    // binary would be handed to this crate (`examples/verify_elf_compiled.rs`
    // takes a real file path, not embedded bytes).
    let mut tmp = tempfile::NamedTempFile::new().expect("create real temp file");
    tmp.write_all(&bytes).expect("write real compiled ELF bytes to temp file");
    tmp.flush().unwrap();
    let tmp = tmp.into_temp_path();
    let path = tmp.to_str().expect("temp path is valid utf8").to_string();

    let out = run_example("verify_elf_compiled", &[&path]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    eprintln!("[verify_d] real stdout: {stdout:?}");
    eprintln!("[verify_d] real exit status: {:?}", out.status);
    assert_eq!(stdout, "hello from real elf\n", "real compiled binary's write(1, ..) syscall should have produced exactly its source-coded message");
    assert_eq!(out.status.code(), Some(42), "real compiled binary's exit(42) syscall should have propagated as the real process exit code");
}

/// The regression test: reproduces the exact guest/host address-mapping bug
/// found building the reference prototype (`guest_interp.rs`) -- a
/// RIP-relative `lea` computing a pointer, then a later instruction
/// dereferencing it. Without `decode::resolve_addr` adding the image's real
/// base address, `rdi` below would hold a small guest-relative offset (the
/// DATA label's offset from the `lea`, a handful of bytes) instead of a real
/// host pointer; the following `mov eax, [rdi]` would then read from that
/// bogus low address -- a real segfault outside a test harness, silently
/// wrong data if it happened to land in mapped memory. This test asserts
/// BOTH the computed address is exactly right AND the dereferenced value is
/// exactly right, in-process (no subprocess needed -- this program never
/// reaches `exit`).
#[test]
fn regression_rip_relative_lea_then_dereference() {
    let mut em = Emitter::new();
    let lea_disp = em.here() + 3;
    em.bytes(&[0x48, 0x8D, 0x3D, 0x00, 0x00, 0x00, 0x00]); // lea rdi, [rip+DATA]
    em.bytes(&[0x8B, 0x07]); // mov eax, [rdi]
    let data_off = em.here();
    em.bytes(&0xDEAD_BEEFu32.to_le_bytes());
    em.patch_i32(lea_disp, data_off);

    let image = GuestImage::new(&em.buf, 0, 4096);
    let mut cpu = Cpu::new(image.entry);

    decode_x86_64::step(&mut cpu, &image.buf, image.base).expect("lea should decode and execute");
    assert_eq!(cpu.get(RDI), image.addr_of(data_off), "lea must compute base + end_of_instruction + disp -- a REAL host pointer, not a bare guest-relative offset");

    decode_x86_64::step(&mut cpu, &image.buf, image.base).expect("mov eax,[rdi] should decode and execute (this is the real dereference -- would segfault on the bug this test guards)");
    assert_eq!(cpu.get(RAX) as u32, 0xDEAD_BEEF, "dereferencing the lea'd pointer must read back the real bytes placed at DATA");
}

#[test]
fn unimplemented_opcode_is_a_clear_fault_not_silent_wrong_behavior() {
    let mut em = Emitter::new();
    em.b(0xF4); // HLT -- deliberately not decoded by this crate
    let image = GuestImage::new(&em.buf, 0, 4096);
    let err = agenterm_guestcore::run_guest(&image, ExitMode::Return).unwrap_err();
    match err {
        GuestFault::UnimplementedOpcode { byte, .. } => assert_eq!(byte, 0xF4),
        other => panic!("expected UnimplementedOpcode, got {other}"),
    }
}

#[test]
fn unimplemented_syscall_is_a_clear_fault() {
    let mut em = Emitter::new();
    em.bytes(&[0xB8, 0x9E, 0x00, 0x00, 0x00]); // mov eax, 158 (arch_prctl -- no honest Intent equivalent)
    em.bytes(&[0x0F, 0x05]); // syscall
    let image = GuestImage::new(&em.buf, 0, 4096);
    let err = agenterm_guestcore::run_guest(&image, ExitMode::Return).unwrap_err();
    match err {
        GuestFault::UnimplementedSyscall { number } => assert_eq!(number, 158),
        other => panic!("expected UnimplementedSyscall, got {other}"),
    }
}

#[test]
fn write_to_non_stdout_fd_is_rejected_not_misrouted() {
    let mut em = Emitter::new();
    em.bytes(&[0xBF, 0x05, 0x00, 0x00, 0x00]); // mov edi, 5   (some fd that is not 1)
    em.bytes(&[0x31, 0xF6]); // xor esi, esi
    em.bytes(&[0x31, 0xD2]); // xor edx, edx
    em.bytes(&[0xB8, 0x01, 0x00, 0x00, 0x00]); // mov eax, 1 (write)
    em.bytes(&[0x0F, 0x05]); // syscall
    let image = GuestImage::new(&em.buf, 0, 4096);
    let err = agenterm_guestcore::run_guest(&image, ExitMode::Return).unwrap_err();
    match err {
        GuestFault::UnsupportedWriteFd { fd } => assert_eq!(fd, 5),
        other => panic!("expected UnsupportedWriteFd, got {other}"),
    }
}

#[test]
fn write_mode_open_is_rejected_not_forced_through_readonly_open() {
    // openat(AT_FDCWD, "<ignored -- flags alone already reject this>", O_WRONLY|O_CREAT, 0644)
    let mut em = Emitter::new();
    em.bytes(&[0xBF, 0x00, 0x00, 0x00, 0x00]); // mov edi, 0
    em.bytes(&[0x31, 0xF6]); // xor esi, esi   (path ptr -- never read, flags reject first)
    em.bytes(&[0xBA, 0x41, 0x00, 0x00, 0x00]); // mov edx, 0x41 (O_WRONLY|O_CREAT)
    em.bytes(&[0xB8, 0x01, 0x01, 0x00, 0x00]); // mov eax, 257 (openat)
    em.bytes(&[0x0F, 0x05]); // syscall
    let image = GuestImage::new(&em.buf, 0, 4096);
    let err = agenterm_guestcore::run_guest(&image, ExitMode::Return).unwrap_err();
    match err {
        GuestFault::UnsupportedOpenFlags { flags } => assert_eq!(flags, 0x41),
        other => panic!("expected UnsupportedOpenFlags, got {other}"),
    }
}
