//! Q13 — Declare without a host layout oracle: bake-and-pray vs bake-and-DETECT.
//!
//! Q6 proved that on Windows (no BTF) every struct-field offset a capability needs
//! enters as a BAKED constant = unverified per-target trust, and a wrong bake fails
//! SILENTLY (reads/writes an adjacent field, proceeds, corrupts downstream). This
//! harness asks the Q13 question on the real box:
//!
//!   Can a wrong layout assumption be DETECTED (fail-fast) instead of silently crashing?
//!
//! For each of Q6's three baked layout facts we (a) run the capability with the
//! CORRECT offset and a ③+④-only self-check that must PASS, then (b) CORRUPT the
//! offset and confirm the self-check FIRES. Detection is thus proven by a negative
//! probe, never asserted. Only the four primitives (sym+call) are used — no new
//! kernel semantic, no PDB, no symbol server (that is the §1.2 pathology).
//!
//! Clean-room: sym/call mirror `core/kernel.rs`'s contract, rewritten for a std
//! harness; no external implementation consulted.

#![allow(non_snake_case)]

// ===========================================================================
// The four primitives — Win64 (③ reach = sym; ④ call = data-driven native call).
// ===========================================================================
#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryA(name: *const u8) -> *mut u8;
    fn GetProcAddress(module: *mut u8, name: *const u8) -> *mut u8;
}

fn sym(module: &[u8], name: &[u8]) -> *mut u8 {
    unsafe {
        let h = LoadLibraryA(module.as_ptr());
        GetProcAddress(h, name.as_ptr())
    }
}

const CALL_CEILING: usize = 11;

fn call(addr: *mut u8, args: &[usize]) -> usize {
    assert!(args.len() <= CALL_CEILING, "④ ceiling exceeded");
    assert!(!addr.is_null(), "④ null target (③ failed to resolve)");
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
            _ => unreachable!("no capability here exceeds 7"),
        }
    }
}
fn k32(name: &[u8]) -> *mut u8 { sym(b"kernel32.dll\0", name) }
fn ws2(name: &[u8]) -> *mut u8 { sym(b"ws2_32.dll\0", name) }

/// A detection outcome: Ok(()) = self-check corroborated the layout assumption;
/// Err(reason) = the assumption was DETECTED wrong (loud, not silent).
type Detect = Result<(), String>;

// ===========================================================================
// FACT 1 — WIN32_FIND_DATAA.cFileName  (READ).  Baked offset 44.
// Self-check is CONSTRUCTIVE: we create a file whose name WE choose, ask
// FindFirstFile for exactly it, and require the string at the assumed offset to
// equal the name we picked. Known input -> known output -> the offset is verified.
// ===========================================================================
const WIN32_FIND_DATAA_SIZE: usize = 320;
const OFF_cFileName_TRUE: usize = 44;

fn make_probe_file(name: &[u8]) -> bool {
    // CreateFileA(name, GENERIC_WRITE, 0, NULL, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL)
    let h = call(k32(b"CreateFileA\0"), &[
        name.as_ptr() as usize, 0x4000_0000, 0, 0, 2, 0x80, 0,
    ]);
    if h == usize::MAX || h == 0 { return false; }
    call(k32(b"CloseHandle\0"), &[h]);
    true
}
fn delete_probe_file(name: &[u8]) {
    call(k32(b"DeleteFileA\0"), &[name.as_ptr() as usize]);
}

fn dir_selfcheck(off_cFileName: usize, expect: &[u8]) -> Detect {
    let mut data = [0u8; WIN32_FIND_DATAA_SIZE];
    let hfind = call(k32(b"FindFirstFileA\0"),
        &[expect.as_ptr() as usize, data.as_mut_ptr() as usize]);
    if hfind == usize::MAX { return Err("FindFirstFileA failed".into()); }
    // Read the NUL-terminated name at the ASSUMED offset.
    let got = {
        let mut s = Vec::new();
        let mut i = 0;
        unsafe {
            while i < 260 && *data.as_ptr().add(off_cFileName + i) != 0 {
                s.push(*data.as_ptr().add(off_cFileName + i));
                i += 1;
            }
        }
        s
    };
    call(k32(b"FindClose\0"), &[hfind]);
    let want = &expect[..expect.len() - 1]; // drop NUL
    if got == want {
        Ok(())
    } else {
        Err(format!("cFileName@{} = {:?}, expected {:?} (offset assumption WRONG)",
            off_cFileName, String::from_utf8_lossy(&got), String::from_utf8_lossy(want)))
    }
}

