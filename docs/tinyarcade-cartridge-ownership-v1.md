# TinyArcade cartridge ownership v1

The same standards-valid `.wasm` format has three deliberately different app
policy surfaces. Runtime origin is fixed when an instance opens and is queryable
through Rust, C and Swift; a caller cannot relabel an already-opened instance.

```text
cartridge origin
├── bundled
│   ├── shipped inside the signed app bundle
│   ├── app release review owns its exact bytes
│   └── only origin eligible for explicitly compiled native registries
├── official-reviewed
│   ├── discovered through the app's curated catalog
│   ├── exact Ed25519 record + hash + manifest verification required
│   ├── live key/content revocation applies to cached bytes
│   └── never created from a user's private import
└── private-user
    ├── user explicitly selects a local file for their own library
    ├── standard byte validation and resource ceilings still apply
    ├── only tinyarcade:core/v1 imports are available
    ├── no native module, guest network or public upload authority
    └── UI must label it private and must not imply catalog review
```

`GameRuntime::from_bytes` / `tinyarcade_v1_open` are the bundled path.
`from_reviewed_bytes` / `tinyarcade_v1_open_reviewed` require the signed trust
gate. `from_private_bytes` / `tinyarcade_v1_open_private` intentionally create
private provenance and instantiate with an empty native capability registry.

The bounded transport index and selection-only deep link are specified by
`docs/tinyarcade-catalog-transport-v1.md`. Catalog JSON can display and locate a
reviewed candidate but cannot grant reviewed origin. A deep link resolves only
an already-decoded row; it never downloads or opens executable bytes.

Private import is not a moderation loophole. Importing a file does not create a
catalog row, public URL, discoverable listing, recommendation, rating, sharing
endpoint or upload for other users. A future creator website may build and
download a cartridge to its creator, but publication into the official catalog
is a separate reviewed and signed operation controlled by the app owner.

This document separates runtime authority; it does not assert that external
WASM execution is presently allowed in an App Store build. The shipping feature
gate is defined by `docs/tinyarcade-app-review-boundary.md`.
