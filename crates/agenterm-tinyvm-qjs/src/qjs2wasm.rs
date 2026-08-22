//! Expression-level lowering to MVP wasm. Not a JS engine and not full JS AOT.

use std::collections::BTreeSet;

use agenterm_tinyvm::WasmError;

enum Atom {
    Int(i32),
    Host(String),
    Local(u32),
}

enum BinOp {
    Add,
    Sub,
}

struct Expr {
    atoms: Vec<Atom>,
    ops: Vec<BinOp>,
}

struct Parsed {
    expr: Expr,
    hosts: Vec<String>,
    n_locals: u32,
}

/// Pack one expression into a standard `.wasm` guest.
///
/// Sugar: decimal integers, `+` / `-`, host names (`g` → import `js.g`),
/// and `$0`/`$1`/… for this-call locals. Anything that needs a JS runtime
/// (calls, functions, objects, `eval`) is rejected.
pub fn qjs2wasm(source: &str) -> Result<Vec<u8>, WasmError> {
    if source.len() > 256 {
        return Err(WasmError::Decode("expression too long"));
    }
    let parsed = parse(source)?;
    Ok(encode(&parsed))
}

fn parse(source: &str) -> Result<Parsed, WasmError> {
    let bytes = source.as_bytes();
    let mut i = 0usize;
    skip_ws(bytes, &mut i);
    if i >= bytes.len() {
        return Err(WasmError::Decode("empty expression"));
    }
    let mut expr = Expr {
        atoms: Vec::new(),
        ops: Vec::new(),
    };
    let mut hosts = BTreeSet::new();
    let mut n_locals = 0u32;
    expr.atoms
        .push(parse_atom(bytes, &mut i, &mut hosts, &mut n_locals)?);
    loop {
        skip_ws(bytes, &mut i);
        if i >= bytes.len() {
            break;
        }
        let op = match bytes[i] {
            b'+' => BinOp::Add,
            b'-' => BinOp::Sub,
            _ => return Err(WasmError::Decode("not an expression subset")),
        };
        i += 1;
        expr.ops.push(op);
        expr.atoms
            .push(parse_atom(bytes, &mut i, &mut hosts, &mut n_locals)?);
    }
    Ok(Parsed {
        expr,
        hosts: hosts.into_iter().collect(),
        n_locals,
    })
}

fn parse_atom(
    bytes: &[u8],
    i: &mut usize,
    hosts: &mut BTreeSet<String>,
    n_locals: &mut u32,
) -> Result<Atom, WasmError> {
    skip_ws(bytes, i);
    if *i >= bytes.len() {
        return Err(WasmError::Decode("truncated expression"));
    }
    match bytes[*i] {
        b'(' | b')' | b'{' | b'}' | b'[' | b']' | b'"' | b'\'' | b'`' | b'.' | b'=' | b';' => {
            Err(WasmError::Decode("not an expression subset"))
        }
        b'$' => {
            *i += 1;
            let n = parse_index(bytes, i)?;
            *n_locals = (*n_locals).max(n + 1);
            Ok(Atom::Local(n))
        }
        b'0'..=b'9' | b'-' => Ok(Atom::Int(parse_i32(bytes, i)?)),
        b'A'..=b'Z' | b'a'..=b'z' | b'_' => {
            let name = parse_ident(bytes, i)?;
            if name == "function" || name == "eval" || name == "return" || name == "var" {
                return Err(WasmError::Decode("full JS is not a converter"));
            }
            hosts.insert(name.clone());
            Ok(Atom::Host(name))
        }
        _ => Err(WasmError::Decode("not an expression subset")),
    }
}

fn parse_ident(bytes: &[u8], i: &mut usize) -> Result<String, WasmError> {
    let start = *i;
    *i += 1;
    while *i < bytes.len() && matches!(bytes[*i], b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_') {
        *i += 1;
    }
    if *i - start > 32 {
        return Err(WasmError::Decode("name too long"));
    }
    core::str::from_utf8(&bytes[start..*i])
        .map(String::from)
        .map_err(|_| WasmError::Decode("name"))
}

fn parse_index(bytes: &[u8], i: &mut usize) -> Result<u32, WasmError> {
    if *i >= bytes.len() || !bytes[*i].is_ascii_digit() {
        return Err(WasmError::Decode("local index"));
    }
    let mut n = 0u32;
    while *i < bytes.len() && bytes[*i].is_ascii_digit() {
        n = n
            .checked_mul(10)
            .and_then(|v| v.checked_add(u32::from(bytes[*i] - b'0')))
            .ok_or(WasmError::Decode("local index"))?;
        *i += 1;
    }
    if n > 16 {
        return Err(WasmError::Decode("local index"));
    }
    Ok(n)
}

