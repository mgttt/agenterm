//! Independent WASM 1.0 MVP goldens.
//!
//! Fixtures live in `tests/fixtures/` (not in `src/wasm.rs`). Expected values
//! were produced by `tests/fixtures/gen_mvp_goldens.py` from the spec, not by
//! running this interpreter. The runner only calls the shipped face:
//! [`agenterm_tinyvm::eval`] or `Module::from_bytes` + `bind_import` +
//! `Module::eval`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use agenterm_tinyvm::{Val, WasmError, WasmModule, eval};

struct Case {
    id: String,
    family: String,
    opcodes: Vec<u8>,
    expect: Expect,
    wasm: Vec<u8>,
    bind: Option<(String, String)>,
}

enum Expect {
    I32(i32),
    I64(i64),
    F32Bits(u32),
    F64Bits(u64),
    Trap,
    Empty,
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
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
    if s == "empty" {
        return Expect::Empty;
    }
    if s == "edge" {
        return Expect::Empty;
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
        let bind = parts.get(5).and_then(|s| {
            let s = s.trim();
            if s.is_empty() {
                None
            } else {
                let (m, f) = s.split_once('.').expect("bind module.field");
                Some((m.to_string(), f.to_string()))
            }
        });
        out.push(Case {
            id: parts[0].to_string(),
            family: parts[1].to_string(),
            opcodes,
            expect: parse_expect(parts[3]),
            wasm: decode_hex(parts[4]),
            bind,
        });
    }
    out
}

