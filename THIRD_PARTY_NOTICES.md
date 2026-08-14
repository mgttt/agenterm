# Third-party notices

AgenTerm uses the following direct Rust dependencies. Their exact resolved
versions and transitive dependency graph are recorded in `Cargo.lock` and the
generated `agenterm-sbom.spdx.json`.
Copyright and license terms remain with their respective authors.

| Package | Declared license |
| --- | --- |
| `ab_glyph` | Apache-2.0 |
| `anyhow` | MIT OR Apache-2.0 |
| `atspi` | Apache-2.0 OR MIT |
| `cc` (dev dependency) | MIT OR Apache-2.0 |
| `itoa` | MIT OR Apache-2.0 |
| `libc` | MIT OR Apache-2.0 |
| `libloading` | ISC |
| `mlua` | MIT |
| `objc2` | MIT |
| `objc2-app-kit` | MIT |
| `objc2-foundation` | MIT |
| `object` | Apache-2.0 OR MIT |
| `png` | MIT OR Apache-2.0 |
| `rhai` | MIT OR Apache-2.0 |
| `rquickjs` | MIT |
| `rusqlite` | MIT |
| `serde` | MIT OR Apache-2.0 |
| `serde_json` | MIT OR Apache-2.0 |
| `sha2` | MIT OR Apache-2.0 |
| `softbuffer` | MIT OR Apache-2.0 |
| `sqlparser` | Apache-2.0 |
| `tempfile` | MIT OR Apache-2.0 |
| `thiserror` | MIT OR Apache-2.0 |
| `tokio` | MIT |
| `unicode-width` | MIT OR Apache-2.0 |
| `ureq` | MIT OR Apache-2.0 |
| `vt100` | MIT |
| `walkdir` | Unlicense/MIT |
| `wasmtime` | Apache-2.0 WITH LLVM-exception |
| `wasmtime-wasi` | Apache-2.0 WITH LLVM-exception |
| `windows-sys` | MIT OR Apache-2.0 |
| `winit` | Apache-2.0 |
| `winresource` (build dependency) | MIT |
| `x11rb` | MIT OR Apache-2.0 |
| `zbus` | MIT |

The corresponding sources and complete license files are available from each
package's entry in the Cargo registry. `scripts/rh/supply-chain.rh` uses
`cargo metadata --locked` as the authoritative inventory, requires this table
to cover every direct dependency, rejects unreviewed license expressions, and
records every resolved transitive package in the SPDX inventory.
