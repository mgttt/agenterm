//! Exec-base acceptance (Linux x86_64): hand-written host bytes staged into a
//! W^X code buffer, then entered.
//!
//! 1. `mov rax, 42; ret` entered as `extern "C" fn() -> i64` returns 42.
//! 2. a hand-written `call` into the `dlsym`-resolved `getpid` returns the same
//!    PID as calling `getpid` directly.

#![cfg(all(unix, target_arch = "x86_64"))]

use agenterm_dyn::{BufferState, CodeBuffer, NameTable, x86_64_call_thunk, x86_64_mov_rax_ret};

#[test]
fn acceptance_1_mov_rax_42_ret() {
    let mut buf = CodeBuffer::new(64).expect("map code buffer");
    assert_eq!(buf.state(), BufferState::Writable);

    let off = buf
        .append(&x86_64_mov_rax_ret(42))
        .expect("stage mov rax,42; ret");

    buf.make_executable().expect("W^X flip to executable");
    assert_eq!(buf.state(), BufferState::Executable);

    // SAFETY: the bytes at `off` are a complete `extern "C" fn() -> i64` body.
    let got = unsafe { buf.enter_i64(off) }.expect("enter buffer");
    assert_eq!(got, 42);
}

#[test]
fn acceptance_2_call_getpid_matches_direct() {
    // Resolve getpid as a dlsym address and record it in the name table as a
    // foreign (outward call-gate) entry.
    let lib = unsafe { libloading::Library::new("libc.so.6") }.expect("load libc");
    let getpid: libloading::Symbol<unsafe extern "C" fn() -> libc::pid_t> =
        unsafe { lib.get(b"getpid\0") }.expect("resolve getpid");
    let getpid_addr = unsafe { getpid.into_raw() }.into_raw() as usize;

    let mut names = NameTable::new();
    names.define_foreign("getpid", getpid_addr);
    let target = names.addr_of("getpid").expect("getpid recorded");

    let mut buf = CodeBuffer::new(64).expect("map code buffer");
    let off = buf
        .append(&x86_64_call_thunk(target))
        .expect("stage call thunk");
    buf.make_executable().expect("W^X flip to executable");

    // SAFETY: the thunk is a valid `extern "C" fn() -> i64` that calls getpid.
    let via_buffer = unsafe { buf.enter_i64(off) }.expect("enter buffer");
    let direct = unsafe { libc::getpid() } as i64;
    assert_eq!(via_buffer, direct);
}
