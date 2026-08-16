//! dyn.3 acceptance: the `Op` stream is execution-mode-agnostic.
//!
//! The same `&[Op]` runs two ways:
//!  - the portable interpreter ([`Interp`]) — no mmap, no unsafe, runs anywhere
//!    including iOS/Wasm;
//!  - the JIT engine ([`Engine`]) — lowers to host bytes and jumps in (unix
//!    x86_64 only).
//!
//! Both must produce identical results. The interpreter scenes are ungated so
//! they exercise the no-JIT floor on every target.

use agenterm_dyn::{Interp, Op};

/// Scene 1: call a name defined only in a later append (forward reference).
fn scene_1() -> Vec<Vec<Op>> {
    vec![
        vec![Op::Label("main".into()), Op::Call("answer".into()), Op::Ret],
        vec![Op::Label("answer".into()), Op::MovRaxImm(1234), Op::Ret],
    ]
}

/// Scene 2: a second append calls a name the first append defined.
fn scene_2() -> Vec<Vec<Op>> {
    vec![
        vec![Op::Label("helper".into()), Op::MovRaxImm(7), Op::Ret],
        vec![Op::Label("caller".into()), Op::Call("helper".into()), Op::Ret],
    ]
}

fn run_interp(batches: &[Vec<Op>], entry: &str) -> i64 {
    let mut it = Interp::new();
    for b in batches {
        it.assemble(b).expect("interp assemble");
    }
    it.enter_i64(entry).expect("interp enter")
}

#[test]
fn interp_runs_scene_1_anywhere() {
    assert_eq!(run_interp(&scene_1(), "main"), 1234);
}

#[test]
fn interp_runs_scene_2_anywhere() {
    assert_eq!(run_interp(&scene_2(), "caller"), 7);
}

#[cfg(all(unix, target_arch = "x86_64"))]
mod jit_parity {
    use super::*;
    use agenterm_dyn::Engine;

    fn run_jit(batches: &[Vec<Op>], entry: &str) -> i64 {
        let mut eng = Engine::new(1024).expect("engine");
        for b in batches {
            eng.assemble(b).expect("jit assemble");
        }
        // SAFETY: entry is a valid extern "C" fn() -> i64 with all names resolved.
        unsafe { eng.enter_i64(entry) }.expect("jit enter")
    }

    #[test]
    fn jit_and_interp_agree_on_both_scenes() {
        assert_eq!(run_jit(&scene_1(), "main"), run_interp(&scene_1(), "main"));
        assert_eq!(
            run_jit(&scene_2(), "caller"),
            run_interp(&scene_2(), "caller")
        );
        // And both equal the known-good constants.
        assert_eq!(run_jit(&scene_1(), "main"), 1234);
        assert_eq!(run_jit(&scene_2(), "caller"), 7);
    }
}
