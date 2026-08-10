//! Q23 — Runtime-declared native intents — clean-room probe.
//!
//! Question (from `plan/archive/design-dynacore-native-core.md` §8): can a native
//! intent's calling contract be declared at *pack-load time* instead of baked
//! into the interpreter's source at compile time, while preserving F1-class
//! verification strength — i.e. `verify()` checking the REAL call contract, not
//! just IR-internal self-consistency (the F1 finding, `research/dynamic-core/
//! assembled/RESULTS.md` §②) — so an agent can teach the interpreter a Win32
//! API it did not ship knowing about, without recompiling `agenterm.exe`, and
//! without that becoming a blind-trust hole?
//!
//! CLEAN-ROOM: this file does not import or depend on
//! `crates/agenterm-nativecore/src/*`. It borrows the *shape* of that crate's
//! `verify.rs` (the `IntentArityMismatch` cross-check) and `seam.rs`
//! (`Arg::Sem(k)`, `intent_table`, the uniform `REACH: &[fn(&[i64])->i64]`
//! trampoline array) by having read them, and re-derives a minimal analogue
//! here — exactly the provenance discipline every other Q in this track used.
//!
//! Windows/x86_64 only (the whole track's real-execution host). Build+run:
//!   rustc --edition 2021 -O main.rs -o out/rti.exe && ./out/rti.exe

#![allow(clippy::missing_safety_doc)]

use std::fmt::Write as _;

// ===========================================================================
// The "pack" — a native intent DEFINITION that is DATA, decided at runtime.
//
// In nativecore, an intent is a compile-time `enum Intent` variant plus a
// compile-time `contract_arity()` match arm plus a compile-time `seam.rs`
// binding. Here, ALL THREE come from a text pack parsed at runtime — the
// interpreter binary contains NO mention of `MulDiv`/`lstrlenA`/`GetTickCount`
// anywhere (grep the source: they appear only as strings the parser reads).
// ===========================================================================

/// One argument slot of a native-call recipe (mirrors `seam.rs::Arg`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ArgSrc {
    /// the k-th SEMANTIC arg supplied by the IR call site (a resolved word)
    Sem(usize),
    /// an injected target constant
    Const(i64),
}

/// A native intent declared entirely by the pack.
#[derive(Clone, Debug)]
struct IntentDef {
    name: String,
    /// the exported symbol this intent binds to (resolved at load time via
    /// GetProcAddress — the interpreter did not know this name at compile time)
    symbol: String,
    /// the native-call recipe: one ArgSrc per native argument, in ABI order
    recipe: Vec<ArgSrc>,
    /// the arity the PACK AUTHOR *separately declares* for this intent. This is
    /// the direction-1-"declared" field; it is deliberately allowed to disagree
    /// with the recipe so we can measure whether a separate declaration
    /// re-opens F1 (it does — see S4).
    declared_arity: usize,
}

impl IntentDef {
    /// The contract arity DERIVED from the recipe itself: 1 + the largest
    /// semantic-slot index the recipe actually dereferences (0 if none). This
    /// is the number of semantic args the seam WILL read; deriving it from the
    /// same artifact that does the dereferencing is the whole point of S3/S4.
    fn contract_arity_derived(&self) -> usize {
        self.recipe
            .iter()
            .filter_map(|a| match a {
                ArgSrc::Sem(k) => Some(*k + 1),
                ArgSrc::Const(_) => None,
            })
            .max()
            .unwrap_or(0)
    }
    /// The number of NATIVE args the recipe produces (Q1's L2 leak: semantic
    /// arity ≠ native arity). Distinct from the contract above; kept to
    /// document that the two are different facts even when they coincide here.
    #[allow(dead_code)]
    fn native_arity(&self) -> usize {
        self.recipe.len()
    }
}

/// Parse the pack text format (one intent per line):
///   intent <name> symbol <export> declared <n> args <src>...
/// where each <src> is `sN` (Sem N) or `cN` (Const N). This is a genuine
/// runtime parse: the symbol names arrive as strings, not as compiled knowledge.
fn parse_pack(text: &str) -> Vec<IntentDef> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let t: Vec<&str> = line.split_whitespace().collect();
        // intent NAME symbol SYM declared N args ...
        assert_eq!(t[0], "intent");
        let name = t[1].to_string();
        assert_eq!(t[2], "symbol");
        let symbol = t[3].to_string();
        assert_eq!(t[4], "declared");
        let declared_arity: usize = t[5].parse().unwrap();
        assert_eq!(t[6], "args");
        let recipe = t[7..]
            .iter()
            .map(|s| {
                let (k, rest) = s.split_at(1);
                let v: i64 = rest.parse().unwrap();
                match k {
                    "s" => ArgSrc::Sem(v as usize),
                    "c" => ArgSrc::Const(v),
                    _ => panic!("bad arg src {s}"),
                }
            })
            .collect();
        out.push(IntentDef { name, symbol, recipe, declared_arity });
    }
    out
}

