//! Expression sugar → MVP wasm → tinyvm [`eval_wasm`].
//!
//! [`qjs2wasm`] lowers a name/op/host-call subset. The world lives in the
//! two [`eval_wasm`] bindings: `globals` (import table) and `locals` (this call).
//! Full JS is not a converter and is not this crate.
//!
//! Concept (Cloudflare Workers, design only — not V8/workerd/isolate/engine):
//! one untrusted guest per slot, language skin over the host door, limits in
//! the tinyvm core; container/OS is a later wrapping, not this crate.
//!
//! Commissar demo: `cargo run -p agenterm-tinyvm-qjs --example commissar`

use agenterm_tinyvm::{HostGlobal, Val, WasmError, eval_wasm};

mod qjs2wasm;
pub use qjs2wasm::qjs2wasm;

/// [`qjs2wasm`] then [`eval_wasm`]: `eval_wasm(&qjs2wasm(source)?, globals, locals)`.
///
/// ```
/// use agenterm_tinyvm::{HostGlobal, Val};
/// use agenterm_tinyvm_qjs::eval_qjs;
/// let g = [HostGlobal::new("js", "g", Val::I32(40))];
/// let got = eval_qjs("g()+$0", &g, &[Val::I32(2)]);
/// assert!(matches!(got, Ok(vals) if matches!(vals.as_slice(), [Val::I32(42)])));
/// ```
pub fn eval_qjs(
    source: &str,
    globals: &[HostGlobal<'_>],
    locals: &[Val],
) -> Result<Vec<Val>, WasmError> {
    eval_wasm(&qjs2wasm(source)?, globals, locals)
}
