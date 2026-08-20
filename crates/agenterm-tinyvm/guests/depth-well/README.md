# Depth Well cartridge

This is the first real TinyArcade cartridge. It is an independent falling-
polycube game using only original code, names, colours and presentation. It is
compiled as an ordinary `.wasm` module and imports only `tinyarcade:core/v1`.

Build from the repository root:

```sh
crates/agenterm-tinyvm/build-depth-well-cartridge.sh
```

The pinned Rust compiler emits the guest, then Binaryen lowers compiler-added
`memory.copy`/`memory.fill` operations to strict WebAssembly MVP. Install
Binaryen so `wasm-opt` is available, or set `WASM_OPT` to its executable.
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
