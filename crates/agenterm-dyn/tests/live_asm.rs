//! dyn.2 live-assembly acceptance (Linux x86_64): the time axis made real.
//!
//! Two scenes, both through the engine's `Op` sequence → bytes → enter path:
//!  1. Call a name that does not exist yet, define it in a later append, run it
//!     — the parked forward reference is backpatched and the result is correct.
//!  2. A second append calls a name the first append defined — names live
//!     across appends.

#![cfg(all(unix, target_arch = "x86_64"))]

use agenterm_dyn::{Engine, Op};

#[test]
fn scene_1_call_undefined_name_then_supply_it() {
    let mut eng = Engine::new(256).expect("engine");

    // Append #1: `main` calls `answer`, which is NOT defined yet.
    //   main:  call answer   ; rax := answer()
    //          ret           ; return rax
    eng.assemble(&[
        Op::Label("main".into()),
        Op::Call("answer".into()),
        Op::Ret,
    ])
    .expect("append main with a forward reference");
    assert_eq!(eng.pending_count(), 1, "answer is parked as pending");

    // Append #2: define `answer` — this backpatches the parked call site.
    //   answer: mov rax, 1234
    //           ret
    eng.assemble(&[
        Op::Label("answer".into()),
        Op::MovRaxImm(1234),
        Op::Ret,
    ])
    .expect("append answer, resolving the forward reference");
    assert_eq!(eng.pending_count(), 0, "forward reference resolved");

    // Run `main`: it calls the now-defined `answer` and returns its value.
    // SAFETY: `main` is a valid extern "C" fn() -> i64 with all names resolved.
    let got = unsafe { eng.enter_i64("main") }.expect("enter main");
    assert_eq!(got, 1234);
}

#[test]
fn scene_2_second_append_calls_first_appends_name() {
    let mut eng = Engine::new(256).expect("engine");

    // Append #1 defines `helper`.
    //   helper: mov rax, 7
    //           ret
    eng.assemble(&[
        Op::Label("helper".into()),
        Op::MovRaxImm(7),
        Op::Ret,
    ])
    .expect("append helper");

    // Append #2, landed later in the same live buffer, calls `helper` by name.
    //   caller: call helper   ; rax := helper()
    //           ret
    eng.assemble(&[
        Op::Label("caller".into()),
        Op::Call("helper".into()),
        Op::Ret,
    ])
    .expect("append caller referencing the earlier name");
    assert_eq!(eng.pending_count(), 0, "helper resolved immediately (backward ref)");

    // SAFETY: `caller` is a valid extern "C" fn() -> i64; `helper` is resolved.
    let got = unsafe { eng.enter_i64("caller") }.expect("enter caller");
    assert_eq!(got, 7);
}
