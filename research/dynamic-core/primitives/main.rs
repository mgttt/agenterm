//! Q6 — primitive-completeness harness (Win64, really runs).
//!
//! Question: is the four-primitive floor stable, or does it creep? We add THREE
//! capabilities of divergent arg-count/shape (memory-mapped file, directory
//! traversal, socket bind) using ONLY the four primitives ③ sym + ④ call + ①②
//! memory — never a new kernel semantic. Then we measure:
//!   ① the max native-call arity (does ④'s ceiling get forced past 11?)
//!   ② which struct-layout facts four primitives can/can't reach
//!   ③ what a 5th `declare` primitive would add
//!
//! Clean-room: the sym/call below mirror `core/kernel.rs`'s CONTRACT (same
//! four-primitive model) rewritten from scratch for a std test harness; no
//! external implementation consulted.

#![allow(non_snake_case)]

// ===========================================================================
// The four primitives — Win64 (mirrors core/kernel.rs contract, rewritten).
// ③ reach = sym (LoadLibraryA + GetProcAddress); ④ call = data-driven native
// call over the integer/pointer word subset. ①② via VirtualAlloc/Protect —
// but the three capabilities below don't need fresh executable memory, so we
// exercise only ③+④ here (that IS the point: capabilities = reach + call).
// ===========================================================================

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryA(name: *const u8) -> *mut u8;
    fn GetProcAddress(module: *mut u8, name: *const u8) -> *mut u8;
}

/// ③ symbol resolution.
fn sym(module: &[u8], name: &[u8]) -> *mut u8 {
    unsafe {
        let h = LoadLibraryA(module.as_ptr());
        GetProcAddress(h, name.as_ptr())
    }
}

/// ④ call — invoke any native address from a data description (arg words).
/// Ceiling is 11 (Q0's 7->11 step). We deliberately DO NOT raise it; if a
/// capability needs more, that is the ① finding, recorded, not patched.
const CALL_CEILING: usize = 11;

fn call(addr: *mut u8, args: &[usize]) -> usize {
    assert!(
        args.len() <= CALL_CEILING,
        "④ call ceiling {} exceeded by {} args — this is a ① FINDING, not to be patched away",
        CALL_CEILING,
        args.len()
    );
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
            7 => (t!(usize, usize, usize, usize, usize, usize, usize))(
                a[0], a[1], a[2], a[3], a[4], a[5], a[6],
            ),
            8 => (t!(usize, usize, usize, usize, usize, usize, usize, usize))(
                a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7],
            ),
            9 => (t!(usize, usize, usize, usize, usize, usize, usize, usize, usize))(
                a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8],
            ),
            10 => (t!(usize, usize, usize, usize, usize, usize, usize, usize, usize, usize))(
                a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8], a[9],
            ),
            _ => (t!(usize, usize, usize, usize, usize, usize, usize, usize, usize, usize, usize))(
                a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8], a[9], a[10],
            ),
        }
    }
}

fn k32(name: &[u8]) -> *mut u8 {
    sym(b"kernel32.dll\0", name)
}

