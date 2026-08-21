# TinyArcade host profile v1

A TAH1 host profile is a deterministic, callback-free description of one
reviewed app build. It lets converters and creator sites compare a standard
`.wasm` cartridge with the exact TinyArcade ABI, resource ceilings, media
versions and app-compiled native import signatures available in that build.
It is compatibility metadata, not executable code, catalog trust or permission
to install a cartridge.

## Canonical binary

All integers are little-endian. The complete artifact is at most 64 KiB.

```text
"TAH1"                       4 bytes
schema_version                u16; exactly 2
header_length                 u16; exactly 64
game_abi_version              u32; exactly 1
max_cartridge_bytes           u32; exactly 2 MiB
max_table_elems               u32; non-zero aggregate across all tables
max_memory_pages              u32; non-zero, 64 KiB per page
max_steps_per_lifecycle       u64; non-zero
max_render_bytes              u32; non-zero
max_audio_bytes               u32; non-zero
max_state_bytes               u32; non-zero
grid3d_version                u16; exactly 1
indexed2d_version             u16; exactly 1
tones_version                 u16; exactly 1
native_function_count         u16; at most 64
max_call_depth                u32; non-zero defined activations
max_activation_slots          u32; non-zero aggregate live VM slots
reserved                      u32; zero
repeated native function, sorted by module then field bytes:
  module_length               u16
  field_length                u16
  parameter_count             u8; at most 16
  result_count                u8; at most 16
  reserved                    u16; zero
  max_calls_per_lifecycle     u32; 1...64
  module                      canonical authority:module/vN UTF-8
  field                       canonical snake_case UTF-8
```

Decoders also accept the original schema-1, 56-byte header. Because that
artifact predates configurable call resources, it maps deterministically to
512 live defined activations and 1,048,576 aggregate activation slots. Encoders
always emit schema 2. This preserves already published profiles without making
the new limits implicit in future app-build identity.

Duplicate, unordered, malformed, unknown-version and trailing data fail closed.
Changing any limit, media version, namespace, signature or quota changes the
profile bytes and therefore its content hash.

## Compatibility meaning

```text
standard cartridge
  → manifest + lifecycle + standard import validation
  → declared initial memory/table checked against TAH1
  → every native import matched by exact module/field/i32 signature
  → compatible for installation preflight
  → dynamic fuel/output/native-semantic conformance still required
```

The profile cannot prove the amount of fuel, output or call resources a guest
will consume; those values are runtime ceilings and must still be exercised by
converter goldens and reviewed game testing. `max_calls_per_lifecycle` describes host
containment but is not a promise that an arbitrary native callback has the
semantics expected by a game. Catalog signature, origin, revocation and the
App Store external-code release gate remain independent later decisions.

Native functions are implementations already compiled into the app. TAH1
contains only their standard WASM interface and finite call quota. Publishing
TAH1 therefore lets fan converters target a concrete app build without
publishing callbacks, dylibs, JIT/AOT products or tinyvm internals.

## Tool and iOS flow

The core-only default profile can be produced and inspected without an app:

```sh
tinyvm host-profile default ios-build.tahost
tinyvm host-profile inspect ios-build.tahost
tinyvm cartridge check-profile game.wasm ios-build.tahost
```

`check-profile` prints a stable key/value compatibility report. A compatible
cartridge reports `compatibility_issues=0` and `compatible=true`. A valid but
incompatible cartridge still reports its identity and one `issue=` row per
native import, distinguishing a wholly missing function from an exact
parameter/result signature mismatch; it then exits unsuccessfully. Parse,
resource-limit and malformed-profile errors remain separate failures rather
than being flattened into compatibility issues.

Library converters use `HostProfileV1::compatibility_report` for the same
non-executing result. Each issue carries the required module, field and arity,
plus the available arity when that app build has the same named function with
the wrong signature. `inspect_cartridge` remains the fail-fast compatibility
door for existing consumers.

Rust app hosts use `NativeModuleRegistry::host_profile`; C hosts use the
two-stage `tinyarcade_v1_copy_host_profile`; Swift uses
`TinyArcadeHostProfileV1.appBuild`. All three encode the same TAH1 bytes.
`inspect_cartridge`, `tinyarcade_v1_check_cartridge_host_profile` and
`inspectCompatibleCartridge` share the same non-executing compatibility gate.

An app owner publishes the exact artifact as `host-profile-v1.tahost` beside
`catalog-v1.json`; the catalog records its bounded length and SHA-256 so a site
or converter can select an exact target. The iOS client treats those fields as
discovery only and accepts downloaded bytes only when they exactly equal TAH1
generated from the local App build. A creator upload must bind those selected
bytes or digest so a later app build cannot be confused with the one the
converter targeted. Passing TAH1 does not promote a private-user upload into
the official reviewed catalog.
