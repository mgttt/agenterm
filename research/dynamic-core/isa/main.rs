//! Driver for the ISA-axis experiment (Q5).
//!
//! Q1 fixed the ISA at x86-64 and varied the ABI. Q5 adds a SECOND ISA (aarch64) and
//! asks: is the neutral IR still neutral across the ISA axis, and what does a second
//! ISA actually cost? This driver:
//!   (1) lowers the SAME three Q1 payloads (byte-identical `payloads.rs` / `ir.rs`)
//!       through two aarch64 targets (Linux SVC, Windows symbol reach);
//!   (2) reports emitted byte sizes (comparable to Q1's x86 numbers);
//!   (3) VALIDATES the hand-written aarch64 encoder against LLVM ground truth
//!       (rustc --emit obj), since no aarch64 host/qemu is available to execute.
//!
//! Honest execution split (mirrors Q1's SysV side): aarch64 code is byte-measured and
//! encoder-validated, NOT executed. The lowering STRUCTURE is identical to Q1's x86
//! path, which WAS executed on Win64.

#[path = "spec/ir.rs"]
mod ir;
#[path = "lower/a64.rs"]
mod a64;
#[path = "lower/common_a64.rs"]
mod common_a64;
#[path = "lower/a64_linux.rs"]
mod a64_linux;
#[path = "lower/a64_win.rs"]
mod a64_win;
#[path = "payloads/payloads.rs"]
mod payloads;

use ir::Module;

fn lower_both(m: &Module) -> (Vec<u8>, Vec<u8>) {
    common_a64::set_externs(&m.externs);
    let lin = common_a64::lower(m, &a64_linux::A64Linux);
    common_a64::set_externs(&m.externs);
    let win = common_a64::lower(m, &a64_win::A64Win);
    (lin, win)
}