// ===========================================================================
// FACT 2 — sockaddr_in.sin_family@0 / sin_addr@4  (WRITE).  Baked offsets.
// Self-check is a ROUND-TRIP: write fields at the assumed offsets, bind to
// 127.0.0.1:0, then getsockname reads the OS's own view back. If the family
// offset is wrong, bind rejects it (WSAEAFNOSUPPORT); if the addr offset is
// wrong, the bound address read back != 127.0.0.1. Either way -> DETECTED.
// ===========================================================================
const OFF_sin_family_TRUE: usize = 0;
const OFF_sin_addr_TRUE: usize = 4;
const SOCKADDR_IN_SIZE: usize = 16;

fn wsa_last_error() -> usize { call(ws2(b"WSAGetLastError\0"), &[]) }

fn socket_selfcheck(off_family: usize, off_addr: usize) -> Detect {
    const AF_INET: usize = 2;
    const SOCK_DGRAM: usize = 2;
    const IPPROTO_UDP: usize = 17;

    let mut wsadata = [0u8; 512];
    if call(ws2(b"WSAStartup\0"), &[0x0202, wsadata.as_mut_ptr() as usize]) != 0 {
        return Err("WSAStartup failed".into());
    }
    let s = call(ws2(b"socket\0"), &[AF_INET, SOCK_DGRAM, IPPROTO_UDP]);
    let mut done = |r: Detect| -> Detect { call(ws2(b"closesocket\0"), &[s]); call(ws2(b"WSACleanup\0"), &[]); r };
    if s == usize::MAX { return done(Err("socket failed".into())); }

    // Build sockaddr_in at the ASSUMED offsets.
    let mut sa = [0u8; SOCKADDR_IN_SIZE];
    sa[off_family] = AF_INET as u8;      // sin_family (LE u16)
    sa[off_family + 1] = 0;
    // sin_port left 0 (ephemeral); network-order 0 is order-agnostic.
    sa[off_addr] = 127;                  // 127.0.0.1 in network order
    sa[off_addr + 1] = 0;
    sa[off_addr + 2] = 0;
    sa[off_addr + 3] = 1;

    let b = call(ws2(b"bind\0"), &[s, sa.as_ptr() as usize, SOCKADDR_IN_SIZE]);
    if b != 0 {
        let e = wsa_last_error();
        // WSAEAFNOSUPPORT = 10047: the family byte was not where we put it.
        return done(Err(format!("bind failed (WSAGetLastError={}) — family/addr offset WRONG", e)));
    }
    // Round-trip: getsockname writes the OS's own sockaddr_in back.
    let mut sa2 = [0u8; SOCKADDR_IN_SIZE];
    let mut len: i32 = SOCKADDR_IN_SIZE as i32;
    let g = call(ws2(b"getsockname\0"),
        &[s, sa2.as_mut_ptr() as usize, (&mut len as *mut i32) as usize]);
    if g != 0 { return done(Err("getsockname failed".into())); }
    // The OS uses the TRUE layout when it writes back; verify against the contract.
    let fam = u16::from_le_bytes([sa2[OFF_sin_family_TRUE], sa2[OFF_sin_family_TRUE + 1]]);
    let addr = [sa2[OFF_sin_addr_TRUE], sa2[OFF_sin_addr_TRUE + 1], sa2[OFF_sin_addr_TRUE + 2], sa2[OFF_sin_addr_TRUE + 3]];
    if fam != AF_INET as u16 {
        return done(Err(format!("bound family read back = {} != AF_INET", fam)));
    }
    if addr != [127, 0, 0, 1] {
        return done(Err(format!("bound addr read back = {:?} != 127.0.0.1 — addr offset WRONG", addr)));
    }
    done(Ok(()))
}

