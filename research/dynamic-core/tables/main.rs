//! Q7 driver — lower Q1's payloads through the TABLE-DRIVEN marshaller and prove the
//! result runs (criterion ①), then report ②–⑤. Reuses Q1's ir/asm/payloads verbatim
//! via #[path]; the ONLY new logic is `table.rs` (data) + `marshal.rs` (fixed engine).

#[path = "../ir/spec/ir.rs"]
mod ir;
#[path = "../ir/lower/asm.rs"]
mod asm;
#[path = "../ir/payloads/payloads.rs"]
mod payloads;
#[path = "table.rs"]
mod table;
#[path = "marshal.rs"]
mod marshal;

use ir::{Intent, Module};

#[cfg(windows)]
extern "system" {
    fn VirtualAlloc(addr: *mut u8, size: usize, typ: u32, protect: u32) -> *mut u8;
    fn VirtualProtect(addr: *mut u8, size: usize, protect: u32, old: *mut u32) -> i32;
    fn LoadLibraryA(name: *const u8) -> *mut u8;
    fn GetProcAddress(module: *mut u8, name: *const u8) -> *mut u8;
    fn GetStdHandle(which: u32) -> *mut u8;
    fn SetStdHandle(which: u32, h: *mut u8) -> i32;
    fn CreateFileA(name: *const u8, access: u32, share: u32, sa: *mut u8, disp: u32, flags: u32, tmpl: *mut u8) -> *mut u8;
    fn CloseHandle(h: *mut u8) -> i32;
}
#[cfg(windows)]
const STD_OUTPUT_HANDLE: u32 = 0xFFFF_FFF5;

#[cfg(windows)]
struct Ctx {
    _bindings: Vec<u64>,
    _rodata: Vec<u8>,
    words: Vec<u64>, // [0]=&bindings [1]=&rodata [2]=stdout handle
}
#[cfg(windows)]
impl Ctx {
    fn new(rodata: &[u8]) -> Self {
        unsafe {
            let k32 = LoadLibraryA(b"kernel32.dll\0".as_ptr());
            let mut bindings = Vec::new();
            for s in table::WIN_SYMBOLS {
                let mut name: Vec<u8> = s.bytes().collect();
                name.push(0);
                let a = GetProcAddress(k32, name.as_ptr());
                assert!(!a.is_null(), "unresolved symbol {s}");
                bindings.push(a as u64);
            }
            let mut c = Ctx { _bindings: bindings, _rodata: rodata.to_vec(), words: vec![0, 0, 0] };
            c.words[0] = c._bindings.as_ptr() as u64;
            c.words[1] = c._rodata.as_ptr() as u64;
            c
        }
    }
    fn ptr(&self) -> *const u64 {
        self.words.as_ptr()
    }
}

#[cfg(windows)]
fn jit_run(code: &[u8], ctx: *const u64) -> u64 {
    unsafe {
        let mem = VirtualAlloc(std::ptr::null_mut(), code.len().max(1), 0x3000, 0x04);
        assert!(!mem.is_null());
        std::ptr::copy_nonoverlapping(code.as_ptr(), mem, code.len());
        let mut old = 0u32;
        VirtualProtect(mem, code.len(), 0x20, &mut old);
        let f: extern "win64" fn(*const u64) -> u64 = std::mem::transmute(mem);
        f(ctx)
    }
}

/// Redirect stdout to a temp file, put the redirected HANDLE into ctx[2] (modelling the
/// kernel's ③ reach resolving stdout at bind time), run, restore, return captured bytes.
#[cfg(windows)]
fn jit_run_capture(code: &[u8], ctx: &mut Ctx, tmp: &str) -> (u64, Vec<u8>) {
    unsafe {
        let mut name: Vec<u8> = tmp.bytes().collect();
        name.push(0);
        let fh = CreateFileA(name.as_ptr(), 0x4000_0000, 0, std::ptr::null_mut(), 2, 0x80, std::ptr::null_mut());
        let saved = GetStdHandle(STD_OUTPUT_HANDLE);
        SetStdHandle(STD_OUTPUT_HANDLE, fh);
        ctx.words[2] = fh as u64; // WriteStdout reads ctx[2]
        let r = jit_run(code, ctx.ptr());
        SetStdHandle(STD_OUTPUT_HANDLE, saved);
        CloseHandle(fh);
        let cap = std::fs::read(tmp).unwrap_or_default();
        (r, cap)
    }
}

fn lower_both(m: &Module) -> (Vec<u8>, Vec<u8>) {
    let sysv = marshal::lower(m, &table::SYSV64);
    let win = marshal::lower(m, &table::WIN64);
    (sysv, win)
}

