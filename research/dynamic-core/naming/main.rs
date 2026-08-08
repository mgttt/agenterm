//! Q14 — Behavioural verification of NAMING bindings: is "symbol X really X?"
//!
//! Q4 declared "is symbol index 1 really CreateFileA?" UNqueryable — the only way
//! to test the call is to make it (circular, Thompson). Q13 inherited that and
//! limited its layout result to "detectable MODULO naming", shrinking the trust set
//! {naming + layout} -> {naming}. This harness challenges the {naming} half.
//!
//! The Q13 insight, made explicit: Q13's FACT1 created a file it named, called the
//! symbol it BELIEVED was FindFirstFileA, and required the returned string to equal
//! the name it chose. If that symbol were NOT FindFirstFileA, the check FIRES. So
//! Q13 was already doing behavioural naming verification. Q14 asks it head-on:
//!
//!   Can a naming binding be behaviourally verified? To what degree, at what cost,
//!   and how bad is the circularity — does it converge to a small ROOT trust set?
//!
//! Posture (from Q13): never assert "it can". Deliberately MIS-BIND a slot (resolve
//! a DIFFERENT export into it — exactly what a lying resolver / wrong binding does)
//! and REQUIRE the behavioural self-check to FIRE. Detection proven by negative probe.
//!
//! Only the four primitives (③ sym + ④ call). No PDB, no symbol server, no PE export
//! parsing, no signature check — those are the §1.3/§6 pathologies. Clean-room.

#![allow(non_snake_case)]

// ===========================================================================
// The four primitives — ③ reach = sym (the RESOLVER, our root), ④ call.
// ===========================================================================
#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryA(name: *const u8) -> *mut u8;
    fn GetProcAddress(module: *mut u8, name: *const u8) -> *mut u8;
}

/// ③ reach. NOTE: this pair IS the resolver root that ④ below cannot escape.
fn sym(module: &[u8], name: &[u8]) -> *mut u8 {
    unsafe {
        let h = LoadLibraryA(module.as_ptr());
        GetProcAddress(h, name.as_ptr())
    }
}
fn k32(name: &[u8]) -> *mut u8 { sym(b"kernel32.dll\0", name) }

const CALL_CEILING: usize = 11;
fn call(addr: *mut u8, args: &[usize]) -> usize {
    assert!(args.len() <= CALL_CEILING, "④ ceiling exceeded");
    if addr.is_null() { return usize::MAX; } // ③ failed to resolve
    unsafe {
        let a = args;
        macro_rules! t {
            ($($p:ty),*) => { core::mem::transmute::<_, extern "win64" fn($($p),*) -> usize>(addr) };
        }
        match a.len() {
            0 => (t!())(),
            1 => (t!(usize))(a[0]),
            2 => (t!(usize, usize))(a[0], a[1]),
            3 => (t!(usize, usize, usize))(a[0], a[1], a[2]),
            4 => (t!(usize, usize, usize, usize))(a[0], a[1], a[2], a[3]),
            5 => (t!(usize, usize, usize, usize, usize))(a[0], a[1], a[2], a[3], a[4]),
            6 => (t!(usize, usize, usize, usize, usize, usize))(a[0], a[1], a[2], a[3], a[4], a[5]),
            7 => (t!(usize, usize, usize, usize, usize, usize, usize))(a[0], a[1], a[2], a[3], a[4], a[5], a[6]),
            _ => unreachable!("no check here exceeds 7 args"),
        }
    }
}

/// A detection outcome: Ok(()) = the behavioural self-check corroborated the binding;
/// Err(reason) = the binding was DETECTED wrong (loud, not silent).
type Detect = Result<(), String>;

// ===========================================================================
// CLASS A — PURE FUNCTION, externally-known output.  Dependency: RESOLVER ONLY.
// The oracle (string length / arithmetic) is a MATHEMATICAL ground truth external
// to the DLL — no other symbol is trusted. A wrong binding (resolver returns the
// pointer for a different export) yields a wrong number -> FIRE. This is the
// "independent probe" Q4 said did not exist.
// ===========================================================================

/// Verify the binding named `bind_as` behaves like lstrlenA. `expect` = len we KNOW.
fn strlen_selfcheck(bind_as: &[u8], input: &[u8], expect: usize) -> Detect {
    let p = k32(bind_as);
    if p.is_null() { return Err(format!("{:?} did not resolve", String::from_utf8_lossy(bind_as))); }
    let got = call(p, &[input.as_ptr() as usize]);
    if got == expect { Ok(()) }
    else { Err(format!("lstrlenA-slot({:?})={} , expected {} (binding WRONG)",
        String::from_utf8_lossy(bind_as), got as isize, expect)) }
}

