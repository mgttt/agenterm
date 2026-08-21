# TinyArcade standard WASM cartridge ABI v1

This document is the converter-facing contract for a TinyArcade cartridge.
A cartridge is an ordinary standard WebAssembly binary module. It has the standard
`.wasm` magic/version, standard sections and standard function imports/exports.
There are no tinyvm-only opcodes and no executable wrapper format.
The v1 runtime rejects an empty cartridge or any whole module above 2 MiB
before manifest/WASM parsing; transports should enforce the same ceiling while
downloading rather than use the runtime as a network buffer.

## Standard module and decode complexity

Standard non-custom sections must appear at most once, in WebAssembly-specified
order, and each parser must consume its complete declared payload. DataCount
(section id 12) is correctly ordered before code rather than numerically after
data. Unknown
non-custom section ids, duplicate/out-of-order sections and trailing bytes fail
before a `Module` exists. Custom sections may appear between standard sections;
they do not change ordering.

The v1 executable profile is MVP scalar instructions plus mutable globals,
tables/`call_indirect`, and the standard bulk-memory proposal over the single
memory and MVP funcref table. It accepts active/passive data and index-encoded
funcref element segments, `memory.init`, `data.drop`, `memory.copy`,
`memory.fill`, `table.init`, `elem.drop` and `table.copy`. DataCount is checked
against the data section and is mandatory when code uses a data-segment
instruction. Reference-typed element expressions, reference types, multivalue,
SIMD, exceptions, threads and multiple memories remain outside v1 and fail
loudly at load time. This is feature negotiation by converter profile: future
runtimes may add standard proposals without changing the `.wasm` container or
inventing tinyvm-only opcodes.

