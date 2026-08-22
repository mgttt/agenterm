# agenterm-tinyvm-qjs

Language skin over [`eval_wasm(data, globals, locals)`](../agenterm-tinyvm).
Not `agenterm-qjs`, not rquickjs, not a JS engine.

`qjs2wasm` lowers a name / arithmetic / zero-arg host-call subset to MVP
`.wasm`. The world is only the two bindings: `globals` (import table) and
`locals` (this call). `eval_qjs` is `eval_wasm(&qjs2wasm(src)?, globals, locals)`.

Commissar demo (from repository root):

```sh
cargo run -p agenterm-tinyvm-qjs --example commissar
```