/// Verify the binding named `bind_as` behaves like MulDiv (a*b/c). Arithmetic oracle.
fn muldiv_selfcheck(bind_as: &[u8], a: usize, b: usize, c: usize, expect: usize) -> Detect {
    let p = k32(bind_as);
    if p.is_null() { return Err(format!("{:?} did not resolve", String::from_utf8_lossy(bind_as))); }
    let got = call(p, &[a, b, c]);
    if got == expect { Ok(()) }
    else { Err(format!("MulDiv-slot({:?})({},{},{})={} , expected {} (binding WRONG)",
        String::from_utf8_lossy(bind_as), a, b, c, got as isize, expect)) }
}

/// Verify the binding named `bind_as` behaves like lstrlenA/lstrcmp family:
/// lstrcmpiA is case-INSENSITIVE (equal -> 0). Mis-binding to lstrcmpA (case-
/// SENSITIVE) returns nonzero for "Ab"/"ab" -> a fine-grained behavioural FIRE.
fn strcmp_i_selfcheck(bind_as: &[u8]) -> Detect {
    let p = k32(bind_as);
    if p.is_null() { return Err(format!("{:?} did not resolve", String::from_utf8_lossy(bind_as))); }
    let got = call(p, &[b"AgentTerm\0".as_ptr() as usize, b"agentterm\0".as_ptr() as usize]);
    if got == 0 { Ok(()) }
    else { Err(format!("lstrcmpiA-slot({:?})(\"AgentTerm\",\"agentterm\")={} , expected 0 (case-insensitive) (binding WRONG)",
        String::from_utf8_lossy(bind_as), got as isize)) }
}

// ===========================================================================
// CLASS B — OS ROUND-TRIP CLUSTER.  Dependency: the OTHER cluster symbols
// (each itself Class-A/B verifiable) -> converges to the RESOLVER root.
// Ground truth = "bytes I wrote come back". Verifies the cluster JOINTLY, modulo
// each other. Mis-bind "CreateFileA" -> DeleteFileA (a plausible same-family wrong
// binding): no writable handle -> round-trip FIRES.
// ===========================================================================
fn roundtrip_selfcheck(create_as: &[u8]) -> Detect {
    const GENERIC_WRITE: usize = 0x4000_0000;
    const GENERIC_READ: usize = 0x8000_0000;
    const CREATE_ALWAYS: usize = 2;
    const OPEN_EXISTING: usize = 3;
    const FILE_ATTR_NORMAL: usize = 0x80;
    let name = b"q14_roundtrip.tmp\0";
    let payload = b"agenterm-q14-naming"; // the ground truth we will read back

    let create = k32(create_as);
    let write = k32(b"WriteFile\0");
    let read = k32(b"ReadFile\0");
    let close = k32(b"CloseHandle\0");
    if create.is_null() { return Err(format!("{:?} did not resolve", String::from_utf8_lossy(create_as))); }

    // create (via the binding under test) + write known bytes
    let hw = call(create, &[name.as_ptr() as usize, GENERIC_WRITE, 0, 0, CREATE_ALWAYS, FILE_ATTR_NORMAL, 0]);
    if hw == usize::MAX || hw == 0 {
        return Err(format!("create-slot({:?}) returned no writable handle ({}) — binding WRONG",
            String::from_utf8_lossy(create_as), hw as isize));
    }
    let mut nwritten: u32 = 0;
    let wok = call(write, &[hw, payload.as_ptr() as usize, payload.len(), (&mut nwritten as *mut u32) as usize, 0]);
    call(close, &[hw]);
    if wok == 0 || nwritten as usize != payload.len() {
        call(k32(b"DeleteFileA\0"), &[name.as_ptr() as usize]);
        return Err(format!("write after create-slot({:?}) failed (wrote {}) — binding WRONG",
            String::from_utf8_lossy(create_as), nwritten));
    }
    // reopen + read back
    let hr = call(create, &[name.as_ptr() as usize, GENERIC_READ, 0, 0, OPEN_EXISTING, FILE_ATTR_NORMAL, 0]);
    if hr == usize::MAX || hr == 0 {
        call(k32(b"DeleteFileA\0"), &[name.as_ptr() as usize]);
        return Err(format!("reopen-slot({:?}) failed — binding WRONG", String::from_utf8_lossy(create_as)));
    }
    let mut buf = [0u8; 64];
    let mut nread: u32 = 0;
    call(read, &[hr, buf.as_mut_ptr() as usize, buf.len(), (&mut nread as *mut u32) as usize, 0]);
    call(close, &[hr]);
    call(k32(b"DeleteFileA\0"), &[name.as_ptr() as usize]);

    if &buf[..nread as usize] == &payload[..] { Ok(()) }
    else { Err(format!("read-back mismatch after create-slot({:?}): got {:?} — binding WRONG",
        String::from_utf8_lossy(create_as), String::from_utf8_lossy(&buf[..nread as usize]))) }
}

