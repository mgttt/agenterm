//! Independent WASM 1.0 MVP goldens.
//!
//! Fixtures live in `tests/fixtures/` (not in `src/wasm.rs`). Expected values
//! were produced by `tests/fixtures/gen_mvp_goldens.py` from the spec, not by
//! running this interpreter. The runner only calls the shipped face:
//! [`agenterm_tinyvm::eval`] or `Module::from_bytes` + `bind_import` +
//! `Module::eval`.
//!
//! Three fixture files, three jobs:
//! - `mvp_goldens.txt` — one path per MVP opcode (the wiring gate).
//! - `family_extra.txt` — one extra per family, different operands/layout.
//! - `family_edge.txt` — spec boundaries: traps, signed/unsigned splits,
//!   shift-count masks, NaN and signed zero, ties-to-even, memory limits and
//!   data segments, the import table (the semantics gate).
//!
//! Every row must *execute*: there is no empty-module escape hatch, and each
//! row's declared opcode column is checked against the module's own bytes, so
//! coverage cannot be manufactured by editing a text column.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use agenterm_tinyvm::{Val, WasmError, WasmModule, eval};

struct Case {
    id: String,
    family: String,
    opcodes: Vec<u8>,
    expect: Expect,
    wasm: Vec<u8>,
    binds: Vec<(String, String)>,
}

enum Expect {
    I32(i32),
    I64(i64),
    F32Bits(u32),
    F64Bits(u64),
    /// Any NaN: the sign/payload of a hardware-produced NaN is platform
    /// dependent and the spec admits either, so bits are not pinned.
    F32Nan,
    F64Nan,
    Trap,
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn fixture_files() -> [&'static str; 4] {
    [
        "mvp_goldens.txt",
        "family_extra.txt",
        "family_edge.txt",
        "prd_leaves.txt",
    ]
}

/// `#78|...` is a data row. `# comment` and `# id|family|...` are not.
fn is_fixture_comment(line: &str) -> bool {
    let Some(rest) = line.strip_prefix('#') else {
        return false;
    };
    rest.starts_with(' ') || rest.starts_with("id|") || !rest.contains('|')
}

fn parse_expect(s: &str) -> Expect {
    if s == "trap" {
        return Expect::Trap;
    }
    if s == "f32nan" {
        return Expect::F32Nan;
    }
    if s == "f64nan" {
        return Expect::F64Nan;
    }
    if let Some(rest) = s.strip_prefix("i32:") {
        return Expect::I32(rest.parse().expect("i32 expect"));
    }
    if let Some(rest) = s.strip_prefix("i64:") {
        return Expect::I64(rest.parse().expect("i64 expect"));
    }
    if let Some(rest) = s.strip_prefix("f32bits:") {
        let bits: u32 = if let Some(h) = rest.strip_prefix("0x") {
            u32::from_str_radix(h, 16).expect("f32 bits")
        } else {
            rest.parse().expect("f32 bits")
        };
        return Expect::F32Bits(bits);
    }
    if let Some(rest) = s.strip_prefix("f64bits:") {
        let bits: u64 = if let Some(h) = rest.strip_prefix("0x") {
            u64::from_str_radix(h, 16).expect("f64 bits")
        } else {
            rest.parse().expect("f64 bits")
        };
        return Expect::F64Bits(bits);
    }
    panic!("unknown expect {s}");
}

fn load_cases(name: &str) -> Vec<Case> {
    let text = fs::read_to_string(fixtures_dir().join(name)).expect(name);
    let mut out = Vec::new();
    for (lineno, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || is_fixture_comment(line) {
            continue;
        }
        let parts: Vec<&str> = line.split('|').collect();
        assert!(
            parts.len() >= 5,
            "{name}:{} expected id|family|opcodes|expect|hex|bind",
            lineno + 1
        );
        let opcodes = parts[2]
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|s| u8::from_str_radix(s, 16).expect("opcode hex"))
            .collect();
        let binds = parts
            .get(5)
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| {
                s.split('+')
                    .map(|one| {
                        let (m, f) = one.split_once('.').expect("bind module.field");
                        (m.to_string(), f.to_string())
                    })
                    .collect()
            })
            .unwrap_or_default();
        let wasm = decode_hex(parts[4]);
        assert!(
            !wasm.is_empty(),
            "{name}:{}: every golden must carry a real module — no empty rows",
            lineno + 1
        );
        out.push(Case {
            id: parts[0].to_string(),
            family: parts[1].to_string(),
            opcodes,
            expect: parse_expect(parts[3]),
            wasm,
            binds,
        });
    }
    out
}

