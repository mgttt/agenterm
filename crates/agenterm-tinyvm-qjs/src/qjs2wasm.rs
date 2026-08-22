//! Expression-level lowering to MVP wasm. Not a JS engine and not full JS AOT.
//!
//! Subset: names, integer arithmetic, grouping, and zero-arg host calls.
//! The world is only the two [`eval_wasm`](agenterm_tinyvm::eval_wasm) bindings:
//! host names → import table (`globals`), `$N` → this-call `locals`.

use std::collections::BTreeSet;

use agenterm_tinyvm::WasmError;

enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

enum Expr {
    Int(i32),
    Host(String),
    Local(u32),
    Neg(Box<Expr>),
    Bin(BinOp, Box<Expr>, Box<Expr>),
}

struct Parsed {
    expr: Expr,
    hosts: Vec<String>,
    n_locals: u32,
}

/// Pack one expression into a standard `.wasm` guest.
///
/// Sugar: decimal integers; `+` `-` `*` `/` `%`; grouping `()`; host names
/// (`g` or `g()` → import `js.g`); `$0`/`$1`/… for this-call locals.
/// Host calls take no arguments — that would be a third world.
/// Anything that needs a JS runtime is rejected.
pub fn qjs2wasm(source: &str) -> Result<Vec<u8>, WasmError> {
    if source.len() > 256 {
        return Err(WasmError::Decode("expression too long"));
    }
    let parsed = parse(source)?;
    Ok(encode(&parsed))
}

fn parse(source: &str) -> Result<Parsed, WasmError> {
    let mut p = Parser {
        bytes: source.as_bytes(),
        i: 0,
    };
    p.skip_ws();
    if p.i >= p.bytes.len() {
        return Err(WasmError::Decode("empty expression"));
    }
    let expr = p.expr()?;
    p.skip_ws();
    if p.i < p.bytes.len() {
        return Err(WasmError::Decode("not an expression subset"));
    }
    let mut hosts = BTreeSet::new();
    let mut n_locals = 0u32;
    collect(&expr, &mut hosts, &mut n_locals);
    Ok(Parsed {
        expr,
        hosts: hosts.into_iter().collect(),
        n_locals,
    })
}

