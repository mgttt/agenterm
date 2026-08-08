//! Q20 — the shape boundary of ④ `call` (float / struct-by-value / varargs / sret).
//!
//! Clean-room, std harness (Q6's style: raw `rustc`, no Cargo, no workspace).
//! Reuses only the ①②③④ *contract* established by `core/kernel.rs` and the
//! neutral-IR "one value type: Word" discipline from `ir/spec/ir.rs` — no code
//! is copied from either.
//!
//! PINNED CRITERIA (written before any code below was run; not edited after):
//!
//! ① BOOLEAN GATE. Four real Win32/CRT calls, each requiring something the
//!    baseline ④ (`core/kernel.rs::call`, integer/pointer-word-only, arity-only
//!    match) cannot express, executed on the real box, checked against an
//!    EXACT expected value (no floating rounding ambiguity):
//!      - `sqrt(144.0) == 12.0`                         (msvcrt.dll, 1 float arg+ret)
//!      - `pow(2.0, 10.0) == 1024.0`                     (msvcrt.dll, 2 float args)
//!      - `ldexp(1.5, 4) == 24.0`                        (msvcrt.dll, MIXED float+int
//!                                                         args, at DIFFERENT
//!                                                         positions — the case that
//!                                                         would expose a Win64
//!                                                         positional-vs-SysV
//!                                                         separate-counter bug)
//!      - `PtInRect(&RECT{0,0,100,100}, POINT{50,50})`   (user32.dll, struct-by-value
//!         == TRUE, and PtInRect(&same, POINT{200,200}) == FALSE
//!                                                         POINT (8B, all-integer
//!                                                         fields) passed BY VALUE)
//!    PASS iff all four run and match exactly.
//!
//! ② MAIN CRITERION — is float support a DATA extension (a `Kind` tag on the
//!    existing "word" signature description) or does it require a genuinely
//!    NEW primitive / new kernel entry point? Judged by construction: does
//!    `call_shaped` add a new *kind of thing* callable from the kernel (a
//!    sixth primitive), or does it stay inside ④'s existing shape (data
//!    description -> dispatch -> native call)?
//!    Struct-by-value: does the PtInRect test go through unmodified ④ (the
//!    pre-existing all-word baseline) or does it require new machinery?
//!
//! ③ COST — LOC delta of the extension vs the established baselines:
//!    `core/kernel.rs::call` (Windows arm, lines 237-268, 32 LOC, arity-only
//!    match, 0..=10 arity) and Q1's own reference number "~20-30 LOC/target
//!    for ABI placement" (`ir/RESULTS.md` ⑤).
//!
//! ④ RESIDUAL — after ①②③, what of R6 (float/SIMD, struct-by-value, varargs,
//!    sret) is genuinely closed vs still permanent, with reasoning.
//!
//! Deviations from these criteria, if any, are recorded in RESULTS.md ⑤, not
//! by silently changing this file.

#![allow(non_snake_case, dead_code)]

use std::fmt;

// ---------------------------------------------------------------------------
// ③ reach — UNCHANGED contract (mirrors core/kernel.rs::sym; not copied).
// ---------------------------------------------------------------------------
#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryA(name: *const u8) -> *mut u8;
    fn GetProcAddress(module: *mut u8, name: *const u8) -> *mut u8;
}

fn sym(module: &str, name: &str) -> *mut u8 {
    let m = format!("{module}\0");
    let n = format!("{name}\0");
    unsafe {
        let h = LoadLibraryA(m.as_ptr());
        assert!(!h.is_null(), "LoadLibraryA({module}) failed");
        let f = GetProcAddress(h, n.as_ptr());
        assert!(!f.is_null(), "GetProcAddress({module}!{name}) failed");
        f
    }
}