fn decode_hex(s: &str) -> Vec<u8> {
    assert!(s.len().is_multiple_of(2), "odd hex length");
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

fn must_ok<T>(r: Result<T, WasmError>, what: &str) -> T {
    match r {
        Ok(v) => v,
        Err(e) => panic!("{what}: {}", e.message()),
    }
}

/// The host side of the import table. Each arm is a plain native closure; the
/// guest reaches it only through a bound import.
fn bind_host(module: &mut WasmModule, m: &str, field: &str) {
    let r = match (m, field) {
        ("host", "mul") => module.bind_import(m, field, |args, _mem| {
            assert_eq!(args.len(), 2);
            Ok(vec![args[0].wrapping_mul(args[1])])
        }),
        ("host", "add19") => module.bind_import(m, field, |args, _mem| {
            assert_eq!(args.len(), 1);
            Ok(vec![args[0].wrapping_add(19)])
        }),
        ("host", "double") => module.bind_import(m, field, |args, _mem| {
            assert_eq!(args.len(), 1);
            Ok(vec![args[0].wrapping_mul(2)])
        }),
        ("host", "plus100") => module.bind_import(m, field, |args, _mem| {
            assert_eq!(args.len(), 1);
            Ok(vec![args[0].wrapping_add(100)])
        }),
        // Writes through the &mut [u8] memory handle the host gate hands out.
        ("host", "poke") => module.bind_import(m, field, |_args, mem| {
            mem[8..12].copy_from_slice(&35i32.to_le_bytes());
            Ok(vec![])
        }),
        other => panic!("unknown host bind {other:?}"),
    };
    must_ok(r, "bind_import");
}

fn run_case(case: &Case) -> Result<Vec<Val>, WasmError> {
    if case.binds.is_empty() {
        return eval(&case.wasm);
    }
    let mut module = WasmModule::from_bytes(&case.wasm)?;
    for (m, f) in &case.binds {
        bind_host(&mut module, m, f);
    }
    module.eval(&[])
}

fn describe_vals(vals: &[Val]) -> String {
    vals.iter()
        .map(|v| match v {
            Val::I32(n) => format!("i32:{n}"),
            Val::I64(n) => format!("i64:{n}"),
            Val::F32(n) => format!("f32bits:{:#x}", n.to_bits()),
            Val::F64(n) => format!("f64bits:{:#x}", n.to_bits()),
            #[cfg(feature = "simd")]
            Val::V128(bytes) => format!("v128:{bytes:02x?}"),
            Val::FuncRef(None) => "funcref:null".to_string(),
            Val::FuncRef(Some(index)) => format!("funcref:{index}"),
            Val::StoreFuncRef(_) => "funcref:store".to_string(),
            Val::ExternRef(None) => "externref:null".to_string(),
            Val::ExternRef(Some(_)) => "externref:host".to_string(),
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn assert_expect(case: &Case, got: Result<Vec<Val>, WasmError>) {
    match (&case.expect, got) {
        (Expect::Trap, Err(WasmError::Trap(_))) => {}
        (Expect::I32(e), Ok(v)) => match v.as_slice() {
            [Val::I32(g)] if g == e => {}
            other => panic!(
                "{}: expected i32 {e}, got {}",
                case.id,
                describe_vals(other)
            ),
        },
        (Expect::I64(e), Ok(v)) => match v.as_slice() {
            [Val::I64(g)] if g == e => {}
            other => panic!(
                "{}: expected i64 {e}, got {}",
                case.id,
                describe_vals(other)
            ),
        },
        (Expect::F32Bits(e), Ok(v)) => match v.as_slice() {
            [Val::F32(g)] if g.to_bits() == *e => {}
            other => panic!(
                "{}: expected f32bits {e:#x}, got {}",
                case.id,
                describe_vals(other)
            ),
        },
        (Expect::F64Bits(e), Ok(v)) => match v.as_slice() {
            [Val::F64(g)] if g.to_bits() == *e => {}
            other => panic!(
                "{}: expected f64bits {e:#x}, got {}",
                case.id,
                describe_vals(other)
            ),
        },
        (Expect::F32Nan, Ok(v)) => match v.as_slice() {
            [Val::F32(g)] if g.is_nan() => {}
            other => panic!(
                "{}: expected an f32 NaN, got {}",
                case.id,
                describe_vals(other)
            ),
        },
        (Expect::F64Nan, Ok(v)) => match v.as_slice() {
            [Val::F64(g)] if g.is_nan() => {}
            other => panic!(
                "{}: expected an f64 NaN, got {}",
                case.id,
                describe_vals(other)
            ),
        },
        (Expect::Trap, Ok(v)) => panic!("{}: expected trap, got {}", case.id, describe_vals(&v)),
        (Expect::Trap, Err(WasmError::Decode(m))) => {
            panic!("{}: expected trap, got decode {m}", case.id)
        }
        (_, Err(e)) => panic!("{}: unexpected {}", case.id, e.message()),
    }
}

// ---------------------------------------------------------------------------
// The opcode column is metadata; the module bytes are the truth. This walks a
// module's code section so coverage is derived from what a row really contains.
// ---------------------------------------------------------------------------

fn leb(bytes: &[u8], i: &mut usize) -> u64 {
    let mut shift = 0;
    let mut out = 0u64;
    loop {
        let b = bytes[*i];
        *i += 1;
        out |= u64::from(b & 0x7F) << shift;
        shift += 7;
        if b & 0x80 == 0 {
            return out;
        }
    }
}

fn skip_leb(bytes: &[u8], i: &mut usize) {
    while bytes[*i] & 0x80 != 0 {
        *i += 1;
    }
    *i += 1;
}

/// Opcode bytes present in a function body, immediates skipped correctly.
fn body_opcodes(body: &[u8], out: &mut BTreeSet<u8>) {
    let mut i = 0usize;
    while i < body.len() {
        let op = body[i];
        i += 1;
        out.insert(op);
        match op {
            0x02..=0x04 => i += 1, // blocktype
            0x0C | 0x0D | 0x10 | 0x20..=0x24 => skip_leb(body, &mut i),
            0x0E => {
                let n = leb(body, &mut i);
                for _ in 0..=n {
                    skip_leb(body, &mut i);
                }
            }
            0x11 => {
                skip_leb(body, &mut i);
                skip_leb(body, &mut i);
            }
            0x28..=0x3E => {
                skip_leb(body, &mut i);
                skip_leb(body, &mut i);
            }
            0x3F | 0x40 => i += 1,
            0x41 | 0x42 => skip_leb(body, &mut i),
            0x43 => i += 4,
            0x44 => i += 8,
            _ => {}
        }
    }
}

/// Every opcode byte in every function body of a module.
fn module_opcodes(wasm: &[u8]) -> BTreeSet<u8> {
    let mut out = BTreeSet::new();
    let mut i = 8usize; // magic + version
    while i < wasm.len() {
        let id = wasm[i];
        i += 1;
        let size = leb(wasm, &mut i) as usize;
        let start = i;
        let end = start + size;
        if id == 10 {
            let payload = &wasm[start..end];
            let mut j = 0usize;
            let count = leb(payload, &mut j);
            for _ in 0..count {
                let body_size = leb(payload, &mut j) as usize;
                let bend = j + body_size;
                let body = &payload[j..bend];
                // locals: vec of (count, valtype)
                let mut k = 0usize;
                let decls = leb(body, &mut k);
                for _ in 0..decls {
                    skip_leb(body, &mut k);
                    k += 1;
                }
                body_opcodes(&body[k..], &mut out);
                j = bend;
            }
        }
        i = end;
    }
    out
}

fn load_opcode_catalog() -> Vec<(String, u8)> {
    let text = fs::read_to_string(fixtures_dir().join("mvp_opcodes.txt")).unwrap();
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut it = line.split_whitespace();
        let name = it.next().unwrap().to_string();
        let byte = u8::from_str_radix(it.next().unwrap(), 16).unwrap();
        out.push((name, byte));
    }
    out
}

#[test]
fn fixtures_live_outside_the_interpreter_source() {
    let src = fixtures_dir();
    for f in fixture_files() {
        assert!(src.join(f).is_file(), "missing fixture {f}");
    }
    let wasm_rs = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/wasm.rs");
    let wasm_src = fs::read_to_string(wasm_rs).unwrap();
    for f in fixture_files() {
        assert!(
            !wasm_src.contains(f),
            "independent goldens must not be embedded in src/wasm.rs"
        );
    }
}

#[test]
fn catalog_lists_all_172_mvp_opcodes() {
    let catalog = load_opcode_catalog();
    assert_eq!(catalog.len(), 172, "WASM 1.0 MVP opcode count");
    let mut seen = BTreeSet::new();
    for (name, byte) in &catalog {
        assert!(
            seen.insert(*byte),
            "duplicate opcode byte {byte:#04x} ({name})"
        );
    }
}

/// A row may not claim an opcode its own module does not contain: coverage is
/// read from the bytes, and the column has to agree with them.
#[test]
fn opcode_columns_match_the_module_bytes() {
    let mut bad = Vec::new();
    for name in fixture_files() {
        for case in load_cases(name) {
            let present = module_opcodes(&case.wasm);
            for op in &case.opcodes {
                if !present.contains(op) {
                    bad.push(format!("{} claims {op:02X}, bytes do not have it", case.id));
                }
            }
        }
    }
    assert!(bad.is_empty(), "opcode column lies: {bad:#?}");
}

/// A row that is byte-identical to another adds no signal; the suite grew that
/// way once (store rows cloned from load rows) and must not again.
#[test]
fn no_two_goldens_are_byte_identical() {
    let mut seen: BTreeMap<(String, Vec<u8>), String> = BTreeMap::new();
    let mut dups = Vec::new();
    for name in ["mvp_goldens.txt", "family_extra.txt", "family_edge.txt"] {
        for case in load_cases(name) {
            let key = (format!("{:?}", ExpectKey(&case.expect)), case.wasm.clone());
            if let Some(first) = seen.insert(key, case.id.clone()) {
                dups.push(format!("{first} == {}", case.id));
            }
        }
    }
    assert!(dups.is_empty(), "duplicate goldens: {dups:?}");
}

struct ExpectKey<'a>(&'a Expect);

impl core::fmt::Debug for ExpectKey<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.0 {
            Expect::I32(v) => write!(f, "i32:{v}"),
            Expect::I64(v) => write!(f, "i64:{v}"),
            Expect::F32Bits(v) => write!(f, "f32:{v:#x}"),
            Expect::F64Bits(v) => write!(f, "f64:{v:#x}"),
            Expect::F32Nan => write!(f, "f32nan"),
            Expect::F64Nan => write!(f, "f64nan"),
            Expect::Trap => write!(f, "trap"),
        }
    }
}

