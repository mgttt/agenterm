# TinyArcade C cartridge authoring v1

Owner: [PRD 02.35](../prd/PRD_02_35_agenterm_tinyvm.md)

Status: development authoring path implemented and executable; external App
Store distribution remains separately gated

A cartridge author does not need Rust, tinyvm internals or a private bytecode.
`build-c-cartridge.sh` compiles freestanding C17 with an ordinary LLVM
WebAssembly backend and linker into a standards-valid `wasm32-unknown-unknown`
module. The canonical converter then appends TinyArcade metadata as one standard
custom section without rewriting the executable prefix.

```text
author.c
  └── clang --target=wasm32-unknown-unknown -nostdlib
      └── raw standard module.wasm
          └── tinyvm cartridge attach-manifest ...
              └── standard cartridge.wasm
                  ├── tinyvm runtime / converter
                  ├── JavaScriptCore WebAssembly oracle
                  └── browser WebAssembly oracle
```

The generic compiler entry point is:

```sh
CLANG=/path/to/wasm-capable-clang \
  crates/agenterm-tinyvm/build-c-cartridge.sh author.c raw.wasm
tinyvm cartridge attach-manifest \
  raw.wasm game-0.1.0.wasm org.example.game 0.1.0 1 1
```

The optional `tinyarcade_guest_v1.h` header declares all core v1 imports and
lifecycle export macros using Clang's standard Wasm attributes. It contributes
no implementation or object code. A cartridge remains freestanding: no libc,
JS glue, WASI, browser DOM or tinyvm runtime library is linked. Its only
platform dependency is the documented versioned import table. Authors may use
another language/toolchain—or write the attributes directly—if it emits the
same standard imports, exports, memory and custom section.

The checked-in `fan-c-cartridge.c` fixture is deliberately small but complete:

- six `tinyarcade:core/v1` imports for input, indexed2d plus its optional
  metadata extension, render and state;
- the exact init/tick/suspend/resume lifecycle;
- one bounded 32×16 indexed frame with a schema-tagged four-byte position
  trailer, plus independent four-byte portable suspend state;
- canonical manifest attachment after linking;
- static descriptor and normal runtime execution;
- suspend into a fresh instance with exact gameplay-state preservation; and
- the same metadata-bearing replay producing exact frames in tinyvm,
  JavaScriptCore and H5.

The fixture is authoring/conformance evidence, not a fourth nostalgia-arcade
product game and not permission to distribute downloaded executable content on
iOS. App Store bundled-only policy remains unchanged. A fan marketplace stays
disabled until its separate Apple approval and product-policy leaves close.
