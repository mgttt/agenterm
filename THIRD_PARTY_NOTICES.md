# Third-party notices

AgenTerm uses the following direct Rust dependencies. Their exact resolved
versions and transitive dependency graph are recorded in `Cargo.lock`.
Copyright and license terms remain with their respective authors.

| Package | Declared license |
| --- | --- |
| `anyhow` | MIT OR Apache-2.0 |
| `png` | MIT OR Apache-2.0 |
| `rhai` | MIT OR Apache-2.0 |
| `rmux-pty` | MIT OR Apache-2.0 |
| `serde` | MIT OR Apache-2.0 |
| `serde_json` | MIT OR Apache-2.0 |
| `vt100` | MIT |
| `windows-sys` | MIT OR Apache-2.0 |
| `winresource` (build dependency) | MIT |

The corresponding sources and complete license files are available from each
package's entry in the Cargo registry. Before a distributable build is
published, `cargo metadata --locked` is the authoritative inventory input for
checking the full resolved graph represented by `Cargo.lock`.
