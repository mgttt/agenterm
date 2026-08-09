//! Verification program (b): a real file read round trip -- `openat` +
//! `read` + `close`, routed through `Intent::FileOpen`/`FileRead`/
//! `FileClose` to real `CreateFileA`/`ReadFile`/`CloseHandle` calls, then
//! `write(1, ..)` to echo what was really read back off disk to real
//! stdout. The path to read is passed as `argv[1]` (the test harness creates
//! a real temp file with known content before spawning this).
//!
//! (Original scope note: an earlier draft of this program tried to prove a
//! WRITE round trip via `openat(O_WRONLY|O_CREAT)`+`write`+`close`. That has
//! no honest 1:1 nativecore `Intent` mapping -- `Intent::FileOpen` is
//! read-only by construction and `Intent::FileWrite` is an atomic
//! create+write+close in one native call, not decomposable across three
//! separate guest syscalls. See `src/intent_map.rs`'s header and the
//! crate README's syscall table. This program instead proves the READ path,
//! which genuinely is a clean 1:1 mapping.)

use std::env;

use agenterm_guestcore::testutil::Emitter;
use agenterm_guestcore::{ExitMode, GuestImage};

const PATH_RESERVE: usize = 512;
const BUF_RESERVE: usize = 512;

fn main() {
    let path = env::args().nth(1).expect("usage: verify_file_roundtrip <path-to-existing-file>");

    let mut em = Emitter::new();
    em.bytes(&[0xBF, 0x00, 0x00, 0x00, 0x00]); // mov edi, 0        (dirfd, ignored by this crate's openat mapping)
    let lea_path_disp = em.here() + 3;
    em.bytes(&[0x48, 0x8D, 0x35, 0x00, 0x00, 0x00, 0x00]); // lea rsi, [rip+PATH_BUF]
    em.bytes(&[0xBA, 0x00, 0x00, 0x00, 0x00]); // mov edx, 0        (flags = O_RDONLY)
    em.bytes(&[0xB8, 0x01, 0x01, 0x00, 0x00]); // mov eax, 257      (openat)
    em.bytes(&[0x0F, 0x05]); // syscall -> rax = fd
    em.bytes(&[0x89, 0xC7]); // mov edi, eax      (fd for the read below)
    let lea_buf_disp = em.here() + 3;
    em.bytes(&[0x48, 0x8D, 0x35, 0x00, 0x00, 0x00, 0x00]); // lea rsi, [rip+READ_BUF]
    em.bytes(&[0xBA, 0x00, 0x02, 0x00, 0x00]); // mov edx, 512      (cap)
    em.bytes(&[0x31, 0xC0]); // xor eax, eax      (read = 0)
    em.bytes(&[0x0F, 0x05]); // syscall -> rax = n
    em.bytes(&[0x89, 0xC2]); // mov edx, eax      (n, saved before eax is reused)
    em.bytes(&[0xB8, 0x03, 0x00, 0x00, 0x00]); // mov eax, 3        (close; rdi still = fd)
    em.bytes(&[0x0F, 0x05]); // syscall (close)
    em.bytes(&[0xBF, 0x01, 0x00, 0x00, 0x00]); // mov edi, 1        (fd = stdout; rsi/rdx still point at buf/n)
    em.bytes(&[0xB8, 0x01, 0x00, 0x00, 0x00]); // mov eax, 1        (write)
    em.bytes(&[0x0F, 0x05]); // syscall (write) -> echoes the real bytes read off disk
    em.bytes(&[0xBF, 0x00, 0x00, 0x00, 0x00]); // mov edi, 0
    em.bytes(&[0xB8, 0x3C, 0x00, 0x00, 0x00]); // mov eax, 60
    em.bytes(&[0x0F, 0x05]); // syscall (exit)

    let path_buf_off = em.here();
    em.bytes(&vec![0u8; PATH_RESERVE]);
    let read_buf_off = em.here();
    em.bytes(&vec![0u8; BUF_RESERVE]);

    em.patch_i32(lea_path_disp, path_buf_off);
    em.patch_i32(lea_buf_disp, read_buf_off);

    let mut image = GuestImage::new(&em.buf, 0, 4096);
    let path_bytes = path.as_bytes();
    assert!(path_bytes.len() < PATH_RESERVE, "path too long for the reserved scratch buffer");
    image.write_cstr(path_buf_off, path_bytes);

    let _ = agenterm_guestcore::run_guest(&image, ExitMode::Real);
    unreachable!("run_guest(ExitMode::Real) diverges on exit -- guest must not have reached it");
}
