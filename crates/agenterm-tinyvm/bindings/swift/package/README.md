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

Generate a self-contained directory from the repository root:

```sh
rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios
crates/agenterm-tinyvm/build-swift-package.sh \
  dist/TinyArcadeRuntimePackage
```

An app may then add that directory as a local Swift package and depend on the
`TinyArcadeRuntime` product. The generated directory is a build artifact; this
template, the Swift source and Rust/C sources remain authoritative.
