# TinyArcadeRuntime Swift package

This generated package is the iOS app integration artifact. It contains the
device/simulator `TinyArcade.xcframework` and the main-actor Swift ownership,
media-decoding and signed-catalog wrapper as one `TinyArcadeRuntime` library
product.

Generate a self-contained directory from the repository root:

```sh
rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios
crates/agenterm-tinyvm/build-swift-package.sh \
  dist/TinyArcadeRuntimePackage
```

An app may then add that directory as a local Swift package and depend on the
`TinyArcadeRuntime` product. The generated directory is a build artifact; this
template, the Swift source and Rust/C sources remain authoritative.