#[test]
fn independent_goldens_cover_every_mvp_opcode_via_eval() {
    let catalog = load_opcode_catalog();
    let cases = load_cases("mvp_goldens.txt");
    assert!(cases.len() >= 172, "need a golden path for every opcode");

    // Coverage comes from the decoded module, not from the opcode column.
    let mut covered: BTreeMap<u8, String> = BTreeMap::new();
    for case in &cases {
        let got = run_case(case);
        assert_expect(case, got);
        for op in module_opcodes(&case.wasm) {
            covered.entry(op).or_insert_with(|| case.id.clone());
        }
    }

    let mut missing = Vec::new();
    for (name, byte) in &catalog {
        if !covered.contains_key(byte) {
            missing.push(format!("{name} {byte:02X}"));
        }
    }
    assert!(
        missing.is_empty(),
        "goldens missing {} opcodes: {missing:?}",
        missing.len()
    );
}

#[test]
fn each_family_has_an_extra_independent_golden() {
    let extras = load_cases("family_extra.txt");
    for fam in FAMILIES {
        assert!(
            extras.iter().any(|c| c.family == fam),
            "missing extra golden for family {fam}"
        );
    }
    for case in &extras {
        assert_expect(case, run_case(case));
    }
}

const FAMILIES: [&str; 10] = [
    "control",
    "parametric",
    "locals",
    "memory",
    "i32",
    "i64",
    "f32",
    "f64",
    "conv",
    "host",
];

