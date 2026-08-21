# tinyvm optional iOS WASI host

Owner: [PRD 02.35](../prd/PRD_02_35_agenterm_tinyvm.md)

Status: iOS Simulator container path implemented; physical-iPhone evidence open

The `ios-wasi-host` Cargo feature and `TinyWasiHost.xcframework` are a separate
development/embedding artifact for standard WASI commands. They do not enter
the default TinyArcade XCFramework, Swift package, bundled game ABI or
nostalgia-arcade App target.

## Boundary

```text
Swift-selected App directory
└── tinyvm_wasi_host_v1_run
    ├── bounded Wasm decode/execution configuration
    ├── StdHostBackend ambient open exactly once
    ├── HostContext virtual preopen /save
    ├── WasiPreview1 exact 16-import subset
    └── standard _start
        ├── normal empty return, or
        └── typed proc_exit code
```

The one-shot C ABI accepts borrowed Wasm bytes, an App-owned UTF-8 directory
path, explicit resource limits and two exit outputs. It runs `_start` with no
arguments. A normal empty return sets `did_exit=0`; an accepted `proc_exit`
sets `did_exit=1` and its exact unsigned code. Decode faults, guest traps,
storage failures, invalid arguments and a caught Rust panic remain distinct
status values.

The guest sees only `/save`. The host does not infer Documents/Caches, expose a
container root, terminate the App process, add network access or load native
code. This is not the TinyArcade game lifecycle ABI and does not turn the App
into an H5 or general mini-app host.

## Artifact separation

- Default: `build-xcframework.sh` uses `ios-c-api` and `include/`.
- Optional host: `build-wasi-host-xcframework.sh` uses `ios-wasi-host` and
  `include-wasi-host/`.
- The optional header/module is `TinyWasiHost`; default consumers import only
  `TinyArcade` and cannot see the WASI host symbols.

## Evidence

`smoke-ios-wasi-host.sh` independently compiles a WAT command, builds device and
universal-simulator slices, checks the C header, links Swift against the arm64
simulator slice and runs it inside a booted iPhone Simulator. Swift supplies a
fresh temporary App directory. The command opens `/save/slot.bin`, writes
`hello`, closes it and calls `proc_exit(7)`; Swift verifies all three externally
observable outcomes.

The normal TinyArcade bridge gate remains unchanged and a negative symbol/header
check keeps `tinyvm_wasi_host_v1_` out of its default XCFramework. A physical
iPhone run remains required before claiming device container evidence.