The segment flags, DataCount ordering, instruction immediates and drop
semantics follow the WebAssembly specification's
[bulk-memory proposal](https://github.com/WebAssembly/spec/blob/main/proposals/bulk-memory-operations/Overview.md).

Bulk copy/fill first bounds-check every range, then charge deterministic fuel
proportional to length (one unit per 16 bytes, in addition to the instruction
unit), then mutate memory. An out-of-bounds or fuel trap therefore cannot leave
a partial copy/fill behind. `memory.copy` has the standard overlap-safe memmove
semantics; `memory.fill` uses the low byte of its i32 value.
Table initialization/copy similarly checks both ranges and charges one fuel
unit per funcref before mutation. Passive data and element liveness is owned by
each instance: dropping a segment in one game cannot affect a sibling instance.
Active and declarative segments are unavailable to init after instantiation,
while a dropped segment still permits only source offset zero with length zero.

One module may materialize at most 262,144 allocation-amplifying decode records
in total. The shared count covers section entries, function parameter/result
types, local values, decoded instructions, element indices and `br_table`
targets. Raw custom/name/data bytes remain covered by the 2 MiB cartridge limit
and their section bounds. Allocation-amplifying guest-declared vectors are
budgeted before fallible reservation, so a tiny module declaring billions of
targets or locals returns a decode error before asking iOS to reserve that
guest-selected memory. This is a fixed TinyArcade ABI v1 compiler profile, not
a private bytecode extension: accepted files remain ordinary standards-valid
`.wasm`, and converter/runtime checks use the same gate.

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

A native namespace uses the canonical form `authority:module/vN`, such as
`com.example:physics/v1`. Authority is a lower-case ASCII name and may use
dots for reverse-DNS ownership; module uses lower-case ASCII letters, digits,
`_` or `-`; `N` is a positive decimal major version without a leading zero.
Imported native function fields use lower-case ASCII `snake_case`, begin with a
letter and are at most 64 bytes. A breaking signature or semantic change must
use a new namespace major version; old namespaces are never silently rebound.
The manifest capability set must equal the set of non-core import module names in
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
indexed2d_version() -> i32
submit_render(pointer: i32, length: i32) -> i32
submit_audio(pointer: i32, length: i32) -> i32
save_state(pointer: i32, length: i32) -> i32
load_state(pointer: i32, capacity: i32) -> i32
```

`input_bits`, `clock_ms`, `random_u32`, and frame submissions are available
only during init/tick. `save_state` is available only during suspend;
`load_state` only during resume. Guest pointers are ranges in the module's
linear memory and are bounds-checked before native access.

A cartridge that emits `tinyarcade:indexed2d/v1` must import
`indexed2d_version`; the current host returns 1. The import is the load-time
compatibility declaration: an older runtime rejects the unknown core import
before instantiation, and the current runtime traps an indexed frame from a
cartridge that omitted it. A cartridge should check the returned version
during init before relying on that media schema.

The v1 input bit assignments are:

```text
bit 0 left       bit 1 right      bit 2 up
bit 3 down       bit 4 primary    bit 5 secondary
bit 6 tertiary   bit 7 start      bit 8 menu
```

Time is host-provided monotonic game time, not wall-clock time. RNG is owned by
the host and its state is included in snapshots. Each lifecycle call may submit
render/audio/state at most once and only within host-selected byte ceilings.
The runtime rejects bits outside 0...8 and a clock below the preceding
successful tick before guest execution. This host-input error does not latch or
mutate the cartridge; the same or a later clock can continue. Successful resume
starts a new host-clock validation epoch because portable runtime snapshots do
not own the app's clock; the iOS snapshot envelope stores that clock beside the
runtime bytes.
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

This deliberately leaves room for future app-native modules without coupling a
cartridge to tinyvm. A native module is compiled into a reviewed app build and
publishes a documented, versioned standard function-import namespace; the
`.wasm` cartridge contains only calls to that interface. Creator tools and
converters can therefore target a declared host profile, inspect compatibility
without running the game, and keep emitting the same standards-valid module for
another conforming interpreter. A new native implementation, optional function,
or breaking behavior must not reinterpret an existing namespace/version.

The public iOS ABI registers at most 64 exact functions per runtime and limits
each to 16 i32 parameters, 16 i32 results and a host-selected 1...64 calls per
lifecycle. The quota resets for init/tick/suspend/resume and is charged before
dispatch, so an over-budget call never reaches app code. A callback runs
synchronously on the runtime owner thread, may read/write guest linear memory
only during that call, and latches the cartridge on callback failure. This is a compatibility
door, not an ambient native API: a host should expose the smallest versioned
module needed by the reviewed game and may decline a manifest capability even
when the app binary implements it.

Native callback implementations are trusted, app-compiled parts of that module
contract. They must bound every guest-derived offset/count and complete without
blocking. A synchronous borrowed-memory callback cannot be safely killed by a
wall-clock timer; post-return timing is telemetry, not containment. Cartridge
containment therefore combines WASM fuel with the pre-dispatch native call
quota, while each shipped native module owns a deterministic finite-work rule.

The exact function import table is the machine-readable interface descriptor:
module namespace, field, parameter count and result count all come from normal
WASM sections. Rust `CartridgeDescriptor::inspect`, `tinyvm cartridge inspect`
and iOS `TinyArcadeCartridgeDescriptorV1.inspect` all use the same structural
validator without instantiating the module, running its start function or
calling guest code. Converters should preserve standard WASM imports and use
this table to report missing host modules; they must not rewrite imports into
private opcodes. A capability declaration is a compatibility requirement, not
an entitlement: cartridge origin and host policy still decide whether a module
is registered.

The C ABI v1.8 two-stage copy function emits the following canonical host-side
descriptor. `TAD1` is inspection output, not a cartridge wrapper and never
replaces the original standard `.wasm` bytes.

```text
"TAD1"                       4 bytes
schema_version               u16 little-endian; 1
header_length                u16 little-endian; 32
abi_version                  u32 little-endian
state_version                u32 little-endian
game_id_length               u16 little-endian
game_version_length          u16 little-endian
native_capability_count      u16 little-endian; at most 64
function_import_count        u16 little-endian; at most 72
wasm_length                  u32 little-endian; exact inspected bytes
reserved                     u32 zero
game_id                      exact UTF-8 bytes
game_version                 exact UTF-8 bytes
repeated native capability:
  namespace_length           u16 little-endian
  namespace                  exact UTF-8 bytes
repeated function import:
  module_length              u16 little-endian
  field_length               u16 little-endian
  parameter_count            u8; at most 16
  result_count               u8; at most 16
  class                      u8; 0 core, 1 native
  reserved                   u8 zero
  module                     exact UTF-8 bytes
  field                      exact UTF-8 bytes
```

Descriptor success proves structural ABI compatibility only. Runtime-specific
memory/step/media limits, native registry availability, catalog trust and guest
initialization remain later independent gates.

## Manifest authoring

An ordinary WebAssembly producer does not need linker-specific custom-section
syntax. The converter CLI can attach the canonical section after compilation:

```text
tinyvm cartridge attach-manifest INPUT.wasm OUTPUT.wasm GAME_ID GAME_VERSION ABI_VERSION STATE_VERSION
```

The command first parses the input as standard WASM, derives the sorted unique
native capability set from its non-core function-import namespaces, appends one
standard custom section, and runs the complete static cartridge descriptor
validator before publishing. The capability list is deliberately not a CLI
argument: the standard import table is its sole source of truth. Input bytes are
preserved as the exact prefix of the output; an existing manifest, incompatible
lifecycle/import contract, output over 2 MiB or existing output path fails
without publishing or overwriting an artifact. Run this as the final post-link,
post-optimization step so a producer optimizer cannot strip the manifest.

A fan-facing converter should publish both the resulting `.wasm` and its
machine-readable descriptor/compatibility report. Its target is an explicit
TinyArcade host profile: core ABI version, media versions, exact native
namespace/function signatures and resource ceilings. It must not probe tinyvm
implementation details or rewrite missing host functions into private opcodes.
The canonical app-build profile is
[`tinyarcade-host-profile-v1.md`](tinyarcade-host-profile-v1.md); its static
check deliberately remains separate from dynamic fuel/output conformance.

Library-based converters may use `CartridgeManifest::append_to_wasm` for the
same deterministic canonical encoding. That low-level method validates the
manifest and section framing and refuses to rewrite an existing manifest; the
caller must still run `CartridgeDescriptor::inspect` to prove the complete game
contract before distribution.

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

1. Emit a standards-valid WebAssembly module within the v1 executable profile,
   then attach exactly one canonical
   manifest without rewriting its executable sections.
2. Export all five exact lifecycle functions.
3. Derive every manifest capability from the standard non-core import table;
   never maintain a second hand-written capability list.
4. Use only registered, versioned i32 capability imports.
5. Treat input/time/RNG as injected deterministic state.
6. Check host-call results and stay within linear-memory and output budgets.
7. Save all game-owned deterministic state; never save native pointers.
8. Pass the runtime black-box suite before a cartridge is eligible for either
   private import or the reviewed catalog.