/// The semantics gate: spec boundaries every family must hold, including the
/// trap conditions and sign/width/rounding rules a wiring-only suite misses.
#[test]
fn spec_edge_goldens_hold() {
    let cases = load_cases("family_edge.txt");
    let mut per_family: BTreeMap<&str, usize> = BTreeMap::new();
    for case in &cases {
        assert_expect(case, run_case(case));
        *per_family.entry(case.family.as_str()).or_default() += 1;
    }
    for fam in FAMILIES {
        let n = per_family.get(fam).copied().unwrap_or(0);
        assert!(n >= 6, "family {fam} has only {n} spec-boundary goldens");
    }
    let traps = cases
        .iter()
        .filter(|c| matches!(c.expect, Expect::Trap))
        .count();
    assert!(traps >= 20, "only {traps} trap goldens; traps are the gate");
}

#[test]
fn host_import_table_bind_and_unbound_are_independent() {
    let cases = load_cases("mvp_goldens.txt");
    let bound = cases.iter().find(|c| c.id == "host.mul").expect("host.mul");
    let unbound = cases
        .iter()
        .find(|c| c.id == "host.unbound")
        .expect("host.unbound");
    assert!(!bound.binds.is_empty());
    assert!(unbound.binds.is_empty());
    assert_expect(bound, run_case(bound));
    assert_expect(unbound, run_case(unbound));

    let mut m = must_ok(WasmModule::from_bytes(&bound.wasm), "from_bytes host.mul");
    assert_eq!(m.imports().len(), 1);
    assert_eq!(m.imports()[0].module, "host");
    assert_eq!(m.imports()[0].field, "mul");
    // Unbound until bind_import: the guest call must trap.
    assert!(matches!(m.eval(&[]), Err(WasmError::Trap(_))));
    // Binding a name that is not in the import table must fail loudly, and
    // must leave the gate shut.
    assert!(m.bind_import("host", "nope", |_, _| Ok(vec![])).is_err());
    assert!(m.bind_import("nope", "mul", |_, _| Ok(vec![])).is_err());
    assert!(matches!(m.eval(&[]), Err(WasmError::Trap(_))));
    bind_host(&mut m, "host", "mul");
    match must_ok(m.eval(&[]), "eval bound mul").as_slice() {
        [Val::I32(221)] => {}
        other => panic!("bound mul expected 221, got {}", describe_vals(other)),
    }
    // A host that fails must propagate its error, not be swallowed.
    let mut fails = must_ok(WasmModule::from_bytes(&bound.wasm), "from_bytes");
    must_ok(
        fails.bind_import("host", "mul", |_, _| Err(WasmError::Trap("host said no"))),
        "bind failing host",
    );
    assert!(matches!(fails.eval(&[]), Err(WasmError::Trap(_))));

    // Two imports: the function index space puts them first, so a defined
    // function's combined index is shifted by the import count.
    let edges = load_cases("family_edge.txt");
    let shift = edges
        .iter()
        .find(|c| c.id == "edge.host.index_shift_defined_func")
        .expect("index shift golden");
    let m2 = must_ok(WasmModule::from_bytes(&shift.wasm), "from_bytes shift");
    assert_eq!(m2.imports().len(), 2);
    assert_eq!(m2.export_index("main"), Some(3));
    assert_expect(shift, run_case(shift));
}

fn prd35_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../prd/PRD_02_35_agenterm_tinyvm.md")
}

/// `[x]` node tokens inside the first fenced tree of PRD 35.
fn parse_prd_x_leaves(prd: &str) -> Vec<String> {
    let mut in_fence = false;
    let mut leaves = Vec::new();
    for line in prd.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if !in_fence || !line.contains("[x]") {
            continue;
        }
        let token: String = line
            .replace("[x]", "")
            .chars()
            .map(|c| match c {
                '│' | '├' | '└' | '─' | '|' => ' ',
                _ => c,
            })
            .collect();
        let token = token.trim();
        if !token.is_empty() {
            leaves.push(token.to_string());
        }
    }
    leaves
}