fn parse_i32(bytes: &[u8], i: &mut usize) -> Result<i32, WasmError> {
    let neg = if bytes.get(*i) == Some(&b'-') {
        *i += 1;
        true
    } else {
        false
    };
    if *i >= bytes.len() || !bytes[*i].is_ascii_digit() {
        return Err(WasmError::Decode("integer"));
    }
    let mut n = 0i32;
    let mut any = false;
    while *i < bytes.len() && bytes[*i].is_ascii_digit() {
        any = true;
        n = n
            .checked_mul(10)
            .and_then(|v| v.checked_add(i32::from(bytes[*i] - b'0')))
            .ok_or(WasmError::Decode("integer"))?;
        *i += 1;
    }
    if !any {
        return Err(WasmError::Decode("integer"));
    }
    Ok(if neg { -n } else { n })
}

fn skip_ws(bytes: &[u8], i: &mut usize) {
    while *i < bytes.len() && matches!(bytes[*i], b' ' | b'\t' | b'\n' | b'\r') {
        *i += 1;
    }
}

fn encode(parsed: &Parsed) -> Vec<u8> {
    let host_type = 0u32;
    let main_type = if !parsed.hosts.is_empty() && parsed.n_locals > 0 {
        1u32
    } else {
        0u32
    };

    let mut types = Vec::new();
    if parsed.hosts.is_empty() {
        push_uleb(&mut types, 1);
        emit_functype(&mut types, parsed.n_locals, 1);
    } else if parsed.n_locals == 0 {
        push_uleb(&mut types, 1);
        emit_functype(&mut types, 0, 1);
    } else {
        push_uleb(&mut types, 2);
        emit_functype(&mut types, 0, 1);
        emit_functype(&mut types, parsed.n_locals, 1);
    }

    let mut imports = Vec::new();
    push_uleb(&mut imports, parsed.hosts.len() as u32);
    for name in &parsed.hosts {
        push_name(&mut imports, b"js");
        push_name(&mut imports, name.as_bytes());
        imports.push(0x00);
        push_uleb(&mut imports, host_type);
    }

    let mut funcs = Vec::new();
    push_uleb(&mut funcs, 1);
    push_uleb(&mut funcs, main_type);

    let mut exports = Vec::new();
    push_uleb(&mut exports, 1);
    push_name(&mut exports, b"main");
    exports.push(0x00);
    push_uleb(&mut exports, parsed.hosts.len() as u32);

    let mut body = Vec::new();
    body.push(0x00);
    emit_expr(&mut body, parsed);
    body.push(0x0B);
    let mut code = Vec::new();
    push_uleb(&mut code, 1);
    push_uleb(&mut code, body.len() as u32);
    code.extend_from_slice(&body);

    let mut wasm = b"\0asm\x01\x00\x00\x00".to_vec();
    push_section(&mut wasm, 1, &types);
    if !parsed.hosts.is_empty() {
        push_section(&mut wasm, 2, &imports);
    }
    push_section(&mut wasm, 3, &funcs);
    push_section(&mut wasm, 7, &exports);
    push_section(&mut wasm, 10, &code);
    wasm
}

fn emit_functype(out: &mut Vec<u8>, n_params: u32, n_results: u32) {
    out.push(0x60);
    push_uleb(out, n_params);
    for _ in 0..n_params {
        out.push(0x7F);
    }
    push_uleb(out, n_results);
    for _ in 0..n_results {
        out.push(0x7F);
    }
}

fn emit_expr(out: &mut Vec<u8>, parsed: &Parsed) {
    for (idx, atom) in parsed.expr.atoms.iter().enumerate() {
        match atom {
            Atom::Int(n) => {
                out.push(0x41);
                push_sleb_i32(out, *n);
            }
            Atom::Host(name) => {
                let host_index = parsed
                    .hosts
                    .iter()
                    .position(|h| h == name)
                    .expect("host collected");
                out.push(0x10);
                push_uleb(out, host_index as u32);
            }
            Atom::Local(n) => {
                out.push(0x20);
                push_uleb(out, *n);
            }
        }
        if idx > 0 {
            match parsed.expr.ops[idx - 1] {
                BinOp::Add => out.push(0x6A),
                BinOp::Sub => out.push(0x6B),
            }
        }
    }
}

fn push_section(wasm: &mut Vec<u8>, id: u8, payload: &[u8]) {
    wasm.push(id);
    push_uleb(wasm, payload.len() as u32);
    wasm.extend_from_slice(payload);
}

fn push_name(out: &mut Vec<u8>, name: &[u8]) {
    push_uleb(out, name.len() as u32);
    out.extend_from_slice(name);
}

fn push_uleb(out: &mut Vec<u8>, mut value: u32) {
    loop {
        let mut byte = (value & 0x7F) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            break;
        }
    }
}

fn push_sleb_i32(out: &mut Vec<u8>, value: i32) {
    let mut value = i64::from(value);
    loop {
        let mut byte = (value as u8) & 0x7F;
        value >>= 7;
        let done = (value == 0 && byte & 0x40 == 0) || (value == -1 && byte & 0x40 != 0);
        if !done {
            byte |= 0x80;
        }
        out.push(byte);
        if done {
            break;
        }
    }
}
