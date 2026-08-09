//! Verification program (a): a loop with a real conditional branch, proving
//! `cmp`/`jl`/`add` and repeated real `write` syscalls actually execute.
//! Guest logic (hand-encoded x86_64, real Linux syscall ABI):
//!
//! ```text
//! ecx = 0
//! loop_top:
//!   write(1, "A", 1)
//!   ecx += 1
//!   if ecx < 3: goto loop_top
//! write(1, "\n", 1)
//! exit(0)
//! ```
//!
//! Expected real stdout: `AAA\n`, real exit code 0. Run directly with
//! `cargo run --example verify_loop_cond`, or see `tests/verify.rs` for the
//! spawned-subprocess assertion (`ExitMode::Real` below really calls
//! `ExitProcess`, which is why this is a subprocess-checked example and not
//! an in-process `#[test]`).

use agenterm_guestcore::testutil::Emitter;
use agenterm_guestcore::{ExitMode, GuestImage};

fn main() {
    let mut em = Emitter::new();
    em.bytes(&[0xB9, 0x00, 0x00, 0x00, 0x00]); // mov ecx, 0

    let loop_top = em.here();
    em.bytes(&[0xBF, 0x01, 0x00, 0x00, 0x00]); // mov edi, 1  (fd = stdout)
    let lea_a = em.here();
    em.bytes(&[0x48, 0x8D, 0x35]); // lea rsi, [rip+MSG_A]
    let lea_a_disp = em.here();
    em.i32_hole();
    em.bytes(&[0xBA, 0x01, 0x00, 0x00, 0x00]); // mov edx, 1  (len = 1)
    em.bytes(&[0xB8, 0x01, 0x00, 0x00, 0x00]); // mov eax, 1  (write)
    em.bytes(&[0x0F, 0x05]); // syscall
    em.bytes(&[0x83, 0xC1, 0x01]); // add ecx, 1
    em.bytes(&[0x83, 0xF9, 0x03]); // cmp ecx, 3
    let jl_hole = em.here() + 1;
    em.bytes(&[0x7C, 0x00]); // jl loop_top (patched below)
    em.patch_i8(jl_hole, loop_top);
    let _ = lea_a; // (label kept for readability; disp is what's patched)

    em.bytes(&[0xBF, 0x01, 0x00, 0x00, 0x00]); // mov edi, 1
    let lea_nl_disp = em.here() + 3;
    em.bytes(&[0x48, 0x8D, 0x35, 0x00, 0x00, 0x00, 0x00]); // lea rsi, [rip+MSG_NL]
    em.bytes(&[0xBA, 0x01, 0x00, 0x00, 0x00]); // mov edx, 1
    em.bytes(&[0xB8, 0x01, 0x00, 0x00, 0x00]); // mov eax, 1
    em.bytes(&[0x0F, 0x05]); // syscall (write)

    em.bytes(&[0xBF, 0x00, 0x00, 0x00, 0x00]); // mov edi, 0
    em.bytes(&[0xB8, 0x3C, 0x00, 0x00, 0x00]); // mov eax, 60
    em.bytes(&[0x0F, 0x05]); // syscall (exit)

    let msg_a_off = em.here();
    em.b(b'A');
    let msg_nl_off = em.here();
    em.b(b'\n');

    em.patch_i32(lea_a_disp, msg_a_off);
    em.patch_i32(lea_nl_disp, msg_nl_off);

    let image = GuestImage::new(&em.buf, 0, 4096);
    // Never returns: guest reaches `exit`, which really calls `ExitProcess`.
    let _ = agenterm_guestcore::run_guest(&image, ExitMode::Real);
    unreachable!("run_guest(ExitMode::Real) diverges on exit -- guest must not have reached it");
}
