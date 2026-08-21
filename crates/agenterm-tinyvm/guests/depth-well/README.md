# Depth Well cartridge

This is the first real TinyArcade cartridge. It is an independent falling-
polycube game using only original code, names, colours and presentation. It is
compiled as an ordinary `.wasm` module and imports only `tinyarcade:core/v1`.

Build from the repository root:

```sh
crates/agenterm-tinyvm/build-depth-well-cartridge.sh
```

Validate the finished artifact exactly as a fan converter would:

```sh
cargo run -p agenterm-tinyvm --bin tinyvm -- \
  cartridge check target/tinyvm-depth-well/depth-well-0.1.0.wasm
```

The pinned Rust compiler emits the guest, and Binaryen retains its standard
`memory.copy`/`memory.fill` operations in the bounded TinyArcade v1 Wasm
profile. Install Binaryen so `wasm-opt` is available, or set `WASM_OPT` to its executable.
Build output belongs under `target/`; a cartridge binary is not committed.

Input mapping:

```text
left/right       move across x
up/down          move across y
primary          rotate around x
secondary        rotate around y
tertiary         rotate around z
start            hard drop
menu             host-owned pause/back; never delivered as a game action
```

The guest owns rules and deterministic state. The native app owns camera,
materials, animation, touch/controller mapping, synthesis, pause UI and safe
storage. Render and sound use the versioned streams documented in
`docs/tinyarcade-media-stream-v1.md`.