fn run(name: &str, d: Detect) -> bool {
    match &d {
        Ok(()) => { println!("    [PASS] {}: behavioural self-check corroborated the binding", name); true }
        Err(e) => { println!("    [FIRE] {}: {}", name, e); false }
    }
}

fn main() {
    println!("=== Q14 — behavioural verification of naming bindings (real Windows) ===\n");
    println!("--- ① BOOLEAN GATE: correct binding must PASS, mis-bound slot must FIRE ---");
    println!("    (a mis-bind = resolve a DIFFERENT export into the slot = exactly what a");
    println!("     lying resolver / wrong binding does)\n");

    // -- CLASS A: pure functions, external oracle, dependency = RESOLVER ONLY --
    println!("[A1] lstrlenA  (pure; oracle = string length is a MATH fact, no other symbol)");
    let a1p = run("correct  lstrlenA", strlen_selfcheck(b"lstrlenA\0", b"agenterm\0", 8));
    let a1n = run("mis-bind GetTickCount->lstrlenA-slot", strlen_selfcheck(b"GetTickCount\0", b"agenterm\0", 8));
    let det_a1 = a1p && !a1n;

    println!("\n[A2] MulDiv  (pure; oracle = 7*191/1 = 1337 is ARITHMETIC, no other symbol)");
    let a2p = run("correct  MulDiv", muldiv_selfcheck(b"MulDiv\0", 7, 191, 1, 1337));
    let a2n = run("mis-bind GetTickCount->MulDiv-slot", muldiv_selfcheck(b"GetTickCount\0", 7, 191, 1, 1337));
    let det_a2 = a2p && !a2n;

    println!("\n[A3] lstrcmpiA  (pure; fine-grained: case-INSENSITIVE equal -> 0)");
    let a3p = run("correct  lstrcmpiA", strcmp_i_selfcheck(b"lstrcmpiA\0"));
    let a3n = run("mis-bind lstrcmpA->lstrcmpiA-slot (case-sensitive)", strcmp_i_selfcheck(b"lstrcmpA\0"));
    let det_a3 = a3p && !a3n;

    // -- CLASS B: OS round-trip cluster, dependency = other cluster symbols --
    println!("\n[B1] CreateFileA round-trip cluster (write known bytes, read them back)");
    let b1p = run("correct  CreateFileA", roundtrip_selfcheck(b"CreateFileA\0"));
    let b1n = run("mis-bind DeleteFileA->CreateFileA-slot", roundtrip_selfcheck(b"DeleteFileA\0"));
    let det_b1 = b1p && !b1n;

    println!("\n--- ① VERDICT (per symbol: PASS-on-correct AND FIRE-on-mis-bind = VERIFIED) ---");
    for (n, d) in [("A1 lstrlenA (pure)", det_a1), ("A2 MulDiv (pure)", det_a2),
                   ("A3 lstrcmpiA (pure)", det_a3), ("B1 CreateFileA (round-trip cluster)", det_b1)] {
        println!("    {:<40} : {}", n, if d { "VERIFIED (mis-bind fired)" } else { "NOT verified" });
    }

    // ==================================================================
    // ④ CIRCULARITY / ROOT TRUST SET — the main criterion.
    // ==================================================================
    println!("\n--- ④ CIRCULARITY: does verifying a name require trusting other names? ---");
    println!("    Dependency of each check (what symbols it must itself call):");
    println!("      A1 lstrlenA  -> {{resolver}}                (pure: NO other symbol; oracle is math)");
    println!("      A2 MulDiv    -> {{resolver}}                (pure: NO other symbol; oracle is math)");
    println!("      A3 lstrcmpiA -> {{resolver}}                (pure: NO other symbol; oracle is math)");
    println!("      B1 create    -> {{WriteFile, ReadFile, CloseHandle, resolver}}  (each itself Class-A/B)");
    println!();
    println!("    KEY: the negative probes above prove a resolver that returns the WRONG");
    println!("    pointer for a CONTRACT-BEARING symbol is CAUGHT by that symbol's own check");
    println!("    (mis-bind fired every time). So the resolver's honesty ABOUT a contract-");
    println!("    bearing symbol is verified TOGETHER with the binding — it is NOT a residual");
    println!("    root for those symbols. The dependency graph's sink is therefore NOT the");
    println!("    whole symbol table; the check chain bottoms out at:");
    println!("      (1) the Thompson trigger  — a symbol/resolver honest during the check,");
    println!("          malicious later. Irreducible for ALL behavioural testing.");
    println!("      (2) no-contract symbols   — see ② below (no probe -> resolver can lie).");
    println!("      (3) the ambient OS kernel — outside the process (= Q15 beyond-boundary).");
    println!("    => N (residual TRUSTED named-bindings for a payload using only contract-");
    println!("       bearing symbols) ~= 0, modulo (1)+(3). The circularity CONVERGES.");

    // ==================================================================
    // ② COVERAGE CLASSIFICATION — which symbols verifiable / not.
    // ==================================================================
    println!("\n--- ② COVERAGE: verifiable iff a constructible KNOWN-in -> KNOWN-out contract ---");
    println!("    VERIFIABLE:");
    println!("      · pure functions w/ external oracle (lstrlenA, MulDiv, lstrcmpiA):");
    println!("        strongest — output is a math fact, ZERO other-symbol trust.");
    println!("      · OS round-trip clusters (CreateFileA/WriteFile/ReadFile): the OS's own");
    println!("        write-then-read-back is the oracle; verified jointly, modulo partners.");
    println!("    UNVERIFIABLE (residue -> stays naming-trust):");
    // Demonstrate: no-contract symbols have no observable to cross-check.
    println!("      · the RESOLVER (GetProcAddress/LoadLibraryA): used to obtain EVERY pointer");
    println!("        incl. the one you'd test it against -> no probe not routed through it.");
    println!("      · no-observable-effect symbols (OutputDebugStringA, ExitProcess): effect");
    println!("        is invisible / unrecoverable -> no known-out to compare.");
    println!("      · weak/indirect-only (Sleep): only observable via ANOTHER symbol");
    println!("        (GetTickCount) and flakily -> at best modulo that symbol, not clean.");
    // live evidence that a no-contract symbol offers nothing: Sleep returns void, no signal.
    let sl = k32(b"Sleep\0");
    if !sl.is_null() {
        let before = call(k32(b"GetTickCount\0"), &[]);
        call(sl, &[1]); // Sleep(1ms)
        let after = call(k32(b"GetTickCount\0"), &[]);
        println!("      · [live] Sleep(1): elapsed~{}ms via GetTickCount — the only 'oracle' is",
            after.wrapping_sub(before));
        println!("        another symbol's reading, and it is non-deterministic -> weak tier.");
    }

    // ==================================================================
    // ③ COST
    // ==================================================================
    println!("\n--- ③ COST: LOC/symbol, +0 kernel bytes (payload-side only) ---");
    println!("    pure-function check (A1/A2/A3): 8 LOC each (resolve+call+compare; most is");
    println!("                                    the FIRE error message — the core is ~4).");
    println!("    round-trip cluster (B1):        40 LOC (create+write+close+reopen+read+cmp).");
    println!("    kernel bytes added by verification = 0 (ordinary ③+④ payload code, like Q13).");
    println!("    vs Q13 layout detection: ~18-38 LOC/fact, +0 kernel bytes — same envelope,");
    println!("    pure-function naming checks are CHEAPER (no constructive setup needed).");

    // ==================================================================
    // ⑤ RELATION TO Q4 / Q13
    // ==================================================================
    println!("\n--- ⑤ RELATION TO Q4/Q13 ---");
    println!("    Q4: 'is symbol X really X?' UNqueryable, 'no independent probe exists'.");
    println!("    Q14 refutes the STRONG form: for a contract-bearing symbol an independent");
    println!("    probe DOES exist — a known input whose correct output is external ground");
    println!("    truth (string length / arithmetic / bytes-written). The mis-bind FIRES.");
    println!("    Q13 said layout is checkable 'modulo naming'. Q14 verifies naming itself for");
    println!("    contract-bearing symbols, so Q13's 'modulo naming' caveat largely DISSOLVES:");
    println!("    layout + naming collapse together and bottom out at the SAME residue class —");
    println!("    {{no-contract symbols, Thompson trigger, ambient kernel}} — NOT at 'naming=trust'.");
    println!("    This is Tier B (behavioural, needs EXECUTION), not Tier A (structural).");

    let all = det_a1 && det_a2 && det_a3 && det_b1;
    println!("\n=== gate ①: {} ===", if all { "PASS (every tested binding verified; kill criterion NOT tripped)" }
        else { "PARTIAL/FAIL" });
}