/// Leaves whose backing is a test rather than a fixture family. The mapped
/// test must exist in this package's integration tests and assert something
/// concrete — the point of naming it here is that a leaf can no longer be
/// satisfied by a text row.
const LEAF_TESTS: [(&str, &str); 137] = [
    ("eval(bytes)", "eval_bytes"),
    ("iOS runtime boundary", "native_interpreter_boundary"),
    ("interpret wasm", "eval_bytes"),
    ("JIT native code", "native_interpreter_boundary"),
    ("device-side AOT", "native_interpreter_boundary"),
    ("dyn native loading", "native_interpreter_boundary"),
    ("tinyvm engine", "eval_bytes"),
    (
        "game runtime",
        "standard_wasm_cartridge_drives_one_bounded_frame",
    ),
    (
        "game ABI",
        "manifest_capabilities_and_lifecycle_signatures_are_exact",
    ),
    (
        "core v1 imports",
        "core_v1_media_versions_are_explicit_and_format_matched",
    ),
    (
        "strict declared-memory semantics",
        "standard_bytes_require_declared_memory",
    ),
    (
        "strict memarg alignment",
        "standard_memarg_alignment_is_validated_at_load",
    ),
    (
        "canonical function expression structure",
        "standard_function_expression_structure_is_canonical",
    ),
    (
        "strict i64 signed-LEB range",
        "standard_i64_leb_rejects_invalid_unused_high_bits",
    ),
    (
        "valid custom-section names",
        "standard_custom_section_name_is_validated_while_opaque_payload_stays_ignored",
    ),
    (
        "empty memory-section vector",
        "standard_bytes_require_declared_memory",
    ),
    (
        "mutable global.set target",
        "standard_global_set_requires_a_mutable_declaration",
    ),
    (
        "WABT load-gate oracle",
        "wabt_oracle_fixture_exactly_matches_the_rust_load_gate",
    ),
    ("stable two-pass copy lengths", "ios_xcframework_swift_link"),
    ("scoped immutable frame views", "ios_xcframework_swift_link"),
    ("single-buffer RGBA expansion", "ios_xcframework_swift_link"),
    (
        "bounded multi-source aggregation",
        "ios_xcframework_swift_link",
    ),
    (
        "Apple keyboard/gamepad adapter",
        "ios_xcframework_swift_link",
    ),
    (
        "overlapping keyboard alias retention",
        "ios_xcframework_swift_link",
    ),
    (
        "real App rising-edge input behavior",
        "current_main_runtime_runs_in_real_nostalgia_app_target",
    ),
    (
        "real iOS app consumer",
        "current_main_runtime_runs_in_real_nostalgia_app_target",
    ),
    (
        "static module validation CLI",
        "module_validate_cli_is_static_and_rejects_invalid_bytes",
    ),
    (
        "bulk memory copy/fill",
        "standard_bulk_memory_copy_fill_execute_with_wasm_semantics",
    ),
    (
        "bulk memory passive lifecycle",
        "standard_bulk_memory_proposal_executes_with_instance_semantics",
    ),
    (
        "sign extension proposal",
        "standard_sign_extension_proposal_executes",
    ),
    (
        "nontrapping float-to-int",
        "standard_nontrapping_conversion_proposal_saturates",
    ),
    (
        "multi-value proposal",
        "standard_multi_value_proposal_executes",
    ),
    (
        "single-table funcref profile",
        "standard_funcref_table_profile_executes_with_instance_semantics",
    ),
    (
        "multiple defined funcref tables",
        "standard_multiple_funcref_tables_execute_and_share_one_host_budget",
    ),
    (
        "multiple internally defined memories",
        "wabt_compiled_multi_memory_matches_tinyvm",
    ),
    (
        "extended constant expressions",
        "standard_extended_const_executes_and_rejects_invalid_expression_stacks",
    ),
    (
        "standard imported globals",
        "standard_imported_globals_bind_types_and_share_mutation",
    ),
    (
        "named standard resource exports",
        "standard_resource_exports_are_resolved_by_name",
    ),
    (
        "standard imported linear memories",
        "standard_imported_memory_binds_limits_and_shares_store_identity",
    ),
    (
        "store-owned imported funcref tables",
        "wabt_compiled_imported_table_decodes_in_standard_index_space",
    ),
    (
        "linked exported globals",
        "wabt_compiled_imported_globals_match_tinyvm",
    ),
    (
        "linked exported memories",
        "wabt_compiled_imported_memory_matches_tinyvm",
    ),
    (
        "linked exported tables",
        "wabt_compiled_imported_table_decodes_in_standard_index_space",
    ),
    (
        "linked exported functions",
        "wabt_compiled_exported_functions_link_across_instances",
    ),
    (
        "numeric value signatures",
        "wabt_compiled_exported_functions_link_across_instances",
    ),
    (
        "store-owned funcref values",
        "wabt_compiled_exported_functions_link_across_instances",
    ),
    (
        "opaque externref function/global values",
        "standard_externref_function_and_global_values_preserve_host_identity",
    ),
    (
        "standard externref tables",
        "wabt_compiled_externref_tables_preserve_host_identity",
    ),
    (
        "tail-call proposal",
        "standard_tail_calls_trampoline_across_direct_indirect_and_host_targets",
    ),
    (
        "decode complexity budget",
        "tiny_declared_count_bombs_fail_before_allocation",
    ),
    (
        "typed standard function imports",
        "standard_typed_host_imports_preserve_all_value_kinds",
    ),
    ("H5/JS/WKWebView", "native_interpreter_boundary"),
    (
        "persistent instance",
        "instance_preserves_globals_but_module_calls_stay_fresh",
    ),
    (
        "explicit guest call stack",
        "guest_call_stack_is_explicit_bounded_and_native_stack_independent",
    ),
    (
        "host-owned call-depth ceiling",
        "call_stack_limits_are_host_owned_and_fail_at_exact_boundaries",
    ),
    (
        "host-owned activation-slot ceiling",
        "call_stack_limits_are_host_owned_and_fail_at_exact_boundaries",
    ),
    (
        "fallible execution-stack growth",
        "operand_and_control_growth_are_preflighted_at_host_slot_boundary",
    ),
    ("start once", "instance_runs_start_exactly_once"),
    (
        "per-call fuel",
        "instruction_budget_is_host_owned_and_resets_per_call",
    ),
    (
        "memory budget",
        "memory_budget_rejects_initial_min_and_caps_grow",
    ),
    (
        "table budget",
        "table_budget_follows_host_not_crate_constant",
    ),
    (
        "deterministic execution stats",
        "execution_stats_are_deterministic_and_cover_guest_host_resources",
    ),
    (
        "call/activation peak telemetry",
        "execution_stats_are_deterministic_and_cover_guest_host_resources",
    ),
    (
        "bounded in-place host dispatch",
        "in_place_native_module_receives_exact_bounded_result_slice",
    ),
    (
        "standard .wasm cartridge",
        "standard_wasm_cartridge_drives_one_bounded_frame",
    ),
    (
        "manifest compatibility",
        "manifest_capabilities_and_lifecycle_signatures_are_exact",
    ),
    (
        "init/tick/suspend/resume",
        "suspend_resume_restores_guest_state_and_host_rng",
    ),
    (
        "portable state snapshot",
        "snapshot_identity_schema_and_bounds_fail_closed",
    ),
    (
        "protected prepublication snapshot replace",
        "ios_xcframework_swift_link",
    ),
    (
        "bounded prepared slot + borrowed restore slice",
        "ios_xcframework_swift_link",
    ),
    (
        "bounded frame output",
        "standard_core_only_cartridge_drives_an_indexed2d_frame",
    ),
    (
        "bounded app metadata hot path",
        "indexed2d_metadata_requires_an_explicit_core_capability",
    ),
    (
        "recyclable host buffers",
        "tick_into_recycles_bounded_frame_storage",
    ),
    (
        "native module registry",
        "native_dispatch_quota_is_charged_before_callback_and_resets_per_lifecycle",
    ),
    (
        "atomic resource-table factory",
        "native_module_can_own_a_resource_behind_a_generation_checked_guest_handle",
    ),
    ("App Store bundled-only gate", "ios_xcframework_swift_link"),
    (
        "machine host profile",
        "host_profile_is_canonical_and_checks_exact_standard_imports",
    ),
    (
        "exact zero-budget channel semantics",
        "zero_game_output_limits_round_trip_and_disable_each_channel",
    ),
    (
        "profile-bound descriptor return",
        "ios_xcframework_swift_link",
    ),
    (
        "metadata schema diagnostics",
        "signal_lock_converter_reports_bounded_application_metadata",
    ),
    (
        "indexed2d metadata extension",
        "ordinary_c_toolchain_emits_a_portable_standard_cartridge",
    ),
    ("catalog profile binding", "ios_xcframework_swift_link"),
    (
        "Depth Well grid3d",
        "standard_depth_well_plays_and_restores_deterministically",
    ),
    (
        "Paddle Guard indexed2d",
        "standard_paddle_guard_launches_moves_and_emits_indexed_frames",
    ),
    (
        "Signal Lock Swift-to-Wasm migration",
        "standard_signal_lock_rotates_channels_and_renders_a_readable_radar",
    ),
    (
        "deterministic replay vectors",
        "depth_well_replay_is_portable_bounded_and_tamper_evident",
    ),
    (
        "development JSC + H5 differential",
        "webkit_matches_tinyvm_replay",
    ),
    (
        "contract / abstraction / backend split",
        "unsupported_platform_operations_fail_explicitly",
    ),
    (
        "internal handles + preopen-only paths",
        "preopens_keep_physical_paths_out_of_guest_space",
    ),
    (
        "optional WASI Preview 1 adapter",
        "wasi_p1_complete_subset_binds_all_exact_signatures",
    ),
    (
        "args + environ",
        "wasi_p1_process_clock_random_preopen_and_close_execute_through_standard_imports",
    ),
    (
        "clock + random",
        "wasi_p1_process_clock_random_preopen_and_close_execute_through_standard_imports",
    ),
    (
        "preopen discovery + fd_close",
        "wasi_p1_process_clock_random_preopen_and_close_execute_through_standard_imports",
    ),
    (
        "fd read/write/seek/stat",
        "wasi_p1_fd_io_seek_and_stat_use_guest_descriptors_and_standard_layouts",
    ),
    (
        "path open/unlink",
        "wasi_p1_path_open_and_unlink_stay_relative_to_a_virtual_preopen",
    ),
    (
        "proc_exit",
        "wasi_p1_proc_exit_is_non_returning_and_exposes_the_typed_code",
    ),
    (
        "capability-directory std backend",
        "standard_wasi_module_reaches_the_real_preopen_backend",
    ),
    (
        "iOS Simulator App container wiring",
        "ios_wasi_host_simulator_container",
    ),
    (
        "indexed guest-memory callback context",
        "typed_host_can_borrow_selected_defined_memories_by_standard_index",
    ),
    (
        "domain + generation guest resource handles",
        "native_module_can_own_a_resource_behind_a_generation_checked_guest_handle",
    ),
    (
        "generalize memory-zero call-scoped borrowing",
        "typed_host_can_borrow_selected_defined_memories_by_standard_index",
    ),
    (
        "explicit selected-memory callback context",
        "selected_memory_context_preserves_aliasing_for_imported_memories",
    ),
    (
        "unified host/guest handle lifetimes",
        "native_module_can_own_a_resource_behind_a_generation_checked_guest_handle",
    ),
    (
        "cross-runtime non-reused table domains",
        "shared_allocator_prevents_cross_runtime_stale_handle_aliases",
    ),
    (
        "native-resource snapshot quiescence",
        "native_module_can_own_a_resource_behind_a_generation_checked_guest_handle",
    ),
    (
        "versioned native import conventions",
        "native_module_names_are_canonical_and_major_versioned",
    ),
    (
        "converter-visible compatibility reports",
        "host_profile_cli_publishes_inspects_and_checks_without_execution",
    ),
    (
        "cross-boundary copy/call benchmarks",
        "boundary_benchmark_separates_call_view_copy_and_guest_costs",
    ),
    (
        "bounded call/callback/completion channels",
        "native_completion_queue_bounds_identity_items_and_reserved_bytes",
    ),
    (
        "owner-thread completion queue core",
        "pending_native_completion_prevents_portable_snapshot",
    ),
    (
        "event-loop-neutral async completion ABI",
        "versioned_completion_imports_drive_pending_ready_take_and_stale_states",
    ),
    (
        "versioned guest import protocol",
        "versioned_completion_imports_drive_pending_ready_take_and_stale_states",
    ),
    (
        "C ABI channel ownership + late delivery",
        "ios_xcframework_swift_link",
    ),
    (
        "Swift MainActor owner + host profile",
        "ios_xcframework_swift_link",
    ),
    (
        "standard async cartridge fixture",
        "standard_async_completion_cartridge_runs_host_neutrally",
    ),
    (
        "booted iOS Simulator completion lifecycle",
        "ios_xcframework_swift_link",
    ),
    (
        "proposal priority by real cartridge workload",
        "real_cartridge_workload_prioritizes_standard_features",
    ),
    (
        "optional SIMD signed-PCM mix subset",
        "wabt_compiled_simd_audio_add_sub_matches_tinyvm",
    ),
    (
        "signed PCM saturating add/sub",
        "wabt_compiled_simd_audio_add_sub_matches_tinyvm",
    ),
    (
        "fan-authored standard .wasm",
        "ordinary_c_toolchain_emits_a_portable_standard_cartridge",
    ),
    (
        "header-only C core v1 declarations",
        "ordinary_c_toolchain_emits_a_portable_standard_cartridge",
    ),
    ("<100KiB>", "size_budget_script_gates_100kib"),
    ("#78", "issue78_runtimes_stay_out_of_the_crate"),
    ("cu", "cu"),
    ("dyn", "dyn"),
    ("chassis", "chassis"),
    ("WASI as implicit/default game host", "WASI"),
    ("APE", "APE"),
    ("WAT", "WAT"),
    (
        "independent WABT/JSC differential per leaf",
        "accepted_standard_feature_matrix_executes_all_oracles_and_budgets",
    ),
    (
        "size/resource budget retained per leaf",
        "accepted_standard_feature_matrix_executes_all_oracles_and_budgets",
    ),
    (
        "P2 — accepted standard Wasm coverage",
        "every_reported_standard_feature_has_an_independent_executable_matrix_edge",
    ),
    ("bounded PCM tone synthesis", "ios_xcframework_swift_link"),
    (
        "single-buffer WAV + bounded wave LRU",
        "ios_xcframework_swift_link",
    ),
    (
        "interruption / route / reset owner",
        "ios_xcframework_swift_link",
    ),
    (
        "real App exact tone-event consumption",
        "current_main_runtime_runs_in_real_nostalgia_app_target",
    ),
    (
        "real App shared session + frame pacer",
        "current_main_runtime_runs_in_real_nostalgia_app_target",
    ),
];