struct Parser<'a> {
    bytes: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn skip_ws(&mut self) {
        while self.i < self.bytes.len()
            && matches!(self.bytes[self.i], b' ' | b'\t' | b'\n' | b'\r')
        {
            self.i += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.i).copied()
    }

    fn bump(&mut self) {
        self.i += 1;
    }

    fn expr(&mut self) -> Result<Expr, WasmError> {
        let mut left = self.term()?;
        loop {
            self.skip_ws();
            let op = match self.peek() {
                Some(b'+') => BinOp::Add,
                Some(b'-') => BinOp::Sub,
                _ => break,
            };
            self.bump();
            let right = self.term()?;
            left = Expr::Bin(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn term(&mut self) -> Result<Expr, WasmError> {
        let mut left = self.unary()?;
        loop {
            self.skip_ws();
            let op = match self.peek() {
                Some(b'*') => BinOp::Mul,
                Some(b'/') => BinOp::Div,
                Some(b'%') => BinOp::Rem,
                _ => break,
            };
            self.bump();
            let right = self.unary()?;
            left = Expr::Bin(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Expr, WasmError> {
        self.skip_ws();
        if self.peek() == Some(b'-') {
            self.bump();
            return Ok(Expr::Neg(Box::new(self.unary()?)));
        }
        self.atom()
    }

    fn atom(&mut self) -> Result<Expr, WasmError> {
        self.skip_ws();
        match self.peek() {
            None => Err(WasmError::Decode("truncated expression")),
            Some(b'(') => {
                self.bump();
                let inner = self.expr()?;
                self.skip_ws();
                if self.peek() != Some(b')') {
                    return Err(WasmError::Decode("not an expression subset"));
                }
                self.bump();
                Ok(inner)
            }
            Some(b'{' | b'}' | b'[' | b']' | b'.') => {
                Err(WasmError::Decode("third world; world is in two bindings"))
            }
            Some(
                b')' | b'"' | b'\'' | b'`' | b'=' | b';' | b',' | b'!' | b'&' | b'|' | b'<' | b'>'
                | b'?',
            ) => Err(WasmError::Decode("not an expression subset")),
            Some(b'$') => {
                self.bump();
                Ok(Expr::Local(self.index()?))
            }
            Some(b'0'..=b'9') => Ok(Expr::Int(self.unsigned_i32()?)),
            Some(b'A'..=b'Z' | b'a'..=b'z' | b'_') => self.host_name(),
            Some(_) => Err(WasmError::Decode("not an expression subset")),
        }
    }

    fn host_name(&mut self) -> Result<Expr, WasmError> {
        let name = self.ident()?;
        if is_js_keyword(&name) {
            return Err(WasmError::Decode("full JS is not a converter"));
        }
        self.skip_ws();
        match self.peek() {
            Some(b'(') => {
                self.bump();
                self.skip_ws();
                if self.peek() != Some(b')') {
                    return Err(WasmError::Decode(
                        "host call takes no args; world is in two bindings",
                    ));
                }
                self.bump();
            }
            Some(b'.' | b'[') => {
                return Err(WasmError::Decode("third world; world is in two bindings"));
            }
            _ => {}
        }
        Ok(Expr::Host(name))
    }

    fn ident(&mut self) -> Result<String, WasmError> {
        let start = self.i;
        self.bump();
        while self.i < self.bytes.len()
            && matches!(self.bytes[self.i], b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_')
        {
            self.i += 1;
        }
        if self.i - start > 32 {
            return Err(WasmError::Decode("name too long"));
        }
        core::str::from_utf8(&self.bytes[start..self.i])
            .map(String::from)
            .map_err(|_| WasmError::Decode("name"))
    }

    fn index(&mut self) -> Result<u32, WasmError> {
        if !self.peek().is_some_and(|b| b.is_ascii_digit()) {
            return Err(WasmError::Decode("local index"));
        }
        let mut n = 0u32;
        while let Some(b) = self.peek() {
            if !b.is_ascii_digit() {
                break;
            }
            n = n
                .checked_mul(10)
                .and_then(|v| v.checked_add(u32::from(b - b'0')))
                .ok_or(WasmError::Decode("local index"))?;
            self.bump();
        }
        if n > 16 {
            return Err(WasmError::Decode("local index"));
        }
        Ok(n)
    }

    fn unsigned_i32(&mut self) -> Result<i32, WasmError> {
        if !self.peek().is_some_and(|b| b.is_ascii_digit()) {
            return Err(WasmError::Decode("integer"));
        }
        let mut n = 0i32;
        let mut any = false;
        while let Some(b) = self.peek() {
            if !b.is_ascii_digit() {
                break;
            }
            any = true;
            n = n
                .checked_mul(10)
                .and_then(|v| v.checked_add(i32::from(b - b'0')))
                .ok_or(WasmError::Decode("integer"))?;
            self.bump();
        }
        if !any {
            return Err(WasmError::Decode("integer"));
        }
        Ok(n)
    }
}

fn is_js_keyword(name: &str) -> bool {
    matches!(
        name,
        "function"
            | "eval"
            | "return"
            | "var"
            | "let"
            | "const"
            | "class"
            | "new"
            | "this"
            | "typeof"
            | "instanceof"
            | "import"
            | "export"
            | "async"
            | "await"
            | "yield"
            | "with"
            | "delete"
            | "undefined"
            | "null"
            | "true"
            | "false"
    )
}

fn collect(expr: &Expr, hosts: &mut BTreeSet<String>, n_locals: &mut u32) {
    match expr {
        Expr::Int(_) => {}
        Expr::Host(name) => {
            hosts.insert(name.clone());
        }
        Expr::Local(n) => {
            *n_locals = (*n_locals).max(n + 1);
        }
        Expr::Neg(inner) => collect(inner, hosts, n_locals),
        Expr::Bin(_, a, b) => {
            collect(a, hosts, n_locals);
            collect(b, hosts, n_locals);
        }
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
    emit_expr(&mut body, parsed, &parsed.expr);
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

fn emit_expr(out: &mut Vec<u8>, parsed: &Parsed, expr: &Expr) {
    match expr {
        Expr::Int(n) => {
            out.push(0x41);
            push_sleb_i32(out, *n);
        }
        Expr::Host(name) => {
            let host_index = parsed
                .hosts
                .iter()
                .position(|h| h == name)
                .expect("host collected");
            out.push(0x10);
            push_uleb(out, host_index as u32);
        }
        Expr::Local(n) => {
            out.push(0x20);
            push_uleb(out, *n);
        }
        Expr::Neg(inner) => {
            out.push(0x41);
            push_sleb_i32(out, 0);
            emit_expr(out, parsed, inner);
            out.push(0x6B);
        }
        Expr::Bin(op, a, b) => {
            emit_expr(out, parsed, a);
            emit_expr(out, parsed, b);
            out.push(match op {
                BinOp::Add => 0x6A,
                BinOp::Sub => 0x6B,
                BinOp::Mul => 0x6C,
                BinOp::Div => 0x6D,
                BinOp::Rem => 0x6F,
            });
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