/// Emit each instruction with the hand-written encoder and compare the resulting
/// 4-byte word to LLVM's encoding of the same mnemonic (captured via
/// `rustc --emit obj --target aarch64-unknown-linux-gnu`). Any mismatch => the
/// encoder is wrong and every byte count below is meaningless.
fn validate_encoder() -> bool {
    use a64::*;
    // (name, expected LLVM words, emit). All expected words captured from
    // `rustc --emit obj --target aarch64-unknown-linux-gnu` disassembly.
    #[allow(clippy::type_complexity)]
    let cases: Vec<(&str, Vec<u32>, Box<dyn Fn(&mut A64)>)> = vec![
        ("movz x0,#163",       vec![0xD280_1460], Box::new(|a: &mut A64| a.mov_imm(X0, 163))),
        ("movz+movk x0",       vec![0xD280_1460, 0xF2B5_79A0], Box::new(|a: &mut A64| a.mov_imm(X0, (0xABCDu64 << 16) | 163))),
        ("add x0,x1,x2",       vec![0x8B02_0020], Box::new(|a: &mut A64| a.add(X0, X1, X2))),
        ("sub x3,x4,x5",       vec![0xCB05_0083], Box::new(|a: &mut A64| a.sub(X3, X4, X5))),
        ("mul x6,x7,x8",       vec![0x9B08_7CE6], Box::new(|a: &mut A64| a.mul(X6, X7, X8))),
        ("eor x0,x1,x2",       vec![0xCA02_0020], Box::new(|a: &mut A64| a.eor(X0, X1, X2))),
        ("and x9,x10,x11",     vec![0x8A0B_0149], Box::new(|a: &mut A64| a.and(X9, X10, X11))),
        ("orr x0,x1,x2",       vec![0xAA02_0020], Box::new(|a: &mut A64| a.orr(X0, X1, X2))),
        ("lsl x0,x1,#4",       vec![0xD37C_EC20], Box::new(|a: &mut A64| a.lsl_imm(X0, X1, 4))),
        ("lsr x0,x1,#4",       vec![0xD344_FC20], Box::new(|a: &mut A64| a.lsr_imm(X0, X1, 4))),
        ("ldr x0,[x1,#24]",    vec![0xF940_0C20], Box::new(|a: &mut A64| a.ldr(X0, X1, 24))),
        ("str x2,[x3,#16]",    vec![0xF900_0862], Box::new(|a: &mut A64| a.str(X2, X3, 16))),
        ("ldrb w0,[x1,#0]",    vec![0x3940_0020], Box::new(|a: &mut A64| a.ldrb(X0, X1, 0))),
        ("strb w2,[x3,#5]",    vec![0x3900_1462], Box::new(|a: &mut A64| a.strb(X2, X3, 5))),
        ("ldr w0,[x1,#8]",     vec![0xB940_0820], Box::new(|a: &mut A64| a.ldr_w(X0, X1, 8))),
        ("str w2,[x3,#4]",     vec![0xB900_0462], Box::new(|a: &mut A64| a.str_w(X2, X3, 4))),
        ("cmp x1,x2",          vec![0xEB02_003F], Box::new(|a: &mut A64| a.cmp(X1, X2))),
        ("cset x0,cc",         vec![0x9A9F_27E0], Box::new(|a: &mut A64| a.cset_lo(X0))),
        ("svc #0",             vec![0xD400_0001], Box::new(|a: &mut A64| a.svc0())),
        ("blr x9",             vec![0xD63F_0120], Box::new(|a: &mut A64| a.blr(X9))),
        ("ret",                vec![0xD65F_03C0], Box::new(|a: &mut A64| a.ret())),
        ("add x0,x1,#64",      vec![0x9101_0020], Box::new(|a: &mut A64| a.add_imm(X0, X1, 64))),
        ("sub sp,sp,#256",     vec![0xD104_03FF], Box::new(|a: &mut A64| a.sub_imm(SP, SP, 256))),
        ("add sp,sp,#256",     vec![0x9104_03FF], Box::new(|a: &mut A64| a.add_imm(SP, SP, 256))),
        ("add x5,sp,#16",      vec![0x9100_43E5], Box::new(|a: &mut A64| a.add_imm(X5, SP, 16))),
        ("mov x9,x10",         vec![0xAA0A_03E9], Box::new(|a: &mut A64| a.mov_rr(X9, X10))),
    ];
    let mut ok = true;
    let mut n = 0;
    for (name, want, emit) in &cases {
        let mut a = A64::new();
        emit(&mut a);
        let got: Vec<u32> = a.code.chunks(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
        if &got != want {
            println!("  ENCODER MISMATCH {name}: got {got:08x?} want {want:08x?}");
            ok = false;
        } else {
            n += 1;
        }
    }
    println!("  {n}/{} instruction encodings match LLVM", cases.len());
    ok
}

fn main() {
    println!("== ISA-axis experiment (Q5): lower the 3 Q1 payloads to a SECOND ISA (aarch64) ==\n");

    println!("== encoder validation vs LLVM ground truth ==");
    let enc_ok = validate_encoder();
    println!("  encoder: {}\n", if enc_ok { "ALL MATCH LLVM" } else { "FAILED" });

    let pure = payloads::pure_compute();
    let rhp = payloads::read_hash_print();
    let spawn = payloads::spawn_echo();

    let (pure_l, pure_w) = lower_both(&pure);
    let (rhp_l, rhp_w) = lower_both(&rhp);
    let (spawn_l, spawn_w) = lower_both(&spawn);

    println!("emitted aarch64 code size (bytes)   a64-linux   a64-win   identical?");
    let rows = [
        ("pure_compute", &pure_l, &pure_w),
        ("read_hash_print", &rhp_l, &rhp_w),
        ("spawn_echo", &spawn_l, &spawn_w),
    ];
    for (n, l, w) in rows {
        println!("  {:<20} {:>7}   {:>7}   {}", n, l.len(), w.len(), l == w);
    }

    let _ = std::fs::create_dir_all("out");
    for (n, l, w) in rows {
        let _ = std::fs::write(format!("out/{n}.a64-linux.bin"), l);
        let _ = std::fs::write(format!("out/{n}.a64-win.bin"), w);
    }
    println!("\n(aarch64 not executable on this x86 host; byte-measured + encoder-validated only)");
    println!("== done ==");
}
