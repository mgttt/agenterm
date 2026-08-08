#![no_std]
#![no_main]
//! Q10 STENCIL TEMPLATES — the build-time input to a REAL optimizing compiler.
//!
//! Each `st_*` is a copy-and-patch stencil: rustc -O2 turns it into a self-contained
//! machine-code fragment (ending in `ret`, which the build tool strips). All VM state
//! lives in a MEMORY register file (`regfile[]`), so nothing is carried in physical
//! registers across stencils — which is what makes them freely concatenable and makes
//! branches trivial (no register-state to reconcile at joins).
//!
//! HOLES are extern symbols the compiler leaves as R_X86_64_PC32 relocations. The build
//! tool maps each symbol name -> a hole KIND the runtime applier resolves per IR instr:
//!   H_RD/H_RS/H_RB : a regfile slot  -> patched to &regfile[idx]
//!   H_A0..H_A3     : a regfile slot (call arg)
//!   H_IMM/H_OFF    : a CONSTANT-POOL slot holding an imm64/off (value hole)
//!   H_ENV / H_CALL*: an env-table slot -> patched to &env[idx]
//! Control flow (JMP/JCC/LABEL) is deliberately NOT a stencil: a compiled fragment
//! cannot leave CPU flags live across its boundary, so the applier emits it (patch.rs).
//!
//! Compiled with `-C relocation-model=static` so refs are DIRECT PC32 (no GOT).

extern "C" {
    static mut H_RD: u64;
    static H_RS: u64;
    static H_RB: u64;
    static H_IMM: u64;
    static H_OFF: u64;
    static H_A0: u64;
    static H_A1: u64;
    static H_A2: u64;
    static H_A3: u64;
    static H_ENV: u64;
    static H_CALL0: extern "sysv64" fn() -> u64;
    static H_CALL1: extern "sysv64" fn(u64) -> u64;
    static H_CALL2: extern "sysv64" fn(u64, u64) -> u64;
    static H_CALL3: extern "sysv64" fn(u64, u64, u64) -> u64;
    static H_CALL4: extern "sysv64" fn(u64, u64, u64, u64) -> u64;
}

#[no_mangle]
pub unsafe extern "C" fn st_imm() {
    H_RD = H_IMM;
}
#[no_mangle]
pub unsafe extern "C" fn st_mov() {
    H_RD = H_RS;
}
#[no_mangle]
pub unsafe extern "C" fn st_add() {
    H_RD = H_RD.wrapping_add(H_RS);
}
#[no_mangle]
pub unsafe extern "C" fn st_sub() {
    H_RD = H_RD.wrapping_sub(H_RS);
}
#[no_mangle]
pub unsafe extern "C" fn st_mul() {
    H_RD = H_RD.wrapping_mul(H_RS);
}
#[no_mangle]
pub unsafe extern "C" fn st_and() {
    H_RD &= H_RS;
}
#[no_mangle]
pub unsafe extern "C" fn st_or() {
    H_RD |= H_RS;
}
#[no_mangle]
pub unsafe extern "C" fn st_xor() {
    H_RD ^= H_RS;
}
#[no_mangle]
pub unsafe extern "C" fn st_shr() {
    H_RD >>= H_IMM;
}
#[no_mangle]
pub unsafe extern "C" fn st_shl() {
    H_RD <<= H_IMM;
}
#[no_mangle]
pub unsafe extern "C" fn st_ld8() {
    let p = H_RB.wrapping_add(H_OFF) as *const u8;
    H_RD = *p as u64;
}
#[no_mangle]
pub unsafe extern "C" fn st_ld64() {
    let p = H_RB.wrapping_add(H_OFF) as *const u64;
    H_RD = *p;
}
#[no_mangle]
pub unsafe extern "C" fn st_st8() {
    let p = H_RB.wrapping_add(H_OFF) as *mut u8;
    *p = H_RS as u8;
}
#[no_mangle]
pub unsafe extern "C" fn st_st64() {
    let p = H_RB.wrapping_add(H_OFF) as *mut u64;
    *p = H_RS;
}
#[no_mangle]
pub unsafe extern "C" fn st_lde() {
    H_RD = H_ENV;
}
#[no_mangle]
pub unsafe extern "C" fn st_call0() {
    H_RD = (H_CALL0)();
}
#[no_mangle]
pub unsafe extern "C" fn st_call1() {
    H_RD = (H_CALL1)(H_A0);
}
#[no_mangle]
pub unsafe extern "C" fn st_call2() {
    H_RD = (H_CALL2)(H_A0, H_A1);
}
#[no_mangle]
pub unsafe extern "C" fn st_call3() {
    H_RD = (H_CALL3)(H_A0, H_A1, H_A2);
}
#[no_mangle]
pub unsafe extern "C" fn st_call4() {
    H_RD = (H_CALL4)(H_A0, H_A1, H_A2, H_A3);
}

#[panic_handler]
fn p(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
