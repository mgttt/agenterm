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
At the app-facing Swift layer, every private/reviewed runtime or library open
also requires an explicit external-cartridge distribution policy. Its default
is bundled-only; a signed entry or local file cannot bypass the release gate.

On iOS, `TinyArcadePrivateLibraryV1` is the concrete owner for the private
path. It validates and core-only preflights complete bytes before atomically
installing one canonical `game-id@version.wasm` file. Enumeration and open
recheck identity, byte ceiling and regular-file ownership; symlinks, dangling
symlinks, corrupt modules and oversized replacements fail closed. The bounded
directory is excluded from backup and protected until first authentication.

The bounded transport index and selection-only deep link are specified by
`docs/tinyarcade-catalog-transport-v1.md`. Catalog JSON can display and locate a
reviewed candidate but cannot grant reviewed origin. A deep link resolves only
an already-decoded row; it never downloads or opens executable bytes.

Private import is not a moderation loophole. Importing a file does not create a
catalog row, public URL, discoverable listing, recommendation, rating, sharing
endpoint or upload for other users. A future creator website may build and
download a cartridge to its creator, but publication into the official catalog
is a separate reviewed and signed operation controlled by the app owner.

“Upload to my own app” remains this private-user route even if its transport is
later a personal account, private cloud object or device-to-device transfer.
The recipient identity, bounded download and explicit install consent are owned
by the app layer; the received bytes still acquire only private-user provenance,
stay core-only in v1 and cannot be promoted by changing a URL or metadata. This
personal transport is disabled in an App Store build until the external-code
release gate has the required Apple approval.

## Cartridge compatibility rule

A cartridge stays an ordinary standards-valid WebAssembly module. TinyArcade
does not reserve private opcodes, change section encoding, or require a
tinyvm-specific executable wrapper. The stable platform contract consists of:

- a versioned manifest in a standard custom section;
- standard function imports with canonical versioned module names;
- exact WebAssembly value signatures and explicit finite-work budgets; and
- standard exported lifecycle functions plus bounded platform records.

`tinyarcade:core/v1` is the portable baseline. Future app-compiled native
modules use independent names such as `authority:module/v1`; adding one never
changes the meaning of core v1. A converter can therefore inspect a module's
manifest and import table without executing it, report required capabilities,
and reject an unavailable version before installation. Unknown namespace,
function, version, or signature always fails closed.

Native modules are host implementations, not cartridge payloads. They may be
added to a later reviewed app binary while old cartridges remain ordinary
standard WASM; cartridges request them only through the exact versioned import
table. This keeps fan-authored cartridges portable across conforming runtimes
and lets converters report a missing host profile before upload or install.

Private-user cartridges remain core-only in v1 even if they declare a native
module. The declaration is still useful to creator tools and future migration,
but it is not authority to load native code. Official catalog admission may
only use native modules whose implementation is already compiled into and
registered by the reviewed app build.

This document separates runtime authority; it does not assert that external
WASM execution is presently allowed in an App Store build. The shipping feature
gate is defined by `docs/tinyarcade-app-review-boundary.md`.