// ===========================================================================
// The "IR call site" — how many semantic args the program passes to an intent.
// (A one-instruction stand-in; the full IR machinery is not what Q23 tests.)
// ===========================================================================
struct IrCall {
    intent: String,
    sem_args: Vec<i64>,
}

// ===========================================================================
// verify() — two variants, the measurable difference of direction 1.
// ===========================================================================
#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    Pass,
    /// the F1 fault: IR's semantic-arg count disagrees with the intent contract
    IntentArityMismatch { provided: usize, contract: usize },
    UnknownIntent,
}

/// DECLARED variant: check IR arg count against the pack's SEPARATE
/// `declared_arity` field. This is "direction 1, naive": trust the author's
/// declaration. It is F1 waiting to happen — the author controls both the IR
/// and the declaration.
fn verify_declared(defs: &[IntentDef], call: &IrCall) -> Verdict {
    let Some(d) = defs.iter().find(|d| d.name == call.intent) else {
        return Verdict::UnknownIntent;
    };
    if call.sem_args.len() != d.declared_arity {
        Verdict::IntentArityMismatch { provided: call.sem_args.len(), contract: d.declared_arity }
    } else {
        Verdict::Pass
    }
}

/// DERIVED variant: check IR arg count against the contract DERIVED FROM THE
/// RECIPE (the same artifact the seam will dereference). This is "direction 1,
/// done right": the contract is not a second author statement, it is computed
/// from the executable recipe, so it cannot silently disagree with what the
/// seam reads. This is the load-time analogue of nativecore's compile-time
/// `contract_arity()` — except the ground truth is the recipe, not a match arm.
fn verify_derived(defs: &[IntentDef], call: &IrCall) -> Verdict {
    let Some(d) = defs.iter().find(|d| d.name == call.intent) else {
        return Verdict::UnknownIntent;
    };
    let contract = d.contract_arity_derived();
    if call.sem_args.len() != contract {
        Verdict::IntentArityMismatch { provided: call.sem_args.len(), contract }
    } else {
        Verdict::Pass
    }
}

// ===========================================================================
// Reach + generic ④ `call` trampoline. Resolves a symbol at load time and
// invokes it with a runtime-decided arg count. This is the mechanism that
// makes "an API the interpreter did not ship knowing about" reachable from
// pure data (primitive ③ reach + ④ call, integer/pointer word subset — the
// libffi model this track's Q6/Q20 established).
// ===========================================================================
#[cfg(windows)]
mod ffi {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn LoadLibraryA(name: *const u8) -> *mut u8;
        pub fn GetProcAddress(module: *mut u8, name: *const u8) -> *const ();
    }
}

#[cfg(windows)]
unsafe fn resolve(dll: &str, symbol: &str) -> Option<*const ()> {
    let dll_c = format!("{dll}\0");
    let sym_c = format!("{symbol}\0");
    unsafe {
        let m = ffi::LoadLibraryA(dll_c.as_ptr());
        if m.is_null() {
            return None;
        }
        let p = ffi::GetProcAddress(m, sym_c.as_ptr());
        if p.is_null() {
            None
        } else {
            Some(p)
        }
    }
}

/// Invoke `f` with `args` using the Windows x64 (`extern "system"`) convention.
/// A genuine ④ trampoline over the integer/pointer subset. NOTE: transmuting to
/// a signature with FEWER params than the real function is exactly how an
/// under-declared arity (S5) produces UB — the callee reads uninitialised
/// arg registers. That is the hole being demonstrated, not relied upon.
#[cfg(windows)]
unsafe fn call_n(f: *const (), args: &[i64]) -> i64 {
    unsafe {
        match args.len() {
            0 => std::mem::transmute::<_, extern "system" fn() -> i64>(f)(),
            1 => std::mem::transmute::<_, extern "system" fn(i64) -> i64>(f)(args[0]),
            2 => std::mem::transmute::<_, extern "system" fn(i64, i64) -> i64>(f)(args[0], args[1]),
            3 => std::mem::transmute::<_, extern "system" fn(i64, i64, i64) -> i64>(f)(args[0], args[1], args[2]),
            4 => std::mem::transmute::<_, extern "system" fn(i64, i64, i64, i64) -> i64>(f)(args[0], args[1], args[2], args[3]),
            n => panic!("trampoline arity {n} out of range"),
        }
    }
}

