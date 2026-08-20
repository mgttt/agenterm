# TinyArcade catalog publisher v1

The offline publisher turns converter-checked standard WebAssembly cartridges
into one complete directory suitable for an HTTPS origin. It does not deploy,
upload, or modify a live site.

```text
author source + standard .wasm + offline Ed25519 seed
    → strict metadata bounds
    → manifest/import/module validation
    → init/tick/media/suspend/resume deterministic replay
    → length + SHA-256 + canonical signed entry
    → public-key verification of the newly signed bytes
    → private staging directory
    → one directory rename
        ├── catalog-v1.json
        └── {game-id}-{game-version}.wasm
```

Build the feature-gated operator tool and publish into a path that does not
already exist:

```sh
cargo run -p agenterm-tinyvm --features catalog-publisher -- \
  catalog build SOURCE.json ED25519-SEED OUTPUT-DIRECTORY
```

`ED25519-SEED` is exactly 32 raw bytes. On Unix it must be a regular file with
no group/other permission bits (normally mode `0600`). The seed is read only to
sign and derive its public key; it is never copied to the staged directory or
printed. It is a TinyArcade catalog key, not an Apple APNs `.p8` key. Keep it
outside the repository and offline. The matching public key and key id are
separately bundled in reviewed app releases so keys can rotate or be revoked.

The source is strict JSON; unknown fields fail. WASM paths are resolved relative
to the source file. Identity and compatibility fields are intentionally absent:
they are derived from each cartridge's canonical embedded manifest so a site
operator cannot relabel executable bytes.

```json
{
  "schema_version": 1,
  "catalog_id": "com.partnernet.tinyarcade",
  "signing_key_id": "catalog-2026-01",
  "games": [
    {
      "wasm": "cartridges/depth-well.wasm",
      "title": "Depth Well",
      "summary": "Drop polycubes into a deep well.",
      "localizations": {
        "zh-Hans": {
          "title": "深井方块",
          "summary": "把立体方块落入深井。"
        }
      }
    }
  ]
}
```

Games are sorted by embedded `game_id`; object names, lowercase hashes,
canonical base64 signatures and pretty JSON are deterministic. An existing
output is never overwritten. Any validation, signing, verification, encoding,
or write failure removes the private staging directory and leaves no visible
publication directory.

The published JSON has discovery authority only. An app must still download
the exact same-origin object under byte limits and pass its signed entry plus
bytes through the live trust/revocation store and verified cache before opening
the runtime. Private user imports do not use this official-review signature;
they remain a separate core-only library route.

Native extensions remain ordinary standard WASM function imports. A fan-made
converter can target `tinyarcade:core/v1` and later documented
`authority:module/vN` namespaces without knowing tinyvm internals. Official
publication additionally requires that the app has reviewed and bundled every
declared native module with an exact signature and finite-work policy.