fn suite_test_names() -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let tests = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    for entry in fs::read_dir(tests).expect("read integration-test directory") {
        let path = entry.expect("integration-test entry").path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
            continue;
        }
        let src = fs::read_to_string(path).expect("read integration test");
        for line in src.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("fn ")
                && let Some(name) = rest.split('(').next()
            {
                names.insert(name.strip_prefix("r#").unwrap_or(name).to_string());
            }
        }
    }
    names
}

fn cargo_deps_section() -> String {
    let t =
        fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    let after = t.split("[dependencies]").nth(1).unwrap_or("");
    after.split("\n[").next().unwrap_or(after).to_string()
}

/// Every `[x]` leaf is either an opcode family with executed goldens in all
/// three fixture files, or a leaf mapped to a test that asserts it.
#[test]
fn prd_x_leaves_have_suite_edges() {
    let prd = fs::read_to_string(prd35_path()).expect("PRD_02_35");
    let leaves = parse_prd_x_leaves(&prd);
    assert!(!leaves.is_empty(), "PRD 35 fence must list [x] leaves");
    let tests = suite_test_names();
    let mut families: BTreeMap<String, usize> = BTreeMap::new();
    for name in ["mvp_goldens.txt", "family_extra.txt", "family_edge.txt"] {
        for case in load_cases(name) {
            *families.entry(case.family).or_default() += 1;
        }
    }
    let mapped: BTreeMap<&str, &str> = LEAF_TESTS.into_iter().collect();
    let mut missing = Vec::new();
    for leaf in &leaves {
        if let Some(test) = mapped.get(leaf.as_str()) {
            assert!(
                tests.contains(*test),
                "leaf {leaf} maps to missing test {test}"
            );
            continue;
        }
        match families.get(leaf) {
            // A family leaf needs a base path, an extra, and a spec edge.
            Some(n) if *n >= 3 => {}
            _ => missing.push(leaf.clone()),
        }
    }
    assert!(
        missing.is_empty(),
        "PRD [x] leaves with no executed backing: {missing:?}"
    );
}

