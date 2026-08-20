# TinyArcade deterministic replay v1

TinyArcade replay is a bounded test and bug-reproduction artifact for an exact
standard `.wasm` cartridge. It is not executable code, a second cartridge
format, a save-game replacement or proof that a cartridge is approved for the
official catalog. A `.tareplay` records an initial portable snapshot, canonical
input/clock steps and the length plus SHA-256 of every render/audio result.

The trace binds the complete cartridge SHA-256 as well as the embedded game id,
game version, ABI version and state version. Replay execution always receives
the original `.wasm` bytes and verifies that binding itself before restoring or
ticking the runtime; callers cannot accidentally rely on manifest identity
alone.

## Canonical binary envelope

All integers are unsigned little-endian. Strings are UTF-8 canonical manifest
tokens, without terminators. There is no alignment padding.

| Offset | Bytes | Field |
|---:|---:|---|
| 0 | 4 | ASCII `TAR1` |
| 4 | 2 | format version, exactly `1` |
| 6 | 2 | header size, exactly `64` |
| 8 | 32 | exact cartridge SHA-256 |
| 40 | 4 | ABI version |
| 44 | 4 | state version |
| 48 | 2 | game-id byte length |
| 50 | 2 | game-version byte length |
| 52 | 4 | initial-snapshot byte length |
| 56 | 4 | step count |
| 60 | 4 | reserved, exactly zero |
| 64 | variable | game id, game version, initial snapshot, then steps |

Each step is exactly 80 bytes:

| Relative offset | Bytes | Field |
|---:|---:|---|
| 0 | 4 | canonical TinyArcade button bitset |
| 4 | 4 | monotonic game clock in milliseconds |
| 8 | 4 | render byte length |
| 12 | 4 | audio byte length |
| 16 | 32 | SHA-256 of exact render bytes |
| 48 | 32 | SHA-256 of exact audio bytes |

Decoder ceilings are part of v1: 8 MiB for the whole trace, 1 MiB for the
initial snapshot, 65,536 steps, 128 bytes for game id, 64 bytes for game
version, 64 KiB render and 16 KiB audio per step. Only the nine core v1 input
bits are accepted. Clocks may remain equal but never move backwards. The
decoder proves the complete declared length with checked arithmetic before it
reserves step storage.

The trace contains no frame payloads. Verification regenerates every frame
through the runtime, validates its versioned media records, then compares exact
lengths and digests. This keeps a long replay bounded while still detecting a
one-byte renderer, audio, state, clock, RNG or interpreter regression.

## Converter workflow

Create a text input plan with one `clock_ms buttons` pair per line. Decimal and
`0x` hexadecimal values are accepted; `#` starts a comment.

```text
0 0
16 0x1
32 0x10
48 0x80
```

Then record and independently check it:

```sh
cargo run -p agenterm-tinyvm --bin tinyvm --features replay -- \
  replay record path/to/game.wasm path/to/inputs.txt path/to/game.tareplay

cargo run -p agenterm-tinyvm --bin tinyvm --features replay -- \
  replay check path/to/game.wasm path/to/game.tareplay
```

Recording publishes a new file only and never overwrites an existing trace.
The repository's Depth Well and Paddle Guard input plans plus asserted trace
length/SHA-256 values are format goldens for grid3d, indexed2d and real tone
output.

The CLI deliberately uses the private core-only runtime policy. The Rust replay
API is namespace-neutral: a reviewed cartridge may use future versioned native
imports when the caller constructs the runtime with the same registered exact
signatures. Such modules must provide deterministic behavior for a replay to
match. A trace does not serialize native side effects or grant a missing native
capability; unknown or changed imports still fail during ordinary runtime
construction.

Passing replay verification establishes deterministic compatibility for those
inputs. It does not establish rights, product quality, signature trust, catalog
review or exhaustive gameplay correctness.