fn reference_pure() -> u64 {
    let mut acc: u64 = 1469598103934665603;
    let mut i: u64 = 0;
    while i < 1_000_000 {
        acc = acc.wrapping_mul(1099511628211).wrapping_add(i);
        i = i.wrapping_add(1);
    }
    acc & 0xff
}
fn reference_hash(bytes: &[u8]) -> u64 {
    let mut h: u64 = 14695981039346656037;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    h
}

fn main() {
    println!("== Q7: OS-interface content as DATA — table-driven marshaller ==\n");

    let pure = payloads::pure_compute();
    let rhp = payloads::read_hash_print();

    let (pure_s, pure_w) = lower_both(&pure);
    let (rhp_s, rhp_w) = lower_both(&rhp);

    // ---- emitted sizes (③ numerator; same measurement basis as Q1) ----
    println!("emitted code size (bytes)   sysv64   win64   identical?");
    for (n, s, w) in [("pure_compute", &pure_s, &pure_w), ("read_hash_print", &rhp_s, &rhp_w)] {
        println!("  {:<18} {:>6}  {:>6}   {}", n, s.len(), w.len(), s == w);
    }
    let _ = std::fs::create_dir_all("out");
    for (n, s, w) in [("pure_compute", &pure_s, &pure_w), ("read_hash_print", &rhp_s, &rhp_w)] {
        let _ = std::fs::write(format!("out/{n}.sysv64.bin"), s);
        let _ = std::fs::write(format!("out/{n}.win64.bin"), w);
    }
    println!();

    // ---- ④ schema validation over the DATA (both targets) ----
    println!("== criterion ④ — schema validation of the tables ==");
    let intents: &[(Intent, usize)] = &[
        (Intent::Alloc, 1),
        (Intent::FileOpen, 1),
        (Intent::FileRead, 3),
        (Intent::FileClose, 1),
        (Intent::WriteStdout, 2),
    ];
    for abi in [&table::WIN64, &table::SYSV64] {
        let r = marshal::validate(abi, intents);
        println!("  {:<7} checked {} intent rows, {} schema errors{}",
                 abi.name, r.checked, r.errors.len(),
                 if r.errors.is_empty() { "  OK" } else { "  FAIL" });
        for e in &r.errors {
            println!("     - {e}");
        }
    }
    // negative probe: a corrupted row must be REJECTED by the schema (proves non-vacuous)
    {
        // Sem(9) on an arity-1 intent is out of range; validate() would flag it.
        let bad = marshal::validate(&table::WIN64, &[(Intent::FileOpen, 0)]);
        println!("  negative probe: FileOpen with declared arity 0 -> {} error(s) {}",
                 bad.errors.len(), if bad.errors.is_empty() { "FAIL(vacuous)" } else { "OK(rejects)" });
    }
    println!();

    // ---- ① execution evidence (Win64 native, real kernel32) ----
    #[cfg(windows)]
    {
        println!("== criterion ① — execution evidence (table-driven bytes, real kernel32) ==");
        assert_eq!(pure_s, pure_w, "pure_compute must lower identically for both ABIs");
        let r = jit_run(&pure_w, std::ptr::null());
        let want = reference_pure();
        println!("  pure_compute    -> {:>3}  (expected {})  {}", r, want, if r == want { "OK" } else { "FAIL" });

        let input = b"dynamic-core experiment 2026-08-08\n";
        std::fs::write("input.txt", input).unwrap();
        let mut ctx = Ctx::new(&rhp.rodata);
        let (_r, out) = jit_run_capture(&rhp_w, &mut ctx, "rhp_stdout.tmp");
        let want_hex = format!("{:016x}\n", reference_hash(input));
        let got = String::from_utf8_lossy(&out).to_string();
        println!("  read_hash_print -> {:?}  (expected {:?})  {}",
                 got.trim_end(), want_hex.trim_end(), if got == want_hex { "OK" } else { "FAIL" });
        let _ = std::fs::remove_file("rhp_stdout.tmp");
    }
    #[cfg(not(windows))]
    println!("(non-Windows host: execution skipped; byte measurement only)");
    println!();

    // ---- ⑤ the boundary: classify every fact of SpawnWait ----
    println!("== criterion ⑤ — the boundary (SpawnWait, the L3 hard bone) ==");
    let facts = marshal::spawn_boundary();
    let (mut data, mut query, mut code) = (0, 0, 0);
    for f in &facts {
        match f.verdict {
            "data" => data += 1,
            "query" => query += 1,
            _ => code += 1,
        }
        println!("  [{:>5}] {:<48} {}", f.verdict, f.name, f.why);
    }
    println!("  --> tablable-as-data={data}  needs-query-channel={query}  IRREDUCIBLY-code={code}");
    println!("  (SpawnWait is NOT in either target table -> single-call marshaller cannot lower it.)");
    println!("\n== done ==");
}