/// Execute one intent: marshal the recipe into native args, resolve, call.
/// Mirrors `seam.rs::exec_single`. Because a VERIFIED module guarantees
/// `sem_args.len() == contract_arity_derived()`, `sem_args[k]` for any
/// `Sem(k)` in the recipe is in bounds by construction — the memory-safety
/// property F1's fix buys.
#[cfg(windows)]
unsafe fn exec(def: &IntentDef, sem_args: &[i64]) -> Result<i64, String> {
    let native: Vec<i64> = def
        .recipe
        .iter()
        .map(|a| match a {
            ArgSrc::Sem(k) => sem_args[*k],
            ArgSrc::Const(v) => *v,
        })
        .collect();
    let f = unsafe { resolve("kernel32.dll", &def.symbol) }
        .ok_or_else(|| format!("symbol {} unresolved", def.symbol))?;
    let raw = unsafe { call_n(f, &native) };
    // these three demo APIs return a 32-bit int; take the low half.
    Ok((raw as i32) as i64)
}

// ===========================================================================
// Tier-B behavioural probe (direction 2 / Q14). For a CONTRACT-BEARING symbol
// (known-in → known-out via an EXTERNAL oracle), call it and require the result
// match. This catches a mis-BOUND symbol (S6) that arity checks cannot.
// ===========================================================================
#[cfg(windows)]
fn oracle_muldiv(a: i64, b: i64, c: i64) -> i64 {
    // MulDiv rounds (a*b)/c to nearest, .5 away from zero.
    let num = a * b;
    if c == 0 {
        return -1;
    }
    ((num as f64) / (c as f64)).round() as i64
}

fn main() {
    // ------------------------------------------------------------------
    // THE PACK — three real kernel32 exports, NONE of which is among
    // nativecore's seven shipped intents (Alloc/FileOpen/FileRead/FileClose/
    // WriteStdout/SpawnWait/FileWrite). Declared purely as runtime text.
    // ------------------------------------------------------------------
    let honest_pack = "\
# name       symbol      declared  recipe (sN=semantic slot, cN=const)
intent muldiv    symbol MulDiv       declared 3 args s0 s1 s2
intent strlen    symbol lstrlenA     declared 1 args s0
intent ticks     symbol GetTickCount declared 0 args
";

    let defs = parse_pack(honest_pack);
    let mut log = String::new();
    let _ = writeln!(log, "== Q23 runtime-declared native intents — real box ==\n");
    let _ = writeln!(log, "pack parsed at runtime: {} intents, symbols = {:?}",
        defs.len(), defs.iter().map(|d| &d.symbol).collect::<Vec<_>>());
    let _ = writeln!(log, "(the interpreter source contains NONE of these symbol names as code)\n");

    #[cfg(windows)]
    run_windows(&defs, &mut log);
    #[cfg(not(windows))]
    let _ = &defs;

    print!("{log}");
}