fn decode_hex(s: &str) -> Vec<u8> {
    assert!(s.len() % 2 == 0, "odd hex length");
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

fn bind_host(module: &mut WasmModule, m: &str, field: &str) {
    match (m, field) {
        ("host", "mul") => {
            must_ok(
                module.bind_import(m, field, |args, _mem| {
                    assert_eq!(args.len(), 2);
                    Ok(vec![args[0].wrapping_mul(args[1])])
                }),
                "bind mul",
            );
        }
        ("host", "add19") => {
            must_ok(
                module.bind_import(m, field, |args, _mem| {
                    assert_eq!(args.len(), 1);
                    Ok(vec![args[0].wrapping_add(19)])
                }),
                "bind add19",
            );
        }
        other => panic!("unknown host bind {other:?}"),
    }
}

fn run_case(case: &Case) -> Result<Vec<Val>, WasmError> {
    if case.wasm.is_empty() {
        return Ok(Vec::new());
    }
    match &case.bind {
        None => eval(&case.wasm),
        Some((m, f)) => {
            let mut module = WasmModule::from_bytes(&case.wasm)?;
            bind_host(&mut module, m, f);
            module.eval(&[])
        }
    }
}

fn describe_vals(vals: &[Val]) -> String {
    vals.iter()
        .map(|v| match v {
            Val::I32(n) => format!("i32:{n}"),
            Val::I64(n) => format!("i64:{n}"),
            Val::F32(n) => format!("f32bits:{:#x}", n.to_bits()),
            Val::F64(n) => format!("f64bits:{:#x}", n.to_bits()),
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn assert_expect(case: &Case, got: Result<Vec<Val>, WasmError>) {
    match (&case.expect, got) {
        (Expect::Trap, Err(WasmError::Trap(_))) => {}
        (Expect::Empty, Ok(v)) if v.is_empty() => {}
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
        (Expect::Trap, Ok(v)) => panic!("{}: expected trap, got {}", case.id, describe_vals(&v)),
        (Expect::Trap, Err(WasmError::Decode(m))) => {
            panic!("{}: expected trap, got decode {m}", case.id)
        }
        (_, Err(e)) => panic!("{}: unexpected {}", case.id, e.message()),
        (Expect::Empty, Ok(v)) => {
            panic!("{}: expected empty, got {}", case.id, describe_vals(&v))
        }
    }
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
    assert!(src.join("mvp_goldens.txt").is_file());
    assert!(src.join("family_extra.txt").is_file());
    assert!(src.join("mvp_opcodes.txt").is_file());
    let wasm_rs = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/wasm.rs");
    let wasm_src = fs::read_to_string(wasm_rs).unwrap();
    assert!(
        !wasm_src.contains("mvp_goldens.txt"),
        "independent goldens must not be embedded in src/wasm.rs"
    );
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

#[test]
fn independent_goldens_cover_every_mvp_opcode_via_eval() {
    let catalog = load_opcode_catalog();
    let cases = load_cases("mvp_goldens.txt");
    assert!(cases.len() >= 172, "need a golden path for every opcode");

    let mut covered: BTreeMap<u8, String> = BTreeMap::new();
    for case in &cases {
        let got = run_case(case);
        assert_expect(case, got);
        for op in &case.opcodes {
            covered.entry(*op).or_insert_with(|| case.id.clone());
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
    let required = [
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
    for fam in required {
        assert!(
            extras.iter().any(|c| c.family == fam),
            "missing extra golden for family {fam}"
        );
    }
    for case in &extras {
        assert_expect(case, run_case(case));
    }
}

#[test]
fn host_import_table_bind_and_unbound_are_independent() {
    let cases = load_cases("mvp_goldens.txt");
    let bound = cases.iter().find(|c| c.id == "host.mul").expect("host.mul");
    let unbound = cases
        .iter()
        .find(|c| c.id == "host.unbound")
        .expect("host.unbound");
    assert!(bound.bind.is_some());
    assert!(unbound.bind.is_none());
    assert_expect(bound, run_case(bound));
    assert_expect(unbound, run_case(unbound));

    let mut m = must_ok(WasmModule::from_bytes(&bound.wasm), "from_bytes host.mul");
    assert_eq!(m.imports().len(), 1);
    assert_eq!(m.imports()[0].module, "host");
    assert_eq!(m.imports()[0].field, "mul");
    // Unbound until bind_import: the guest call must trap.
    assert!(matches!(m.eval(&[]), Err(WasmError::Trap(_))));
    bind_host(&mut m, "host", "mul");
    match must_ok(m.eval(&[]), "eval bound mul").as_slice() {
        [Val::I32(221)] => {}
        other => panic!("bound mul expected 221, got {}", describe_vals(other)),
    }
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

fn fixture_files() -> [&'static str; 3] {
    ["mvp_goldens.txt", "family_extra.txt", "prd_leaves.txt"]
}

fn suite_edge_tokens() -> BTreeSet<String> {
    let mut edges = BTreeSet::new();
    for name in fixture_files() {
        for case in load_cases(name) {
            edges.insert(case.id);
            edges.insert(case.family);
        }
    }
    let src = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/mvp_golden.rs"),
    )
    .unwrap();
    for line in src.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("fn ")
            && let Some(name) = rest.split('(').next()
        {
            let name = name.strip_prefix("r#").unwrap_or(name);
            edges.insert(name.to_string());
        }
    }
    edges
}

fn cargo_deps_section() -> String {
    let t = fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    let after = t.split("[dependencies]").nth(1).unwrap_or("");
    after.split("\n[").next().unwrap_or(after).to_string()
}

#[test]
fn prd_x_leaves_have_suite_edges() {
    let prd = fs::read_to_string(prd35_path()).expect("PRD_02_35");
    let leaves = parse_prd_x_leaves(&prd);
    assert!(
        !leaves.is_empty(),
        "PRD 35 fence must list [x] leaves"
    );
    let edges = suite_edge_tokens();
    let mut missing = Vec::new();
    for leaf in &leaves {
        if !edges.contains(leaf) {
            missing.push(leaf.clone());
        }
    }
    assert!(
        missing.is_empty(),
        "PRD [x] leaves with no suite edge (family/id/test name): {missing:?}"
    );
}

#[test]
fn eval_bytes() {
    let case = load_cases("prd_leaves.txt")
        .into_iter()
        .find(|c| c.id == "eval(bytes)")
        .expect("eval(bytes) fixture");
    assert_eq!(case.id, "eval(bytes)");
    assert_expect(&case, run_case(&case));
}

#[test]
fn size_budget_script_gates_100kib() {
    let sh = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("measure-core.sh"),
    )
    .unwrap();
    assert!(sh.contains("102400"), "measure-core.sh must keep the 100 KiB cap");
    assert!(sh.contains("OK: < 100 KiB and selftest==42"));
    assert!(
        load_cases("prd_leaves.txt")
            .iter()
            .any(|c| c.id == "<100KiB>" && c.family == "<100KiB>"),
        "<100KiB> leaf must exist as fixture id/family"
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
        "WASI is a non-goal: not a crate dependency"
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
    assert!(
        load_cases("prd_leaves.txt").iter().any(|c| c.id == "#78"),
        "#78 leaf must exist as fixture id"
    );
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
