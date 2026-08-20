# TinyArcade media streams v1

`submit_render` and `submit_audio` carry self-identifying, bounded binary
streams. They are not native pointers, GPU commands, JavaScript objects or
archived platform types. Converters emit little-endian records; native hosts
validate a whole stream before rendering or scheduling audio.

## `tinyarcade:grid3d/v1`

The grid frame starts with a 32-byte header:

```text
"TAG3"             4 bytes
version            u16 = 1
header_bytes       u16 = 32
board_width        u16
board_depth        u16
board_height       u16
cell_count         u16
score              u32
cleared_decks      u32
level              u32
flags              u32; bit 0 = game over
```

Exactly `cell_count` eight-byte records follow:

```text
x, y, z            u8 each
kind               u8; 1 settled, 2 active, 3 landing ghost
rgba               u32 little-endian RGBA8
```

Dimensions are non-zero, every coordinate is inside the declared board, every
kind is known, unknown flag bits are rejected and trailing bytes are forbidden.
Consumers use `kind` to draw settled, ghost and active cells in stable visual
priority independent of record order.

## `tinyarcade:indexed2d/v1`

The indexed frame is a complete, uncompressed 2D pixel plane. Its 16-byte
header is:

```text
"TAI2"             4 bytes
version            u16 = 1
header_bytes       u16 = 16
width              u16; 1..512
height             u16; 1..512
palette_count      u16; 1..256
flags              u16 = 0
```

Exactly `palette_count` four-byte colors follow. Each color is encoded as the
four bytes R, G, B, A and is exposed by the Rust/Swift SDKs as one
little-endian RGBA8 `u32`/`UInt32`. The remainder is exactly `width × height`
one-byte palette indices in row-major top-to-bottom order. Every index must be
less than `palette_count`; trailing bytes, unknown flags and malformed sizes
are rejected before native presentation.

Each dimension is at most 512, the pixel plane is at most 65,535 bytes and the
whole stream is at most 64 KiB. Therefore ordinary 256 × 240 and 320 × 200
frames with full 256-color palettes fit the default render budget. This v1
format is deliberately a whole frame rather than a delta, compressed payload
or GPU command list. The native host owns nearest-neighbour scaling, aspect
fit, color-space conversion, compositing and display refresh; the cartridge
cannot address Metal, Core Graphics or platform objects.

The iOS SDK includes a host-side convenience that expands the validated plane
to canonical sRGB RGBA8 and a UIKit view configured for aspect-fit,
nearest-neighbour presentation. This does not change the cartridge protocol:
custom Metal renderers may consume the same palette and indices directly, and
no Apple framework type crosses the WASM boundary.

An indexed cartridge must import `tinyarcade:core/v1.indexed2d_version` with
signature `() -> i32` and check for version 1 during init. This ordinary WASM
import makes compatibility fail at load on runtimes that predate indexed 2D;
emitting `TAI2` without declaring the import traps the current cartridge.

## `tinyarcade:tones/v1`

```text
"TAT1"             4 bytes
version            u16 = 1
event_count        u16
```

Exactly `event_count` eight-byte events follow:

```text
kind               u8; 1 lock, 2 deck clear, 3 game over
reserved           u8 = 0
frequency_hz       u16; 40..20000
duration_ms        u16; 1..2000
amplitude_milli    u16; 0..1000
```

The host owns the synthesizer, mixing, mute policy, interruption behavior and
audio session. A cartridge can request only these bounded semantic cues; it
cannot supply native audio code or address system audio APIs.
