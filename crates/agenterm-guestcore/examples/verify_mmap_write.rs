//! Verification program (c): `mmap` (anonymous) -> `Intent::Alloc` -> a real
//! `VirtualAlloc`, then the guest genuinely writes into the returned region
//! and reads it back through a SEPARATE real syscall (`write(1, ptr, 8)`),
//! proving the memory is real, host-writable AND host-readable -- not just
//! "the call succeeded". Guest logic:
//!
//! ```text
//! ptr = mmap(NULL, 4096, PROT_READ|PROT_WRITE, MAP_PRIVATE|MAP_ANONYMOUS, -1, 0)
//! r8 = ptr
//! *(u32*)(r8+0) = "MMAP" (little-endian bytes)
//! *(u32*)(r8+4) = "-OK\n" (little-endian bytes)
//! write(1, r8, 8)
//! exit(0)
//! ```
//!
//! Expected real stdout: `MMAP-OK\n`, real exit code 0.

use agenterm_guestcore::testutil::Emitter;
use agenterm_guestcore::{ExitMode, GuestImage};

fn main() {
    let mut em = Emitter::new();
    em.bytes(&[0xBF, 0x00, 0x00, 0x00, 0x00]); // mov edi, 0            (addr = NULL)
    em.bytes(&[0xBE, 0x00, 0x10, 0x00, 0x00]); // mov esi, 4096         (length)
    em.bytes(&[0xBA, 0x03, 0x00, 0x00, 0x00]); // mov edx, 3            (prot, ignored by Alloc)
    em.bytes(&[0x41, 0xBA, 0x22, 0x00, 0x00, 0x00]); // mov r10d, 0x22  (MAP_PRIVATE|MAP_ANONYMOUS)
    em.bytes(&[0xB8, 0x09, 0x00, 0x00, 0x00]); // mov eax, 9            (mmap)
    em.bytes(&[0x0F, 0x05]); // syscall -> rax = ptr (a real host address)
    em.bytes(&[0x49, 0x89, 0xC0]); // mov r8, rax           (keep the full 64-bit pointer)

    // Write real bytes into the mapped region via two 32-bit stores --
    // exercises `C7 /0` (mov rm32, imm32) against a REX.B-extended base
    // register, addressing a region this crate never allocated as part of
    // its own flat guest image (see mem.rs's header: "guest address == host
    // address" holds for a VirtualAlloc'd region exactly the same way it
    // holds for the image buffer).
    em.bytes(&[0x41, 0xC7, 0x00, 0x4D, 0x4D, 0x41, 0x50]); // mov dword [r8],   0x50414D4D ("MMAP" LE)
    em.bytes(&[0x41, 0xC7, 0x40, 0x04, 0x2D, 0x4F, 0x4B, 0x0A]); // mov dword [r8+4], 0x0A4B4F2D ("-OK\n" LE)

    em.bytes(&[0x4C, 0x89, 0xC6]); // mov rsi, r8           (buf = the mmap'd pointer itself)
    em.bytes(&[0xBF, 0x01, 0x00, 0x00, 0x00]); // mov edi, 1
    em.bytes(&[0xBA, 0x08, 0x00, 0x00, 0x00]); // mov edx, 8
    em.bytes(&[0xB8, 0x01, 0x00, 0x00, 0x00]); // mov eax, 1
    em.bytes(&[0x0F, 0x05]); // syscall (write) -- reads real bytes back out of the mmap'd region

    em.bytes(&[0xBF, 0x00, 0x00, 0x00, 0x00]); // mov edi, 0
    em.bytes(&[0xB8, 0x3C, 0x00, 0x00, 0x00]); // mov eax, 60
    em.bytes(&[0x0F, 0x05]); // syscall (exit)

    let image = GuestImage::new(&em.buf, 0, 4096);
    let _ = agenterm_guestcore::run_guest(&image, ExitMode::Real);
    unreachable!("run_guest(ExitMode::Real) diverges on exit -- guest must not have reached it");
}