// ===========================================================================
// FACT 3 — SYSTEM_INFO.dwPageSize@4  (READ).  Baked offset.
// Self-check is CROSS-FIELD consistency: GetSystemInfo also fills
// wProcessorArchitecture@0 (==9 AMD64 on this box) and dwAllocationGranularity@28
// (==65536). If the struct base/offset assumption drifts, @0 is no longer 9 and
// the value read as "page size" is not a plausible power-of-two page size.
// ===========================================================================
const OFF_dwPageSize_TRUE: usize = 4;
const OFF_wProcessorArch: usize = 0;
// NB: x64 layout. Pointers/DWORD_PTR are 8 bytes, so dwAllocationGranularity is
// @40 (it is @28 in the 32-bit layout). Baking the 32-bit offset here on x64 is
// exactly the silent-drift bug this experiment studies — and the cross-field
// self-check caught it on the first run. See RESULTS ④ (weak/cross-field tier).
const OFF_dwAllocGranularity: usize = 40;
const OFF_dwNumberOfProcessors: usize = 32;

fn read_u16(b: &[u8], o: usize) -> u16 { u16::from_le_bytes([b[o], b[o + 1]]) }
fn read_u32(b: &[u8], o: usize) -> u32 { u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) }

fn pagesize_selfcheck(off_pagesize: usize) -> Detect {
    let mut si = [0u8; 48];
    call(k32(b"GetSystemInfo\0"), &[si.as_mut_ptr() as usize]);
    let arch = read_u16(&si, OFF_wProcessorArch);
    let gran = read_u32(&si, OFF_dwAllocGranularity);
    let page = read_u32(&si, off_pagesize);
    // Anchor: struct base is right iff arch == 9 (AMD64) and granularity == 65536.
    if arch != 9 {
        return Err(format!("wProcessorArchitecture@0 = {} != 9 (AMD64) — struct base WRONG", arch));
    }
    let plausible = page >= 4096 && page <= 65536 && (page & (page - 1)) == 0;
    if !plausible {
        return Err(format!("dwPageSize@{} = {} — not a plausible power-of-two page size (offset WRONG)", off_pagesize, page));
    }
    if gran != 65536 {
        return Err(format!("dwAllocationGranularity@28 = {} != 65536 (cross-field inconsistent)", gran));
    }
    Ok(())
}

// ===========================================================================
// ① Host-oracle probe: what does Windows PUBLISH at runtime?
// ===========================================================================
fn host_oracle_probe() {
    let mut si = [0u8; 48];
    call(k32(b"GetSystemInfo\0"), &[si.as_mut_ptr() as usize]);
    println!("    GetSystemInfo publishes VALUES (machine facts):");
    println!("      wProcessorArchitecture = {}", read_u16(&si, 0));
    println!("      dwPageSize             = {}", read_u32(&si, 4));
    println!("      dwAllocationGranularity= {}", read_u32(&si, OFF_dwAllocGranularity));
    println!("      dwNumberOfProcessors   = {}", read_u32(&si, OFF_dwNumberOfProcessors));
    println!("    -> the VALUES are host-published, but each is gated behind a struct");
    println!("       OFFSET (@0/@4/@32/@40) that the host does NOT publish. The oracle's");
    println!("       own answer is behind a layout you must bake: a bootstrap circularity.");
    println!("    struct-field-offset oracle at runtime (offsetof)? NONE — would need PDB");
    println!("       (dbghelp/symbol server) = the §1.2 pathology, out of scope by design.");
}

fn run(name: &str, d: Detect) -> bool {
    match &d {
        Ok(()) => { println!("    [PASS] {}: self-check corroborated the offset", name); true }
        Err(e) => { println!("    [FIRE] {}: {}", name, e); false }
    }
}