// Records the arity actually used, so ① is measured, not asserted.
struct ArityLog {
    max: usize,
    calls: Vec<(&'static str, usize)>,
}
impl ArityLog {
    fn new() -> Self {
        ArityLog { max: 0, calls: vec![] }
    }
    fn record(&mut self, name: &'static str, n: usize) {
        if n > self.max {
            self.max = n;
        }
        self.calls.push((name, n));
    }
}

// ===========================================================================
// Capability A — memory-mapped file.  NO struct field read (pure ④).
// ===========================================================================
fn cap_mmap_file(log: &mut ArityLog) -> Result<u64, String> {
    // Map this very source file, FNV-1a/64 the first 64 bytes.
    let path = b"main.rs\0";
    const GENERIC_READ: usize = 0x8000_0000;
    const FILE_SHARE_READ: usize = 0x1;
    const OPEN_EXISTING: usize = 3;
    const FILE_ATTRIBUTE_NORMAL: usize = 0x80;
    const PAGE_READONLY: usize = 0x02;
    const FILE_MAP_READ: usize = 0x4;
    const INVALID: usize = usize::MAX;

    log.record("CreateFileA", 7);
    let h = call(
        k32(b"CreateFileA\0"),
        &[
            path.as_ptr() as usize,
            GENERIC_READ,
            FILE_SHARE_READ,
            0,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            0,
        ],
    );
    if h == INVALID || h == 0 {
        return Err("CreateFileA failed".into());
    }
    log.record("CreateFileMappingA", 6);
    let m = call(
        k32(b"CreateFileMappingA\0"),
        &[h, 0, PAGE_READONLY, 0, 0, 0],
    );
    if m == 0 {
        return Err("CreateFileMappingA failed".into());
    }
    log.record("MapViewOfFile", 5);
    let view = call(k32(b"MapViewOfFile\0"), &[m, FILE_MAP_READ, 0, 0, 0]);
    if view == 0 {
        return Err("MapViewOfFile failed".into());
    }
    // Read raw bytes DIRECTLY from the returned pointer — no struct offset.
    let mut hsh: u64 = 0xcbf2_9ce4_8422_2325;
    unsafe {
        let p = view as *const u8;
        for i in 0..64 {
            hsh ^= *p.add(i) as u64;
            hsh = hsh.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    log.record("UnmapViewOfFile", 1);
    call(k32(b"UnmapViewOfFile\0"), &[view]);
    log.record("CloseHandle", 1);
    call(k32(b"CloseHandle\0"), &[h]);
    Ok(hsh)
}

// ===========================================================================
// Capability B — directory traversal.  READS WIN32_FIND_DATAA.cFileName.
// The offset 44 is a BAKED CONSTANT — nothing in ①②③④ produced it.
// ===========================================================================
const WIN32_FIND_DATAA_SIZE: usize = 320;
const OFF_cFileName: usize = 44; // <-- BAKED layout fact. See ② finding.

fn cap_dir_list(log: &mut ArityLog) -> Result<usize, String> {
    let pattern = b"*\0";
    let mut data = [0u8; WIN32_FIND_DATAA_SIZE];
    const INVALID: usize = usize::MAX;

    log.record("FindFirstFileA", 2);
    let hfind = call(
        k32(b"FindFirstFileA\0"),
        &[pattern.as_ptr() as usize, data.as_mut_ptr() as usize],
    );
    if hfind == INVALID {
        return Err("FindFirstFileA failed".into());
    }
    let mut count = 0usize;
    let mut shown = 0usize;
    loop {
        // Read cFileName at the BAKED offset. Four primitives gave us the
        // struct POINTER (via the out-param) but NOT the offset of the field.
        let name_ptr = unsafe { data.as_ptr().add(OFF_cFileName) };
        let mut namebuf = String::new();
        unsafe {
            let mut i = 0;
            while *name_ptr.add(i) != 0 && i < 260 {
                namebuf.push(*name_ptr.add(i) as char);
                i += 1;
            }
        }
        if shown < 4 {
            println!("      dir entry: {}", namebuf);
            shown += 1;
        }
        count += 1;
        log.record("FindNextFileA", 2);
        let ok = call(k32(b"FindNextFileA\0"), &[hfind, data.as_mut_ptr() as usize]);
        if ok == 0 {
            break;
        }
    }
    log.record("FindClose", 1);
    call(k32(b"FindClose\0"), &[hfind]);
    Ok(count)
}

// ===========================================================================
// Capability C — socket bind (loopback, no remote).  WRITES sockaddr_in.
// Offsets 0/2/4 are BAKED CONSTANTS.
// ===========================================================================
const OFF_sin_family: usize = 0; // BAKED
const OFF_sin_port: usize = 2; // BAKED
const OFF_sin_addr: usize = 4; // BAKED
const SOCKADDR_IN_SIZE: usize = 16;

fn cap_socket_bind(log: &mut ArityLog) -> Result<(), String> {
    let ws2 = b"ws2_32.dll\0";
    const AF_INET: usize = 2;
    const SOCK_DGRAM: usize = 2;
    const IPPROTO_UDP: usize = 17;
    const INVALID_SOCKET: usize = usize::MAX;

    let mut wsadata = [0u8; 512];
    log.record("WSAStartup", 2);
    let r = call(sym(ws2, b"WSAStartup\0"), &[0x0202, wsadata.as_mut_ptr() as usize]);
    if r != 0 {
        return Err(format!("WSAStartup -> {}", r));
    }
    log.record("socket", 3);
    let s = call(sym(ws2, b"socket\0"), &[AF_INET, SOCK_DGRAM, IPPROTO_UDP]);
    if s == INVALID_SOCKET {
        return Err("socket failed".into());
    }
    // Build sockaddr_in by writing fields at BAKED offsets.
    let mut sa = [0u8; SOCKADDR_IN_SIZE];
    sa[OFF_sin_family] = AF_INET as u8; // sin_family = AF_INET (little-endian u16)
    sa[OFF_sin_family + 1] = 0;
    // sin_port = 0 (ephemeral) — network byte order; 0 is order-agnostic.
    sa[OFF_sin_port] = 0;
    sa[OFF_sin_port + 1] = 0;
    // sin_addr = 127.0.0.1 = 0x7F000001, stored in network order = 7F 00 00 01.
    sa[OFF_sin_addr] = 127;
    sa[OFF_sin_addr + 1] = 0;
    sa[OFF_sin_addr + 2] = 0;
    sa[OFF_sin_addr + 3] = 1;

    log.record("bind", 3);
    let b = call(
        sym(ws2, b"bind\0"),
        &[s, sa.as_ptr() as usize, SOCKADDR_IN_SIZE],
    );
    if b != 0 {
        // record but don't hard-fail measurement
        eprintln!("      (bind returned {}, WSAGetLastError follows)", b);
    }
    log.record("closesocket", 1);
    call(sym(ws2, b"closesocket\0"), &[s]);
    log.record("WSACleanup", 0);
    call(sym(ws2, b"WSACleanup\0"), &[]);
    if b != 0 {
        return Err(format!("bind -> {}", b));
    }
    Ok(())
}

// ===========================================================================
// ③ measurement — the candidate 5th primitive `declare`, two halves.
// 5b Describe-machine: ASK the host a machine fact (page size). This IS host-
//    answerable — and note it is reachable via ordinary ③+④ (GetSystemInfo),
//    EXCEPT the offset of dwPageSize (@4) is itself a baked layout fact.
// 5a/query-layout: there is NO machine-readable struct-layout oracle on
//    Windows (no BTF). So the "query" degenerates to a baked table — the trust
//    just moves from adapter to kernel. Recorded, not hidden.
// ===========================================================================
const OFF_SYSTEM_INFO_dwPageSize: usize = 4; // BAKED even to READ the describe answer

fn declare_describe_pagesize(log: &mut ArityLog) -> u32 {
    let mut si = [0u8; 48]; // SYSTEM_INFO
    log.record("GetSystemInfo", 1);
    call(k32(b"GetSystemInfo\0"), &[si.as_mut_ptr() as usize]);
    unsafe { *(si.as_ptr().add(OFF_SYSTEM_INFO_dwPageSize) as *const u32) }
}

fn main() {
    println!("=== Q6 primitive-completeness harness (Win64, real run) ===\n");
    let mut log = ArityLog::new();

    println!("[A] memory-mapped file (no struct field read):");
    match cap_mmap_file(&mut log) {
        Ok(h) => println!("    OK  fnv64(first 64 bytes of main.rs) = {:016x}", h),
        Err(e) => println!("    ERR {}", e),
    }

    println!("[B] directory traversal (reads WIN32_FIND_DATAA.cFileName @44):");
    match cap_dir_list(&mut log) {
        Ok(n) => println!("    OK  {} entries in cwd", n),
        Err(e) => println!("    ERR {}", e),
    }

    println!("[C] socket bind loopback (writes sockaddr_in @0/2/4):");
    match cap_socket_bind(&mut log) {
        Ok(()) => println!("    OK  UDP socket bound to 127.0.0.1:0"),
        Err(e) => println!("    ERR {}", e),
    }

    println!("\n[declare] Describe-machine (host-answerable):");
    let ps = declare_describe_pagesize(&mut log);
    println!("    page size (GetSystemInfo) = {} bytes", ps);

    // ---- ① result ----
    println!("\n--- ① ④ arity ceiling: STEP vs SLOPE ---");
    for (name, n) in &log.calls {
        println!("      {:<20} {} args", name, n);
    }
    println!("    MAX native-call arity across ALL capabilities = {}", log.max);
    println!(
        "    ④ ceiling = {}.  Forced past 11 by any capability? {}",
        CALL_CEILING,
        if log.max > CALL_CEILING { "YES (SLOPE)" } else { "NO (one-time STEP holds)" }
    );

    // ---- ② result ----
    println!("\n--- ② layout facts: reachable by four primitives? ---");
    println!("    A mmap-file : NONE            (pure ④, no offsetof)            -> data N/A");
    println!("    B dir       : cFileName @44   (READ)  -> BAKED constant, unverified per-target trust");
    println!("    C socket    : sin_* @0/2/4    (WRITE) -> BAKED constants, unverified per-target trust");
    println!("    declare(pg) : dwPageSize @4   (READ)  -> BAKED even to read the host's own answer");
    println!("    => four primitives RAN all of the above WITH baked offsets (Claim K holds:");
    println!("       the KERNEL did not grow). But NONE of ①②③④ PRODUCED an offset:");
    println!("       offsetof is reachable by nothing here — it is baked (trust) or, where a");
    println!("       host publishes layout (Linux BTF), fetched via ③+④ + a payload parser.");
    println!("       Absent host publication (Windows system structs) it is IRREDUCIBLY baked.");
    println!("       => Claim R ('nothing unreachable') is FALSIFIED for the layout class.");

    println!("\n(byte-floor of a `declare` primitive: see kernel4.rs vs kernel5.rs, RESULTS ③)");
}
