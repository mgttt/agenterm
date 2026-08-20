# TinyArcade catalog transport v1

The official lobby index is bounded discovery metadata. It is not executable
authority and its JSON bytes are not signed as a substitute for a cartridge
record. Selecting an item yields one `TinyArcadeReviewedCatalogEntry`; the
downloaded bytes must still pass the signed-entry trust store and verified
cache before a reviewed runtime can open them.

```text
catalog JSON
├── shows bounded title/summary/localizations
├── resolves one same-origin cartridge filename
├── carries the detached signed-entry fields
└── never activates code
    └── complete response
        → signed entry verification
        → atomic verified cache
        → reviewed runtime open
```

## Document

The UTF-8 JSON document is at most 1 MiB and contains 1...256 current games.
One `game_id` appears at most once, so a deep link cannot ambiguously select
among versions. Numbers are JSON integers within their declared unsigned
ranges.

```json
{
  "schema_version": 1,
  "catalog_id": "com.example.tinyarcade",
  "games": [
    {
      "game_id": "com.example.paddle-guard",
      "game_version": "0.1.0",
      "title": "Paddle Guard",
      "summary": "Defend the field.",
      "localizations": {
        "zh-Hans": {
          "title": "护盾弹球",
          "summary": "守住球场。"
        }
      },
      "cartridge": "paddle-guard-0.1.0.wasm",
      "abi_version": 1,
      "state_version": 1,
      "wasm_length": 5280,
      "wasm_sha256": "<64-lowercase-hex-characters>",
      "signing_key_id": "catalog-2026-a",
      "signature": "<canonical-base64-of-64-bytes>"
    }
  ]
}
```

`catalog_id`, `game_id` and `signing_key_id` use bounded lowercase ASCII
identifier characters (`a-z`, `0-9`, `.`, `_`, `-`). Versions use bounded
ASCII alphanumerics plus `.`, `_`, `+`, `-`. Title and summary are nonblank,
with 256-byte and 1,024-byte UTF-8 ceilings. At most 16 BCP-47-shaped language
tags are accepted per game. Lookup tries an exact case-insensitive tag and then
removes trailing subtags before falling back to the default text.

`wasm_sha256` is exactly 32 bytes encoded as lowercase hex. `signature` is the
canonical Base64 encoding of exactly 64 bytes. These fields reconstruct the
signed record specified by `docs/tinyarcade-signed-catalog-v1.md`; display text
and transport location do not gain signature authority merely by sharing the
same JSON object. Unknown JSON members are transport extensions and cannot
change the fields passed to the trust gate.

## Cartridge URL

The app supplies an HTTPS directory URL, such as
`https://partnernetsoftware.com/wasm/`. `cartridge` is one ASCII filename, not a
URL: it contains no slash, traversal, query, fragment or percent-encoded path,
ends in `-<game_version>.wasm`, and resolves on the same scheme/host/port as the
directory. A configurable positive per-cartridge ceiling defaults to 8 MiB.

The conventional publication layout is therefore:

```text
https://partnernetsoftware.com/wasm/
├── catalog-v1.json
├── depth-well-0.1.0.wasm
└── paddle-guard-0.1.0.wasm
```

Transport code owns HTTPS policy, redirects, response status, timeout and the
download byte ceiling. TinyVM owns none of those network decisions and a guest
has no network import.

## Deep links

The stable selection form is `tinyarcade://game/<game_id>`. It must contain
exactly one path component and no user info, port, query or fragment. Resolution
returns the already-decoded catalog item only. In particular, flags such as
`?run=1` are rejected and resolving a link performs no network, cache or runtime
operation. The lobby remains responsible for presenting the selected item and
starting any reviewed acquisition flow explicitly.

Private-user imports are not catalog rows and do not acquire public discovery,
reviewed labels or shareable deep links through this format.