#[test]
fn eval_bytes() {
    let case = load_cases("prd_leaves.txt")
        .into_iter()
        .find(|c| c.id == "eval(bytes)")
        .expect("eval(bytes) fixture");
    assert_expect(&case, run_case(&case));
}

#[test]
fn native_interpreter_boundary() {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let wasm = fs::read_to_string(crate_dir.join("src/wasm.rs")).expect("read wasm engine");
    assert!(
        !wasm.contains("unsafe"),
        "the interpreter engine must not require an unsafe native-code door"
    );
    let deps = cargo_deps_section().to_ascii_lowercase();
    for backend in [
        "javascriptcore",
        "webkit",
        "wasmtime",
        "wasmi",
        "cranelift",
        "dynasm",
    ] {
        assert!(
            !deps.contains(backend),
            "{backend} must not replace the tinyvm interpreter authority"
        );
    }
}

/// The `<100KiB>` leaf is a measurement, not a grep: this builds the no_std
/// static core, links it with production dead-code elimination, strips it, and
/// checks the size and the selftest.
#[test]
fn size_budget_script_gates_100kib() {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let script = crate_dir.join("measure-core.sh");
    let sh = fs::read_to_string(&script).unwrap();
    assert!(
        sh.contains("102400"),
        "measure-core.sh must keep the 100 KiB cap"
    );
    assert!(sh.contains("OK: < 100 KiB and selftest==42"));

    if Command::new("sh")
        .args(["-c", "command -v cc"])
        .output()
        .map(|o| !o.status.success())
        .unwrap_or(true)
    {
        eprintln!("no C compiler: size gate not measured in this environment");
        return;
    }
    // A separate target dir so the outer cargo's lock is untouched.
    let target = crate_dir.join("../../target/measure-core-gate");
    let out = Command::new("sh")
        .arg(&script)
        .env("CARGO_TARGET_DIR", target)
        .output()
        .expect("run measure-core.sh");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success() && stdout.contains("OK: < 100 KiB and selftest==42"),
        "measure-core.sh failed:\n{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let size: usize = stdout
        .lines()
        .find_map(|l| l.strip_prefix("static core: "))
        .and_then(|l| l.split(' ').next())
        .and_then(|n| n.parse().ok())
        .expect("size line");
    assert!(size < 102_400, "static core is {size} bytes");
    assert!(
        stdout.contains("selftest rc=42"),
        "selftest must return 42:\n{stdout}"
    );
}