// ---------------------------------------------------------------------------
// ④ call — the BASELINE (arity-only, all-word). This is a faithful re-write
// of core/kernel.rs's Windows `call` arm's SHAPE (transmute-to-typed-fn +
// match), scoped down to the arities this file needs, so the LOC delta below
// is measured against a real, present baseline rather than a citation.
// ---------------------------------------------------------------------------
mod baseline {
    /// core/kernel.rs::call (Windows arm) shape, arity 0..=2 only (sufficient
    /// to anchor the LOC comparison — the real file goes to 11; that part is
    /// unchanged by this experiment and is NOT re-measured here).
    pub fn call_word(addr: *mut u8, nargs: usize, args: &[usize]) -> usize {
        unsafe {
            macro_rules! t {
                ($($p:ty),*) => { core::mem::transmute::<_, extern "win64" fn($($p),*) -> usize>(addr) };
            }
            match nargs {
                0 => (t!())(),
                1 => (t!(usize))(args[0]),
                2 => (t!(usize, usize))(args[0], args[1]),
                _ => panic!("baseline scoped to arity <= 2"),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ④ call — THE EXTENSION under test. A `Kind` TAG on each Word of the
// signature description. This is data ABOUT a Word (still one storage type:
// a u64 bit pattern — matches ir/spec/ir.rs's "one value type: Word"
// discipline), not a second IR/primitive value type. The dispatch mechanism
// (transmute to a typed extern "win64" fn, let the HOST CODEGEN place the
// argument) is UNCHANGED from the baseline above — only the table of typed
// signatures being transmuted-to grows, exactly the way Q1's ABI-placement
// axis was already "derived entirely from the semantic signature, zero IR
// involvement" (ir/RESULTS.md NON-leak). Float placement (XMM vs GPR) is
// delegated to Rust's own win64-ABI-aware codegen; this file supplies no
// register names, no XMM/GPR literal anywhere.
// ---------------------------------------------------------------------------
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Int,
    Float,
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", if *self == Kind::Int { "I" } else { "F" })
    }
}

/// Extended ④: `sig`/`ret` are the DATA description (spec §2 candidate 1).
/// `args[i]` is a u64: for `Kind::Int` it is the raw usize bits; for
/// `Kind::Float` it is `f64::to_bits(x)`. Returns a u64 (undo with
/// `f64::from_bits` at the call site if `ret == Kind::Float`).
///
/// NEW arms beyond the baseline's all-Int shapes are marked `// NEW`. The
/// all-Int arm is listed for completeness (dispatch symmetry) but is NOT new
/// capability -- it is exactly baseline::call_word's arity-2 case, restated.
pub fn call_shaped(addr: *mut u8, sig: &[Kind], ret: Kind, args: &[u64]) -> u64 {
    unsafe {
        match (sig, ret) {
            (&[Kind::Int, Kind::Int], Kind::Int) => {
                // == baseline::call_word arity 2. Not new capability.
                let f: extern "win64" fn(usize, usize) -> usize = core::mem::transmute(addr);
                f(args[0] as usize, args[1] as usize) as u64
            }
            (&[Kind::Float], Kind::Float) => {
                // NEW: 1 float arg + float return (sqrt).
                let f: extern "win64" fn(f64) -> f64 = core::mem::transmute(addr);
                f(f64::from_bits(args[0])).to_bits()
            }
            (&[Kind::Float, Kind::Float], Kind::Float) => {
                // NEW: 2 float args, tests XMM0+XMM1 (pow).
                let f: extern "win64" fn(f64, f64) -> f64 = core::mem::transmute(addr);
                f(f64::from_bits(args[0]), f64::from_bits(args[1])).to_bits()
            }
            (&[Kind::Float, Kind::Int], Kind::Float) => {
                // NEW: MIXED, float first then int (ldexp). This is the case
                // that would break under a naive "count floats separately"
                // (SysV-shaped) implementation applied to Win64: the int arg
                // must land in the 2nd POSITION's GPR (rdx), not the 1st
                // unused GPR (rcx). We supply no register hint at all --
                // this is Rust's win64 codegen enforcing the positional rule
                // by construction. See RESULTS.md ② for why this matters.
                let f: extern "win64" fn(f64, i32) -> f64 = core::mem::transmute(addr);
                f(f64::from_bits(args[0]), args[1] as i32).to_bits()
            }
            _ => panic!("call_shaped: shape {:?}->{:?} not in the tested subset", sig, ret),
        }
    }
}

// ---------------------------------------------------------------------------
// PtInRect(const RECT*, POINT) -> BOOL — struct-by-value (candidate 2).
//
// POINT { LONG x; LONG y; } is 8 bytes, ALL-INTEGER fields. Win64 classifies
// any <=8-byte aggregate as INTEGER class and passes it in a single GPR as
// raw bytes -- i.e. IDENTICAL to passing a plain u64. So this call goes
// through the UNMODIFIED baseline word-call (arity 2, both Int) -- proof
// that struct-by-value, when it fits in one register, needs ZERO extension
// to ④: no Kind::Struct, no new match arm, nothing. The only baked fact the
// CALLER needs is POINT's field ORDER (x is the low 32 bits, y is the high
// 32 bits) -- that is a layout fact in Q6/Q13's sense (an offsetof-class
// residue), not a call-primitive capability gap.
//
// RECT { LONG left,top,right,bottom; } (16 bytes) is passed BY POINTER here
// (as the real Win32 signature requires) -- allocated and baked the same way
// Q6/Q13 already established (VirtualAlloc + manual field writes at known
// offsets), not a new mechanism either.
// ---------------------------------------------------------------------------
fn pt_in_rect(addr: *mut u8, rect_ptr: *mut u8, x: i32, y: i32) -> bool {
    // pack POINT{x,y} into one u64: x = low 32 bits, y = high 32 bits
    // (little-endian struct layout, x declared first) -- the one baked fact.
    let point: u64 = (x as u32 as u64) | ((y as u32 as u64) << 32);
    let r = baseline::call_word(addr, 2, &[rect_ptr as usize, point as usize]);
    r != 0
}

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------
fn main() {
    let mut all_pass = true;
    let mut check = |name: &str, cond: bool, detail: String| {
        let tag = if cond { "PASS" } else { "FAIL" };
        println!("[{tag}] {name}  {detail}");
        if !cond {
            all_pass = false;
        }
    };

    // ---- ① sqrt: 1 float arg + float return ----
    let f_sqrt = sym("msvcrt.dll", "sqrt");
    let r = f64::from_bits(call_shaped(f_sqrt, &[Kind::Float], Kind::Float, &[144.0f64.to_bits()]));
    check("sqrt(144.0) == 12.0", r == 12.0, format!("got {r}"));

    // ---- ① pow: 2 float args (XMM0+XMM1) ----
    let f_pow = sym("msvcrt.dll", "pow");
    let r = f64::from_bits(call_shaped(
        f_pow,
        &[Kind::Float, Kind::Float],
        Kind::Float,
        &[2.0f64.to_bits(), 10.0f64.to_bits()],
    ));
    check("pow(2.0, 10.0) == 1024.0", r == 1024.0, format!("got {r}"));

    // ---- ① ldexp: MIXED float,int at DIFFERENT positions ----
    let f_ldexp = sym("msvcrt.dll", "ldexp");
    let r = f64::from_bits(call_shaped(
        f_ldexp,
        &[Kind::Float, Kind::Int],
        Kind::Float,
        &[1.5f64.to_bits(), 4u64],
    ));
    check("ldexp(1.5, 4) == 24.0", r == 24.0, format!("got {r}"));

    // ---- ① PtInRect: struct-by-value (POINT), through the UNMODIFIED baseline ----
    let f_ptinrect = sym("user32.dll", "PtInRect");
    // RECT{left=0,top=0,right=100,bottom=100} -- baked offsets 0,4,8,12,
    // same methodology as Q6/Q13 (VirtualAlloc a scratch buffer, write i32
    // fields at known byte offsets).
    let rect_buf: Vec<u8> = vec![0u8; 16];
    let rect_ptr = rect_buf.as_ptr() as *mut u8;
    unsafe {
        (rect_ptr.add(0) as *mut i32).write(0); // left
        (rect_ptr.add(4) as *mut i32).write(0); // top
        (rect_ptr.add(8) as *mut i32).write(100); // right
        (rect_ptr.add(12) as *mut i32).write(100); // bottom
    }
    let inside = pt_in_rect(f_ptinrect, rect_ptr, 50, 50);
    check("PtInRect(rect[0,0,100,100], (50,50)) == true", inside, format!("got {inside}"));
    let outside = pt_in_rect(f_ptinrect, rect_ptr, 200, 200);
    check("PtInRect(rect[0,0,100,100], (200,200)) == false", !outside, format!("got {outside}"));

    // ---- regression: baseline all-Int call still runs (dispatch symmetry) ----
    let f_getproc = sym("kernel32.dll", "GetCurrentProcessId");
    let pid = baseline::call_word(f_getproc, 0, &[]);
    check("baseline call_word still works (GetCurrentProcessId)", pid > 0, format!("pid={pid}"));

    println!();
    println!("== Q20 RESULT: {} ==", if all_pass { "ALL PASS" } else { "SOME FAILED" });
    std::process::exit(if all_pass { 0 } else { 1 });
}

// ===========================================================================
// STRUCTURAL-ONLY: the SysV analog of `call_shaped`, gated by
// `#[cfg(target_os = "linux")]` -- same posture as core/kernel.rs's own
// Linux arms when built on this Windows host: written, present in the file,
// but NEVER TYPE-CHECKED here (the cfg strips it before rustc looks at it).
// [结构推断，未编译于本机 — 与全轨 SysV posture 一致，无 WSL 不可执行/不可编码器验证]
//
// The point it demonstrates in source form: Win64's rule above is POSITIONAL
// (arg i -> GPR[i] or XMM[i] purely by index i, independent of other
// positions' kinds). SysV is NOT positional -- it keeps two SEPARATE
// advancing counters, one for the INTEGER class, one for the SSE class, and
// each argument consumes the next register in ITS OWN class's sequence
// regardless of position. Expressing that needs two running indices instead
// of one shared index -- more than zero extra lines, but still a DATA-DRIVEN
// LOOP (no new primitive, no new kernel entry point): see the `int_i`/`flt_i`
// counters below, which is the whole delta versus the Win64 positional
// table-lookup.
// ===========================================================================
#[cfg(target_os = "linux")]
mod sysv_reasoned {
    use super::Kind;

    // SysV integer-class arg registers, in order.
    const IREG: [&str; 6] = ["rdi", "rsi", "rdx", "rcx", "r8", "r9"];
    // SysV SSE-class arg registers, in order.
    const FREG: [&str; 8] = ["xmm0", "xmm1", "xmm2", "xmm3", "xmm4", "xmm5", "xmm6", "xmm7"];

    /// Classify each Word's register by SEPARATE per-class counters (the
    /// SysV rule), vs Win64's shared positional index. Returns the register
    /// NAME chosen for each arg (illustrative -- a real emitter would use
    /// this to pick the mov target, exactly as Win64's `AREG`/`FREG` arrays
    /// would if this file built codegen instead of transmuting to a typed
    /// Rust fn pointer -- SysV has no `extern "sysv64"`-callable analog here
    /// since there is no real Linux libm loaded on this host).
    pub fn sysv_register_plan(sig: &[Kind]) -> Vec<&'static str> {
        let mut int_i = 0usize;
        let mut flt_i = 0usize;
        let mut plan = Vec::with_capacity(sig.len());
        for k in sig {
            match k {
                Kind::Int => {
                    plan.push(IREG[int_i]);
                    int_i += 1;
                }
                Kind::Float => {
                    plan.push(FREG[flt_i]);
                    flt_i += 1;
                }
            }
        }
        plan
    }
    // NOTE the divergence from Win64's `call_shaped` above, worked through by
    // hand for [Float, Int] (ldexp's shape):
    //   Win64 (positional):  arg0(Float)->XMM0 [pos0]   arg1(Int)->RDX [pos1, GPR-list[1]]
    //   SysV  (per-class):   arg0(Float)->XMM0 [flt_i=0] arg1(Int)->RDI [int_i=0, NOT RDX]
    // Same Kind sequence, DIFFERENT register outcome -- confirms candidate
    // 1's flagged risk ("混合排序规则的复杂度") is real, but the fix is a
    // 6-line two-counter loop, not a new primitive or a new value type.
}
