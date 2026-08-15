//! Chassis-L2 AOT bytecode VM. Included via path so this test crate compiles
//! before `lib.rs` wires `mod bytecode` / `mod vm`.

#[path = "../src/bytecode.rs"]
mod bytecode;
#[path = "../src/vm.rs"]
mod vm;

use bytecode::{IrOp, L2Source, Op, Program, assemble, decode};
use vm::{CapHost, DEFAULT_MAX_STEPS, run};

struct MockHost {
    calls: Vec<String>,
    reply: i64,
}

impl MockHost {
    fn new(reply: i64) -> Self {
        Self {
            calls: Vec::new(),
            reply,
        }
    }
}

impl CapHost for MockHost {
    fn call(&mut self, cap: &str) -> Result<i64, String> {
        self.calls.push(cap.to_string());
        Ok(self.reply)
    }
}

struct NullHost;

impl CapHost for NullHost {
    fn call(&mut self, cap: &str) -> Result<i64, String> {
        Err(format!("unexpected cap `{cap}`"))
    }
}

fn src(caps: &[&str], ops: Vec<IrOp>) -> L2Source {
    L2Source {
        caps: caps.iter().map(|s| (*s).to_string()).collect(),
        ops,
    }
}

fn compile(caps: &[&str], ops: Vec<IrOp>) -> Program {
    assemble(&src(caps, ops), None).expect("assemble")
}

#[test]
fn aot_run_add_1_plus_2_eq_3() {
    let program = compile(
        &[],
        vec![
            IrOp::Push(1),
            IrOp::Push(2),
            IrOp::Add,
            IrOp::Push(3),
            IrOp::Eq,
            IrOp::Halt,
        ],
    );
    let value = run(&program, &mut NullHost, DEFAULT_MAX_STEPS).expect("run");
    assert_eq!(value, 1);

    let sum = compile(
        &[],
        vec![IrOp::Push(1), IrOp::Push(2), IrOp::Add, IrOp::Halt],
    );
    assert_eq!(run(&sum, &mut NullHost, DEFAULT_MAX_STEPS).expect("sum"), 3);
}

#[test]
fn jump_if_zero_skips_a_path() {
    // 0: push 0
    // 1: jz 4
    // 2: push 99
    // 3: halt
    // 4: push 3
    // 5: halt
    let program = compile(
        &[],
        vec![
            IrOp::Push(0),
            IrOp::JumpIfZero(4),
            IrOp::Push(99),
            IrOp::Halt,
            IrOp::Push(3),
            IrOp::Halt,
        ],
    );
    assert_eq!(
        run(&program, &mut NullHost, DEFAULT_MAX_STEPS).expect("run"),
        3
    );
}

#[test]
fn call_cap_hits_mock_host_and_is_observed() {
    let program = compile(
        &["tabs.list"],
        vec![IrOp::Call("tabs.list".into()), IrOp::Halt],
    );
    let mut host = MockHost::new(42);
    let value = run(&program, &mut host, DEFAULT_MAX_STEPS).expect("run");
    assert_eq!(value, 42);
    assert_eq!(host.calls, ["tabs.list"]);
}

#[test]
fn step_budget_exceeded() {
    let program = compile(&[], vec![IrOp::Jump(0)]);
    let err = run(&program, &mut NullHost, 32).expect_err("budget");
    assert!(
        err.contains("step budget exceeded"),
        "unexpected error: {err}"
    );
}

#[test]
fn unknown_opcode_rejected() {
    let program = Program {
        code: vec![0xFF],
        caps: Vec::new(),
    };
    let err = run(&program, &mut NullHost, 16).expect_err("opcode");
    assert!(err.contains("unknown opcode"), "unexpected error: {err}");
}

#[test]
fn bad_jump_rejected() {
    let mut code = vec![Op::Jump as u8];
    code.extend_from_slice(&1i64.to_le_bytes());
    let program = Program {
        code,
        caps: Vec::new(),
    };
    let err = decode(&program.code).expect_err("bad jump");
    assert!(err.contains("bad jump"), "unexpected error: {err}");
    let err = run(&program, &mut NullHost, 16).expect_err("run bad jump");
    assert!(err.contains("bad jump"), "unexpected error: {err}");
}

#[test]
fn assemble_is_deterministic() {
    let source = src(
        &["tabs.list"],
        vec![
            IrOp::Push(1),
            IrOp::Push(2),
            IrOp::Add,
            IrOp::Call("tabs.list".into()),
            IrOp::Halt,
        ],
    );
    let a = assemble(&source, None).expect("a");
    let b = assemble(&source, None).expect("b");
    assert_eq!(a, b);
    assert_eq!(a.code, b.code);
    assert_eq!(a.caps, b.caps);

    let via_json = L2Source::from_json(
        r#"{
            "caps": ["tabs.list"],
            "ops": [
                ["push", 1],
                ["push", 2],
                ["add"],
                ["call", "tabs.list"],
                ["halt"]
            ]
        }"#,
    )
    .expect("json");
    let c = assemble(&via_json, None).expect("c");
    assert_eq!(a, c);
}

#[test]
fn assemble_rejects_unknown_cap_when_allow_list_is_some() {
    let source = src(&["not.a.capability"], vec![IrOp::Halt]);
    let allow = vec!["tabs.list".to_string()];
    let err = assemble(&source, Some(allow.as_slice())).expect_err("unknown");
    assert!(err.contains("unknown cap name"), "unexpected error: {err}");

    let ok = src(
        &["tabs.list"],
        vec![IrOp::Call("tabs.list".into()), IrOp::Halt],
    );
    assemble(&ok, Some(allow.as_slice())).expect("allowed");
    assemble(&ok, None).expect("no allow-list");
}

#[test]
fn assemble_rejects_too_many_ops() {
    let ops = vec![IrOp::Halt; bytecode::MAX_OPS + 1];
    let err = assemble(&src(&[], ops), None).expect_err("too many");
    assert!(err.contains("max is"), "unexpected error: {err}");
}