#[test]
fn cu() {
    assert!(
        !cargo_deps_section().contains("agenterm-cu"),
        "cu is a non-goal: not a crate dependency"
    );
}

#[test]
fn r#dyn() {
    assert!(
        !cargo_deps_section().contains("agenterm-dyn"),
        "dyn is a non-goal: not a crate dependency"
    );
}

#[test]
fn chassis() {
    assert!(
        !cargo_deps_section().contains("agenterm-chassis"),
        "chassis is a non-goal: not a crate dependency"
    );
}

#[test]
#[allow(non_snake_case)]
fn WASI() {
    assert!(
        !cargo_deps_section().to_ascii_lowercase().contains("wasi"),
        "an external WASI runtime must not replace tinyvm's optional owned adapter"
    );
}

#[test]
#[allow(non_snake_case)]
fn APE() {
    let deps = cargo_deps_section();
    assert!(
        !deps.lines().any(|l| l.trim_start().starts_with("ape")),
        "APE is a non-goal: not kernel work"
    );
}

#[test]
fn issue78_runtimes_stay_out_of_the_crate() {
    let deps = cargo_deps_section().to_ascii_lowercase();
    for banned in ["sljit", "wasmtime", "wasmi", "wasmbin"] {
        assert!(
            !deps.contains(banned),
            "{banned} must not be a crate dep (#78)"
        );
    }
}

#[test]
#[allow(non_snake_case)]
fn WAT() {
    let deps = cargo_deps_section();
    assert!(
        !deps.to_ascii_lowercase().contains("wat") && !deps.contains("wabt"),
        "WAT is not a kernel input"
    );
}
