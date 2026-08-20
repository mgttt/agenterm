# TinyArcadeRuntime Swift package

This generated package is the iOS app integration artifact. It contains the
device/simulator `TinyArcade.xcframework` and the main-actor Swift ownership,
media-decoding and signed-catalog wrapper as one `TinyArcadeRuntime` library
product.

Use `tickMedia` for the discriminated `grid3d/v1` or `indexed2d/v1` render
frame. Existing Depth Well integrations may keep using the `grid3d/v1`-only
`tick` convenience.

For indexed cartridges, `TinyArcadeIndexed2DFrame.makeCGImage()` provides an
exact sRGB RGBA image and `TinyArcadeIndexed2DView` is a ready-to-layout UIKit
surface with aspect-fit, nearest-neighbour presentation. Apps that own a Metal
renderer can instead consume the validated palette and pixel plane directly.

For audio feedback, pass `TinyArcadeMediaFrame.tones` to
`TinyArcadeTonePlayer.play(_:)`. The default player uses a mixing `.ambient`
audio session; use `TinyArcadeTonePlayer(managesAudioSession: false)` when the
app already owns session policy. Forward interruption-began events, call
`stop()` when feedback should be cut immediately, and call `deactivate()` when
leaving the game surface. The SDK deliberately does not resume interrupted
gameplay tones or choose haptics for the app.

Reviewed downloads should be handed to `TinyArcadeCartridgeCacheV1.activate`
only after the app has received the complete response. The cache verifies the
signed entry and atomically selects it; `loadActive` and `rollback` recheck live
revocations before returning executable bytes. The cache performs no network
request, and private-user imports remain a separate origin and storage policy.

Decode official lobby metadata with `TinyArcadeCatalogV1.decode`. It bounds the
document, game count, strings, localizations, signed-entry encodings and
same-origin `{name}-{version}.wasm` filename. A generated
`tinyarcade://game/<game-id>` URL only selects an existing row; it never
downloads, activates or opens a cartridge. JSON discovery is not a substitute
for cache/trust verification.

Generate a self-contained directory from the repository root:

```sh
rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios
crates/agenterm-tinyvm/build-swift-package.sh \
  dist/TinyArcadeRuntimePackage
```

An app may then add that directory as a local Swift package and depend on the
`TinyArcadeRuntime` product. The generated directory is a build artifact; this
template, the Swift source and Rust/C sources remain authoritative.
