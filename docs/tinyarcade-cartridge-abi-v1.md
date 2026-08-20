# TinyArcade standard WASM cartridge ABI v1

This document is the converter-facing contract for a TinyArcade cartridge.
A cartridge is an ordinary WebAssembly 1.0 binary module. It has the standard
`.wasm` magic/version, standard sections and standard function imports/exports.
There are no tinyvm-only opcodes and no executable wrapper format.
The v1 runtime rejects an empty cartridge or any whole module above 2 MiB
before manifest/WASM parsing; transports should enforce the same ceiling while
downloading rather than use the runtime as a network buffer.

## Required manifest custom section

Exactly one standard custom section named `tinyarcade.manifest.v1` is required.
After the standard custom-section name, its canonical payload is:

```text
"TAM1"                       4 bytes
abi_version                  u32 little-endian; must be 1
state_version                u32 little-endian; non-zero
game_id_length               u16 little-endian
game_id                      UTF-8; 3..128 [a-z0-9._-] bytes
game_version_length          u16 little-endian
game_version                 UTF-8; 1..64 [A-Za-z0-9._+-] bytes
native_capability_count      u16 little-endian; at most 64
repeated capability:
  namespace_length           u16 little-endian
  namespace                  UTF-8 versioned import module name
```

A native namespace uses a version suffix such as `studio:physics/v1`. The
manifest capability set must equal the set of non-core import module names in
the WASM module and is encoded in ascending byte order. Duplicate or unordered
declarations/imports, undeclared imports and unused declarations are rejected
before instantiation.

## Required guest exports

Every lifecycle export has the exact signature `() -> i32`. Zero means
success; any other result fails the lifecycle operation.

```text
game_abi_version  returns 1
game_init         called once after WASM start
game_tick         called once for a requested frame
game_suspend      submits guest state once
game_resume       loads guest state once
```

The runtime latches a guest trap, invalid lifecycle result or host-budget
violation. A failed instance cannot run another frame and must be discarded by
the app. Bad external snapshot bytes are rejected before guest execution and do
not poison a healthy instance.

## Core import namespace

Core services are optional standard function imports from
`tinyarcade:core/v1`. All values use the portable i32 ABI.

```text
input_bits() -> i32
clock_ms() -> i32
random_u32() -> i32
submit_render(pointer: i32, length: i32) -> i32
submit_audio(pointer: i32, length: i32) -> i32
save_state(pointer: i32, length: i32) -> i32
load_state(pointer: i32, capacity: i32) -> i32
```

`input_bits`, `clock_ms`, `random_u32`, and frame submissions are available
only during init/tick. `save_state` is available only during suspend;
`load_state` only during resume. Guest pointers are ranges in the module's
linear memory and are bounds-checked before native access.

The v1 input bit assignments are:

```text
bit 0 left       bit 1 right      bit 2 up
bit 3 down       bit 4 primary    bit 5 secondary
bit 6 tertiary   bit 7 start      bit 8 menu
```

Time is host-provided monotonic game time, not wall-clock time. RNG is owned by
the host and its state is included in snapshots. Each lifecycle call may submit
render/audio/state at most once and only within host-selected byte ceilings.
Versioned, self-identifying media schemas are defined separately in
[`tinyarcade-media-stream-v1.md`](tinyarcade-media-stream-v1.md). Unknown magic,
unknown versions and malformed records must fail before native rendering or
audio scheduling.

## Native capability imports

Native modules remain standard WASM function imports. The native app must
explicitly register the exact namespace, field and i32 arity before loading a
cartridge. Importing a name does not grant it. Unknown namespaces, wrong
signatures and attempts to replace `tinyarcade:core/v1` fail closed.

Native capability callbacks are app code, not downloaded machine code. A
cartridge never carries dylibs, JIT output, device-side AOT output, JavaScript,
WASI or direct network access.

## Snapshot envelope

The app persists the bytes returned by runtime suspend. The canonical envelope
is:

```text
"TGS1"                       4 bytes
abi_version                  u32 little-endian
state_version                u32 little-endian
game_id_length               u16 little-endian
game_id                      exact manifest UTF-8 bytes
host_rng_state               u32 little-endian
guest_state_length           u32 little-endian
guest_state                  bytes submitted by save_state
```

Resume requires an exact game id, ABI version and state version, consumes the
whole envelope, restores the host RNG and exposes only the guest payload to
`load_state`. Cartridge authors increment `state_version` whenever saved bytes
are no longer backward compatible.

## Converter conformance checklist

1. Emit a valid WebAssembly 1.0 module and exactly one canonical manifest.
2. Export all five exact lifecycle functions.
3. Declare every non-core namespace in the manifest and no unused namespace.
4. Use only registered, versioned i32 capability imports.
5. Treat input/time/RNG as injected deterministic state.
6. Check host-call results and stay within linear-memory and output budgets.
7. Save all game-owned deterministic state; never save native pointers.
8. Pass the runtime black-box suite before a cartridge is eligible for either
   private import or the reviewed catalog.