fn main() {
    println!("=== Q13 — Declare detection on real Windows (no layout oracle) ===\n");

    // Constructive probe file for FACT 1.
    let probe = b"q13_marker.tmp\0";
    let made = make_probe_file(probe);

    println!("--- ② DETECTION: correct offset must PASS, corrupted offset must FIRE ---\n");

    // FACT 1 — dir cFileName@44 (READ)
    println!("[1] WIN32_FIND_DATAA.cFileName  (READ, baked @44)");
    let p1 = if made { run("correct @44", dir_selfcheck(OFF_cFileName_TRUE, probe)) } else {
        println!("    (probe file not created; skipping)"); false };
    // Corrupt: shift the assumed offset by +8 (a realistic small field drift).
    let c1 = run("corrupted @52", dir_selfcheck(52, probe));
    let detect1 = p1 && c1 == false;

    // FACT 2 — socket sin_family@0 / sin_addr@4 (WRITE)
    println!("\n[2] sockaddr_in.sin_family@0 / sin_addr@4  (WRITE, round-trip)");
    let p2 = run("correct @0/@4", socket_selfcheck(OFF_sin_family_TRUE, OFF_sin_addr_TRUE));
    // Corrupt the family offset: put AF_INET at @8 instead of @0.
    let c2 = run("corrupted family@8", socket_selfcheck(8, OFF_sin_addr_TRUE));
    let detect2 = p2 && c2 == false;

    // FACT 3 — SYSTEM_INFO.dwPageSize@4 (READ)
    println!("\n[3] SYSTEM_INFO.dwPageSize  (READ, baked @4, cross-field)");
    let p3 = run("correct @4", pagesize_selfcheck(OFF_dwPageSize_TRUE));
    // Corrupt: read "page size" at @0 (the arch union) instead of @4.
    let c3 = run("corrupted @0", pagesize_selfcheck(0));
    let detect3 = p3 && c3 == false;

    if made { delete_probe_file(probe); }

    println!("\n--- ② VERDICT (per fact: PASS-on-correct AND FIRE-on-corrupt = DETECTED) ---");
    println!("    FACT 1 cFileName (READ, constructive)  : {}", if detect1 { "DETECTED" } else { "not detected" });
    println!("    FACT 2 sockaddr_in (WRITE, round-trip) : {}", if detect2 { "DETECTED" } else { "not detected" });
    println!("    FACT 3 dwPageSize (READ, cross-field)  : {}", if detect3 { "DETECTED" } else { "not detected" });

    println!("\n--- ① HOST ORACLE ---");
    host_oracle_probe();

    println!("\n--- ④ COVERAGE BOUNDARY (which facts are detection-IMPOSSIBLE) ---");
    println!("    Detectable  = a Win32 API exists whose KNOWN-input->KNOWN-output (or the OS's");
    println!("                  own round-trip write-back) touches the field, letting a wrong");
    println!("                  offset produce an observable value mismatch. All 3 facts qualify.");
    println!("    Undetectable (permanent bake-and-pray residue) = a field with NO such API:");
    println!("      · WRITE-only input fields the OS consumes internally with no round-trip and");
    println!("        no validation (e.g. an opaque flags/reserved field bind/CreateProcess ignore)");
    println!("      · READ fields whose value is UNPREDICTABLE and round-tripped by nothing");
    println!("        (e.g. WIN32_FIND_DATAA.ftCreationTime — you cannot pre-know the FILETIME)");
    println!("      · pure size/`cb` fields the OS only loosely validates (STARTUPINFOA.cb): a");
    println!("        wrong-but-in-range size may be silently tolerated -> marginal/undetectable");
    println!("    => detection covers fields with a semantic round-trip; the residue is fields");
    println!("       with none. Bake-and-pray survives ONLY there, and shrinks to that set.");

    println!("\n--- ⑤ RELATION TO Q4 ---");
    println!("    Q4/L1: 'is symbol index 1 really CreateFileA' is UNqueryable (circular: the");
    println!("      only way to test the call is to make it). Layout binding-truth ('is @44");
    println!("      really cFileName') IS checkable here — a wrong offset has an observable");
    println!("      consequence you can drive with known I/O. So the trust set shrinks from");
    println!("      {{naming + layout}} to {{naming}}: LAYOUT exits the trust hole, MODULO naming");
    println!("      (the self-check calls FindFirstFileA/bind — symbols L1 still trusts).");
}
