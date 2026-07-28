# Third-party notices

AgenTerm uses the following direct Rust dependencies. Their exact resolved
versions and transitive dependency graph are recorded in `Cargo.lock` and the
generated `agenterm-sbom.spdx.json`.
Copyright and license terms remain with their respective authors.

| Package | Declared license |
| --- | --- |
| `anyhow` | MIT OR Apache-2.0 |
| `libc` | MIT OR Apache-2.0 |
| `png` | MIT OR Apache-2.0 |
| `rhai` | MIT OR Apache-2.0 |
| `rmux-pty` | MIT OR Apache-2.0 |
| `serde` | MIT OR Apache-2.0 |
| `serde_json` | MIT OR Apache-2.0 |
| `softbuffer` | MIT OR Apache-2.0 |
| `vt100` | MIT |
| `windows-sys` | MIT OR Apache-2.0 |
| `winit` | Apache-2.0 |
| `winresource` (build dependency) | MIT |

The corresponding sources and complete license files are available from each
package's entry in the Cargo registry. `scripts/supply-chain.ps1` uses
`cargo metadata --locked` as the authoritative inventory, requires this table
to cover every direct dependency, rejects unreviewed license expressions, and
records every resolved transitive package in the SPDX inventory.