#[cfg(windows)]
fn run_windows(defs: &[IntentDef], log: &mut String) {
    // rodata for the string-pointer arg of lstrlenA.
    let hello = b"HELLO\0";
    let hello_ptr = hello.as_ptr() as i64;

    // ---- S1: honest MulDiv, called correctly. Extensibility gate. ----
    {
        let call = IrCall { intent: "muldiv".into(), sem_args: vec![7, 11, 5] };
        let vd = verify_derived(defs, &call);
        let def = defs.iter().find(|d| d.name == "muldiv").unwrap();
        let got = unsafe { exec(def, &call.sem_args) }.unwrap();
        let want = oracle_muldiv(7, 11, 5);
        let _ = writeln!(log, "[S1 extensibility] MulDiv(7,11,5): verify_derived={vd:?}  \
            native_call={got}  oracle={want}  {}",
            if vd == Verdict::Pass && got == want { "PASS — a never-shipped API ran from load-time data" } else { "FAIL" });
    }

    // ---- S2: honest lstrlenA. Contract-bearing pointer arg. ----
    {
        let call = IrCall { intent: "strlen".into(), sem_args: vec![hello_ptr] };
        let vd = verify_derived(defs, &call);
        let def = defs.iter().find(|d| d.name == "strlen").unwrap();
        let got = unsafe { exec(def, &call.sem_args) }.unwrap();
        let _ = writeln!(log, "[S2 extensibility] lstrlenA(\"HELLO\"): verify_derived={vd:?}  \
            native_call={got}  oracle=5  {}",
            if vd == Verdict::Pass && got == 5 { "PASS" } else { "FAIL" });
    }

    // ---- S3: Lie A — internal under-provision. IR passes 2 args to a
    //         recipe that dereferences Sem(0..2) (contract 3). ----
    {
        let call = IrCall { intent: "muldiv".into(), sem_args: vec![7, 11] };
        let vd = verify_derived(defs, &call);
        let caught = matches!(vd, Verdict::IntentArityMismatch { .. });
        let _ = writeln!(log, "\n[S3 F1/Lie-A, derived] MulDiv recipe needs 3, IR passes 2: \
            verify_derived={vd:?}  {}",
            if caught { "CAUGHT before execution — F1 reproduced and closed at LOAD TIME" } else { "MISSED" });
    }

    // ---- S4: the derive-vs-declare finding. A pack that declares arity 2 for
    //         a recipe that dereferences Sem(2). declared-verify passes it
    //         (F1 returns); derived-verify still catches it. ----
    {
        let mut lying = defs.to_vec();
        // muldiv, but the author lies in the SEPARATE declared field: says 2.
        if let Some(d) = lying.iter_mut().find(|d| d.name == "muldiv") {
            d.declared_arity = 2;
        }
        let call = IrCall { intent: "muldiv".into(), sem_args: vec![7, 11] };
        let v_decl = verify_declared(&lying, &call);
        let v_der = verify_derived(&lying, &call);
        let _ = writeln!(log, "[S4 derive-vs-declare] recipe derefs Sem(2) but author declares arity 2, IR passes 2:");
        let _ = writeln!(log, "        verify_DECLARED = {v_decl:?}  -> {}",
            if v_decl == Verdict::Pass { "PASS = F1 REOPENED (seam would read Sem(2) OOB at runtime)" } else { "caught" });
        let _ = writeln!(log, "        verify_DERIVED  = {v_der:?}  -> {}",
            if matches!(v_der, Verdict::IntentArityMismatch{..}) { "CAUGHT (contract derived from recipe = 3 != 2)" } else { "missed" });
    }

    // ---- S5: Lie B — ABI under-declaration of a REAL symbol. Pack declares
    //         MulDiv as a 1-arg function (recipe [Sem0]), IR passes 1 arg.
    //         Internally consistent => derived-verify PASSES => the trampoline
    //         calls the real 3-arg MulDiv with 1 arg => UB / wrong answer. ----
    {
        let lie = IntentDef {
            name: "muldiv1".into(),
            symbol: "MulDiv".into(),
            recipe: vec![ArgSrc::Sem(0)],
            declared_arity: 1,
        };
        let mut pack = defs.to_vec();
        pack.push(lie);
        let call = IrCall { intent: "muldiv1".into(), sem_args: vec![7] };
        let vd = verify_derived(&pack, &call);
        let def = pack.iter().find(|d| d.name == "muldiv1").unwrap();
        let got = unsafe { exec(def, &call.sem_args) }.unwrap();
        let true_want = oracle_muldiv(7, 11, 5); // what MulDiv(7,11,5) SHOULD be
        let _ = writeln!(log, "\n[S5 Lie-B, ABI under-declaration] pack declares MulDiv as 1-arg, IR passes 1:");
        let _ = writeln!(log, "        verify_derived = {vd:?}  -> {}",
            if vd == Verdict::Pass { "PASS (recipe is internally consistent — NO machine check fires)" } else { "caught" });
        let _ = writeln!(log, "        native MulDiv(7,<garbage RDX>,<garbage R8>) returned {got} \
            (a true 3-arg call would be {true_want}); result is UB from an under-declared ABI —");
        let _ = writeln!(log, "        NOTHING in the single-author pack lets verify() know MulDiv's TRUE arity is 3.");
    }

    // ---- S6: direction 2 (Q14) recovers a NAMING lie for a contract-bearing
    //         symbol. Pack claims intent "len" is strlen but binds lstrcmpiA.
    //         Arity matches (both look 1-arg-ish here); only a BEHAVIOURAL
    //         probe against the external oracle catches it. ----
    {
        let misbound = IntentDef {
            name: "len".into(),
            symbol: "lstrcmpiA".into(), // WRONG: not strlen at all
            recipe: vec![ArgSrc::Sem(0)],
            declared_arity: 1,
        };
        // Behavioural probe: strlen("HELLO") MUST be 5 (external oracle). The
        // mis-bound lstrcmpiA(ptr) called with one arg returns something else.
        let got = unsafe { exec(&misbound, &[hello_ptr]) }.unwrap();
        let fired = got != 5;
        let _ = writeln!(log, "\n[S6 direction-2/Q14 naming probe] intent 'len' binds lstrcmpiA but claims strlen:");
        let _ = writeln!(log, "        arity check: PASS (1==1) — arity cannot see a naming lie");
        let _ = writeln!(log, "        Tier-B probe strlen(\"HELLO\")==5 : got {got} -> {}",
            if fired { "FIRE (behavioural probe caught the mis-bind — Q14, contract-bearing)" } else { "missed" });
        let _ = writeln!(log, "        residue: a NO-contract symbol (e.g. OutputDebugStringA) has no oracle -> unprobeable (Q14 residue).");
    }
}
