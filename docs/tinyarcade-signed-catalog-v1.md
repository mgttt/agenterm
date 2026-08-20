# TinyArcade signed catalog v1

Official remote cartridges are content-addressed reviewed objects, not URLs
that the runtime executes on trust. A catalog record is signed with Ed25519 by
an app-bundled offline trust key. Verification binds the signature, exact WASM
length and SHA-256, embedded cartridge identity, ABI version and state version
before `GameRuntime` sees the bytes.

The canonical signature message is:

```text
"TinyArcade signed catalog entry v1\0"
catalog_schema_version       u32 little-endian = 1
game_id_length + game_id     u16 + canonical UTF-8
game_version_length + value  u16 + canonical UTF-8
abi_version                  u32 little-endian
state_version                u32 little-endian
wasm_length                  u64 little-endian
wasm_sha256                  32 bytes
signing_key_id_length + id   u16 + canonical UTF-8
```

The detached signature is exactly 64 Ed25519 bytes. Transport JSON may encode
these fields for discovery, but JSON bytes are never the signature authority.
The canonical binary message above prevents field-order and number-format
ambiguity.

```text
catalog trust gate
├── key id exists in app trust store
├── key has not been revoked
├── content hash has not been revoked
├── Ed25519 signature matches canonical record
├── downloaded length and SHA-256 match record
├── embedded WASM manifest matches record identity/schema
└── only then hand bytes to cartridge validation/runtime
```

Multiple bundled keys permit rotation. Key revocation and content-hash
revocation both fail closed even for an otherwise valid cached object. Private
user imports are a separate policy surface: they still receive byte validation
and resource limits but must not be presented as official reviewed catalog
content.

## Cache and rollback

Verified objects are stored by lowercase SHA-256 under one app-owned cache
directory. Staging creation is exclusive, file contents are flushed, promotion
uses a same-directory rename and the directory is flushed. Symlinks and
oversized/non-regular objects are rejected. One fixed-size activation record
atomically carries both current and previous hashes, so interruption cannot
produce a half-updated current/rollback pair.

Loading current or rolling back is not a trust shortcut: the caller supplies
the matching signed catalog entry and the cache rechecks the object with the
current key/content revocations. Only one previous generation is retained as
active rollback state; later garbage collection may remove unreferenced content
objects under an independent disk budget.
